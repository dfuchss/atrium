//! User service definitions on disk: `<config>/services/*.toml`. Only this module reads and
//! writes the folder; parsing is the domain's ([`Catalog::parse_user_files`]).

use std::io::Write;
use std::path::{Path, PathBuf};

use domain::{Catalog, ServiceDef};
use protocol::{ServiceFile, ServiceProblem};

const README: &str = include_str!("../assets/services-README.md");
const EXAMPLE: &str = include_str!("../assets/example.toml.disabled");

const README_NAME: &str = "README.md";
const EXAMPLE_NAME: &str = "example.toml.disabled";

const ENABLED_SUFFIX: &str = ".toml";
const DISABLED_SUFFIX: &str = ".toml.disabled";
const MAX_FILE_NAME_STEM: usize = 64;

/// Reads all `*.toml` files of `dir` (sorted by name, so later names win) and parses them.
/// A missing folder is not an error; unreadable files are reported.
pub fn read_user_services(dir: &Path) -> (Vec<ServiceDef>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (vec![], vec![]);
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    paths.sort();
    let (mut files, mut errors) = (vec![], vec![]);
    for path in paths {
        let source = path.display().to_string();
        match std::fs::read_to_string(&path) {
            Ok(text) => files.push((source, text)),
            Err(error) => errors.push(format!("cannot read {source}: {error}")),
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
    let contains = |wanted: &dyn Fn(&Path) -> bool| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| wanted(&entry.path()))
        })
    };
    let has_toml = contains(&|path| {
        path.extension()
            .is_some_and(|extension| extension == "toml")
    });
    let has_readme = contains(&|path| path.file_name().is_some_and(|name| name == README_NAME));
    if !has_toml && !has_readme {
        std::fs::write(dir.join(README_NAME), README)?;
        std::fs::write(dir.join(EXAMPLE_NAME), EXAMPLE)?;
    }
    Ok(())
}

/// Checks a file name coming from the UI: `[a-z0-9_-]+` plus `.toml` or `.toml.disabled`. This
/// also rules out path separators and `..`, so the file stays inside the services folder.
pub fn validate_file_name(name: &str) -> Result<(), String> {
    let stem = name
        .strip_suffix(DISABLED_SUFFIX)
        .or_else(|| name.strip_suffix(ENABLED_SUFFIX))
        .ok_or("the file name must end in .toml")?;
    if stem.is_empty() || stem.len() > MAX_FILE_NAME_STEM {
        return Err(format!(
            "the file name needs 1 to {MAX_FILE_NAME_STEM} characters before .toml"
        ));
    }
    let allowed = |character: char| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'
            || character == '_'
    };
    if stem.chars().all(allowed) {
        Ok(())
    } else {
        Err("use only a-z, 0-9, - and _ in the file name".into())
    }
}

/// The enabled and disabled definition files of `dir`, sorted by name. A missing folder is empty.
pub fn list_service_files(dir: &Path) -> Vec<ServiceFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files: Vec<ServiceFile> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| validate_file_name(name).is_ok())
        .map(|name| ServiceFile {
            enabled: name.ends_with(ENABLED_SUFFIX),
            name,
        })
        .collect();
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

fn file_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    validate_file_name(name)?;
    Ok(dir.join(name))
}

/// Contents of one definition file.
pub fn read_service_file(dir: &Path, name: &str) -> Result<String, String> {
    std::fs::read_to_string(file_path(dir, name)?)
        .map_err(|error| format!("cannot read {name}: {error}"))
}

/// Text for a new definition: the example without its "ignored file" header, with the minimal
/// definition uncommented and the optional fields left as comments.
pub fn new_service_template() -> String {
    let body = EXAMPLE.split_once("\n\n").map_or(EXAMPLE, |(_, rest)| rest);
    let uncommented = body
        .lines()
        .map(|line| {
            line.strip_prefix("#! ")
                .or_else(|| line.strip_prefix("#!"))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("# Remove the leading \"# \" of an optional line to use it.\n\n{uncommented}\n")
}

/// Writes `text` to `path` through a temporary file in the same folder, so a crash never leaves
/// a half-written definition. Owner-only, like the account file.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// The other spelling of a file name: `a.toml` <-> `a.toml.disabled`.
fn counterpart(name: &str) -> String {
    name.strip_suffix(".disabled")
        .map_or_else(|| format!("{name}.disabled"), str::to_string)
}

/// Validates and writes a definition. `Ok` with problems means the text was refused and nothing
/// was written; `Err` is a name or disk error. `create` refuses to replace an existing file.
pub fn save_service_file(
    dir: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> Result<Vec<ServiceProblem>, String> {
    let path = file_path(dir, name)?;
    let problems = Catalog::diagnose(text);
    if !problems.is_empty() {
        return Ok(problems);
    }
    if create && (path.exists() || dir.join(counterpart(name)).exists()) {
        return Err(format!("{name} already exists"));
    }
    prepare_services_dir(dir).map_err(|error| format!("cannot create the folder: {error}"))?;
    write_atomically(&path, text).map_err(|error| format!("cannot write {name}: {error}"))?;
    Ok(vec![])
}

pub fn delete_service_file(dir: &Path, name: &str) -> Result<(), String> {
    std::fs::remove_file(file_path(dir, name)?)
        .map_err(|error| format!("cannot delete {name}: {error}"))
}

/// Renames `name` to its enabled or disabled spelling; refuses to replace an existing file.
pub fn set_service_file_enabled(dir: &Path, name: &str, enabled: bool) -> Result<(), String> {
    let from = file_path(dir, name)?;
    let target_name = if enabled {
        name.strip_suffix(".disabled").unwrap_or(name).to_string()
    } else if name.ends_with(DISABLED_SUFFIX) {
        name.to_string()
    } else {
        counterpart(name)
    };
    let target = dir.join(&target_name);
    if target == from {
        return Ok(());
    }
    if target.exists() {
        return Err(format!("{target_name} already exists"));
    }
    std::fs::rename(&from, &target).map_err(|error| format!("cannot rename {name}: {error}"))
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

    const VALID: &str = "id='a'\nname='A'\ncolor='#111111'\nurl='https://a.example/'\n";

    #[test]
    fn file_names_are_restricted_to_the_folder() {
        for good in ["a.toml", "my-service_2.toml", "a.toml.disabled"] {
            assert!(validate_file_name(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            ".toml",
            "a",
            "a.txt",
            "A.toml",
            "a b.toml",
            "../a.toml",
            "a/b.toml",
            "..\\a.toml",
            "a.toml/",
            ".hidden.toml",
            "a.toml.tmp",
            "\u{e4}.toml",
            "a.TOML",
        ] {
            assert!(validate_file_name(bad).is_err(), "{bad}");
        }
        assert!(validate_file_name(&format!("{}.toml", "a".repeat(65))).is_err());
    }

    #[test]
    fn template_is_valid_and_documented() {
        let template = new_service_template();
        assert!(Catalog::diagnose(&template).is_empty(), "{template}");
        assert!(template.contains("# instance_label"));
        assert!(!template.contains("#!"));
        assert!(!template.contains("ignored"));
    }

    #[test]
    fn saving_validates_then_writes_atomically_with_owner_only_mode() {
        let dir = temp("save");
        let problems = save_service_file(&dir, "a.toml", "id = ", true).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(
            !dir.join("a.toml").exists(),
            "invalid text must not be written"
        );
        assert!(
            save_service_file(&dir, "a.toml", VALID, true)
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read_to_string(dir.join("a.toml")).unwrap(), VALID);
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        #[cfg(unix)]
        {
            let mode = std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(dir.join("a.toml")).unwrap().permissions(),
            );
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_refuses_to_replace_but_editing_replaces() {
        let dir = temp("replace");
        save_service_file(&dir, "a.toml", VALID, true).unwrap();
        assert!(save_service_file(&dir, "a.toml", VALID, true).is_err());
        set_service_file_enabled(&dir, "a.toml", false).unwrap();
        assert!(save_service_file(&dir, "a.toml", VALID, true).is_err());
        let changed = VALID.replace("'A'", "'B'");
        assert!(
            save_service_file(&dir, "a.toml.disabled", &changed, false)
                .unwrap()
                .is_empty()
        );
        assert!(
            read_service_file(&dir, "a.toml.disabled")
                .unwrap()
                .contains("'B'")
        );
        assert!(save_service_file(&dir, "../x.toml", VALID, true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_toggles_and_deletes_files() {
        let dir = temp("manage");
        save_service_file(&dir, "b.toml", VALID, true).unwrap();
        save_service_file(&dir, "a.toml", VALID, true).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let names = |files: Vec<ServiceFile>| {
            files
                .into_iter()
                .map(|file| (file.name, file.enabled))
                .collect::<Vec<_>>()
        };
        let before = names(list_service_files(&dir));
        assert!(before.contains(&("a.toml".into(), true)));
        assert!(before.iter().all(|(name, _)| name != "notes.txt"));
        set_service_file_enabled(&dir, "a.toml", false).unwrap();
        assert!(names(list_service_files(&dir)).contains(&("a.toml.disabled".into(), false)));
        assert_eq!(read_user_services(&dir).0.len(), 1);
        std::fs::write(dir.join("a.toml"), VALID).unwrap();
        assert!(set_service_file_enabled(&dir, "a.toml.disabled", true).is_err());
        set_service_file_enabled(&dir, "b.toml", true).unwrap();
        delete_service_file(&dir, "a.toml.disabled").unwrap();
        assert!(delete_service_file(&dir, "a.toml.disabled").is_err());
        assert!(delete_service_file(&dir, "../notes.txt").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
