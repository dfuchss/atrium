//! User service definitions on disk: `<config>/services/*.toml`. Only this module reads and
//! writes the folder; parsing is the domain's ([`Catalog::parse_user_files`]).

use std::path::Path;

use domain::{Catalog, ServiceDef};

const README: &str = include_str!("../assets/services-README.md");
const EXAMPLE: &str = include_str!("../assets/example.toml.disabled");

const README_NAME: &str = "README.md";
const EXAMPLE_NAME: &str = "example.toml.disabled";

/// Reads all `*.toml` files of `dir` (sorted by name, so later names win) and parses them.
/// A missing folder is not an error; unreadable files are reported.
pub fn read_user_services(dir: &Path) -> (Vec<ServiceDef>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (vec![], vec![]);
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let (mut files, mut errors) = (vec![], vec![]);
    for path in paths {
        let source = path.display().to_string();
        match std::fs::read_to_string(&path) {
            Ok(text) => files.push((source, text)),
            Err(e) => errors.push(format!("cannot read {source}: {e}")),
        }
    }
    let (defs, parse_errors) = Catalog::parse_user_files(&files);
    errors.extend(parse_errors);
    (defs, errors)
}

/// Creates `dir`; if it holds no `*.toml` and no `README.md`, writes a README and a disabled,
/// fully commented example.
pub fn prepare_services_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let has = |wanted: &dyn Fn(&Path) -> bool| {
        std::fs::read_dir(dir).is_ok_and(|rd| rd.filter_map(Result::ok).any(|e| wanted(&e.path())))
    };
    let has_toml = has(&|p| p.extension().is_some_and(|e| e == "toml"));
    let has_readme = has(&|p| p.file_name().is_some_and(|n| n == README_NAME));
    if !has_toml && !has_readme {
        std::fs::write(dir.join(README_NAME), README)?;
        std::fs::write(dir.join(EXAMPLE_NAME), EXAMPLE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("msgr-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn prepare_writes_docs_only_in_an_empty_folder() {
        let dir = temp("prepare").join("services");
        prepare_services_dir(&dir).unwrap();
        assert!(dir.join(README_NAME).is_file());
        assert!(dir.join(EXAMPLE_NAME).is_file());
        std::fs::remove_file(dir.join(README_NAME)).unwrap();
        std::fs::write(dir.join("my.toml"), "x").unwrap();
        prepare_services_dir(&dir).unwrap();
        assert!(!dir.join(README_NAME).exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn reads_valid_files_and_reports_broken_ones() {
        let dir = temp("read");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.toml"),
            "id='a'\nname='A'\ncolor='#111111'\nurl='https://a.example/'\n",
        )
        .unwrap();
        std::fs::write(dir.join("b.toml"), "id = ").unwrap();
        std::fs::write(dir.join("c.toml.disabled"), "garbage").unwrap();
        let (defs, errors) = read_user_services(&dir);
        assert_eq!(defs.len(), 1);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("b.toml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn example_minimal_lines_form_a_valid_definition() {
        let text = EXAMPLE
            .lines()
            .filter_map(|l| l.strip_prefix("#!"))
            .collect::<Vec<_>>()
            .join("\n");
        Catalog::parse("example", &text).expect("lines marked #! form a valid definition");
    }
}
