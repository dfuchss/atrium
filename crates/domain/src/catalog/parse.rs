//! Reading a definition from TOML: syntax, types and values checked, with the position of an
//! error for the logs and the editor.

use protocol::ServiceProblem;

use super::{Catalog, CatalogError, ServiceDef};

/// [`Catalog::parse`] that also returns the byte range of a TOML syntax or type error.
fn parse_located(
    source: &str,
    toml_text: &str,
) -> Result<ServiceDef, (CatalogError, Option<std::ops::Range<usize>>)> {
    let parse_err = |message: String| CatalogError::Parse {
        source: source.to_string(),
        message,
    };
    let def: ServiceDef = toml::from_str(toml_text)
        .map_err(|error| (parse_err(error.message().to_string()), error.span()))?;
    if def.id.trim().is_empty() {
        return Err((parse_err("empty id".into()), None));
    }
    if def.name.trim().is_empty() {
        return Err((parse_err("empty name".into()), None));
    }
    if !is_hex_color(&def.color) {
        let message = format!("invalid color {:?} (#rgb or #rrggbb expected)", def.color);
        return Err((parse_err(message), None));
    }
    let rules = [&def.unread.title_regex, &def.unread.title_indirect_regex];
    for pattern in rules.into_iter().flatten() {
        regex::Regex::new(pattern).map_err(|error| {
            let invalid = CatalogError::InvalidRegex {
                service: source.to_string(),
                message: error.to_string(),
            };
            (invalid, None)
        })?;
    }
    Ok(def)
}

/// 1-based line and column (in characters) of the byte `offset` in `text`; an offset past the
/// end or inside a character is clamped to the nearest earlier boundary.
fn line_and_column(text: &str, offset: usize) -> (u32, u32) {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before[line_start..].chars().count() + 1;
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
    )
}

/// True for a hex color `#` + 3 to 8 hex digits. The UI puts service colors into a style
/// attribute, so nothing else may get through.
pub(super) fn is_hex_color(color: &str) -> bool {
    color.strip_prefix('#').is_some_and(|digits| {
        (3..=8).contains(&digits.len()) && digits.bytes().all(|digit| digit.is_ascii_hexdigit())
    })
}

impl Catalog {
    /// Parses one TOML definition and validates its regexes. `source` names it in errors.
    pub fn parse(source: &str, toml_text: &str) -> Result<ServiceDef, CatalogError> {
        parse_located(source, toml_text).map_err(|(error, span)| match (error, span) {
            (CatalogError::Parse { source, message }, Some(span)) => {
                let (line, column) = line_and_column(toml_text, span.start);
                CatalogError::Parse {
                    source,
                    message: format!("{message} (line {line}, column {column})"),
                }
            }
            (error, _) => error,
        })
    }

    /// The problems of a definition text as the editor shows them: the same checks as
    /// [`Catalog::parse`], with line and column where the TOML parser knows a position.
    pub fn diagnose(toml_text: &str) -> Vec<ServiceProblem> {
        let Err((error, span)) = parse_located("", toml_text) else {
            return vec![];
        };
        let message = match error {
            CatalogError::Parse { message, .. } => message,
            CatalogError::InvalidRegex { message, .. } => {
                format!("invalid regular expression: {message}")
            }
            other => other.to_string(),
        };
        let position = span.map(|span| line_and_column(toml_text, span.start));
        vec![ServiceProblem {
            message,
            line: position.map(|(line, _)| line),
            column: position.map(|(_, column)| column),
        }]
    }

    /// Parses the user's definition files, given as `(source, text)` in the order they are
    /// applied (later files win). Invalid files are skipped and reported as messages.
    pub fn parse_user_files(files: &[(String, String)]) -> (Vec<ServiceDef>, Vec<String>) {
        let (mut defs, mut errors) = (vec![], vec![]);
        for (source, text) in files {
            match Catalog::parse(source, text) {
                Ok(def) => defs.push(def),
                Err(error) => errors.push(format!("invalid service definition: {error}")),
            }
        }
        (defs, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_and_column_counts_characters_from_one() {
        let text = "ab\nc\u{e4}d\n";
        assert_eq!(line_and_column(text, 0), (1, 1));
        assert_eq!(line_and_column(text, 2), (1, 3));
        assert_eq!(line_and_column(text, 3), (2, 1));
        assert_eq!(line_and_column(text, 6), (2, 3));
        assert_eq!(line_and_column(text, 8), (3, 1));
        assert_eq!(line_and_column(text, 400), (3, 1));
        // Offset 5 is inside the two-byte "\u{e4}": clamped to its start.
        assert_eq!(line_and_column(text, 5), (2, 2));
    }

    #[test]
    fn diagnose_accepts_a_valid_definition() {
        assert!(
            Catalog::diagnose("id='a'\nname='A'\ncolor='#111111'\nurl='https://a/'").is_empty()
        );
    }

    #[test]
    fn diagnose_locates_syntax_errors() {
        let problems = Catalog::diagnose("id = 'a'\nname = \n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, Some(2));
        assert!(problems[0].column.is_some());
        assert!(
            !problems[0].message.contains('\n'),
            "{:?}",
            problems[0].message
        );
    }

    #[test]
    fn diagnose_reports_missing_fields_and_bad_regexes() {
        let missing = Catalog::diagnose("id = 'a'\nname = 'A'\n");
        assert!(missing[0].message.contains("url") || missing[0].message.contains("color"));
        let regex =
            Catalog::diagnose("id='a'\nname='A'\ncolor='#111'\nurl='x'\n[unread]\ntitle_regex='('");
        assert!(regex[0].message.starts_with("invalid regular expression"));
        assert_eq!(regex[0].line, None);
        let empty_id = Catalog::diagnose("id=' '\nname='A'\ncolor='#111'\nurl='x'");
        assert_eq!(empty_id[0].message, "empty id");
    }

    #[test]
    fn parse_errors_in_logs_name_the_position() {
        let error = Catalog::parse("f.toml", "id = 'a'\nname = ").unwrap_err();
        assert!(error.to_string().contains("line 2"), "{error}");
    }

    #[test]
    fn parse_errors() {
        assert!(matches!(
            Catalog::parse("s", "garbage ="),
            Err(CatalogError::Parse { .. })
        ));
        assert!(matches!(
            Catalog::parse("s", "id=''\nname='a'\ncolor='#000'\nurl='u'"),
            Err(CatalogError::Parse { .. })
        ));
        let bad = "id='a'\nname='a'\ncolor='#000'\nurl='u'\n[unread]\ntitle_regex='('";
        match Catalog::parse("src", bad) {
            Err(CatalogError::InvalidRegex { service, .. }) => assert_eq!(service, "src"),
            other => panic!("{other:?}"),
        }
    }
}
