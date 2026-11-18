//! Export files: the content of `accounts.json` and the user's service definition files in one
//! JSON document.

use std::path::Path;

use domain::Persisted;
use protocol::TransferScope;
use serde::{Deserialize, Serialize};

use crate::{ACCOUNTS_FILE_MODE, FsStore, services};

const FORMAT: &str = "atrium-export";
const VERSION: u32 = 1;

/// Largest export file read: custom icons make it big, but not this big.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct ExportFile {
    #[serde(flatten)]
    header: Header,
    accounts: Persisted,
    service_files: Vec<ServiceFileText>,
}

/// Read on its own first, so a file of another kind or a newer version is refused as such
/// instead of failing on its content.
#[derive(Serialize, Deserialize)]
struct Header {
    format: String,
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct ServiceFileText {
    name: String,
    text: String,
}

/// Writes the saved state of `store` (for [`TransferScope::Settings`] without the accounts)
/// and the definition files of `services_dir` to `path`.
pub fn write_export(
    path: &Path,
    store: &FsStore,
    services_dir: &Path,
    scope: TransferScope,
) -> Result<(), String> {
    let mut accounts = load(store)?;
    if scope == TransferScope::Settings {
        accounts.accounts.clear();
        accounts.settings.active = None;
    }
    let file = ExportFile {
        header: Header {
            format: FORMAT.into(),
            version: VERSION,
        },
        accounts,
        service_files: read_service_files(services_dir)?,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
    fs_util::write_atomically(path, &json, ACCOUNTS_FILE_MODE)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Reads the export file `path` and adds its service files to `services_dir`, keeping existing
/// ones. Returns the saved state of the file and why service files were skipped. A
/// [`TransferScope::Full`] import is refused while `store` holds accounts.
pub fn import(
    path: &Path,
    store: &FsStore,
    services_dir: &Path,
    scope: TransferScope,
) -> Result<(Persisted, Vec<String>), String> {
    let cannot_read = |error| format!("cannot read {}: {error}", path.display());
    if std::fs::metadata(path).map_err(cannot_read)?.len() > MAX_FILE_BYTES {
        return Err("the file is too large for an Atrium export".into());
    }
    let json = std::fs::read_to_string(path).map_err(cannot_read)?;
    let file = parse(&json)?;
    if scope == TransferScope::Full && !load(store)?.accounts.is_empty() {
        return Err("a full import needs an Atrium without accounts. Remove the accounts first, or import the settings only.".into());
    }
    let skipped = file
        .service_files
        .iter()
        .filter_map(|file| {
            match services::save_service_file(services_dir, &file.name, &file.text, true) {
                Ok(problems) => problems
                    .first()
                    .map(|problem| format!("{}: {}", file.name, problem.message)),
                Err(error) => Some(error),
            }
        })
        .collect();
    Ok((file.accounts, skipped))
}

/// Reads without setting a corrupt accounts file aside, which only the start does.
fn load(store: &FsStore) -> Result<Persisted, String> {
    store.read().map_err(|error| error.to_string())
}

fn parse(json: &str) -> Result<ExportFile, String> {
    let header: Header = serde_json::from_str(json)
        .map_err(|error| format!("this is not an Atrium export file ({error})"))?;
    if header.format != FORMAT {
        return Err("this is not an Atrium export file".into());
    }
    if header.version > VERSION {
        return Err(format!(
            "the file was exported by a newer Atrium (format version {}); update Atrium first",
            header.version
        ));
    }
    serde_json::from_str(json).map_err(|error| format!("the export file is damaged: {error}"))
}

/// The definition files of `dir` with their text; the generated example is not the user's.
fn read_service_files(dir: &Path) -> Result<Vec<ServiceFileText>, String> {
    services::list_service_files(dir)
        .into_iter()
        .filter(|file| file.name != services::EXAMPLE_NAME)
        .map(|file| {
            services::read_service_file(dir, &file.name).map(|text| ServiceFileText {
                name: file.name,
                text,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Account;
    use domain::ports::Store as _;

    const SERVICE: &str = "id = \"chat\"\nname = \"Chat\"\ncolor = \"#123456\"\nurl = \"https://chat.example.org/\"\n";

    /// A config folder whose store holds one active account, with DND on.
    fn source() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path());
        let mut saved = Persisted::default();
        let id = uuid::Uuid::from_u128(1);
        saved.accounts.push(Account::new(id, "slack", "Work", None));
        saved.settings.active = Some(id);
        saved.settings.app.dnd = true;
        store.save(&saved).unwrap();
        (dir, store)
    }

    fn export(scope: TransferScope) -> (tempfile::TempDir, std::path::PathBuf) {
        let (dir, store) = source();
        services::save_service_file(dir.path(), "chat.toml", SERVICE, true).unwrap();
        services::save_service_file(dir.path(), "kept.toml", SERVICE, true).unwrap();
        let path = dir.path().join("export.json");
        write_export(&path, &store, dir.path(), scope).unwrap();
        (dir, path)
    }

    #[test]
    fn import_takes_the_export_and_keeps_existing_service_files() {
        let (source, path) = export(TransferScope::Full);
        let target = tempfile::tempdir().unwrap();
        let own = SERVICE.replace("Chat", "Mine");
        services::save_service_file(target.path(), "kept.toml", &own, true).unwrap();

        let store = FsStore::new(target.path());
        let (saved, skipped) = import(&path, &store, target.path(), TransferScope::Full).unwrap();

        assert_eq!(saved, FsStore::new(source.path()).load().unwrap());
        assert_eq!(skipped, ["kept.toml already exists"]);
        let read = |name| services::read_service_file(target.path(), name).unwrap();
        assert_eq!(read("chat.toml"), SERVICE);
        assert_eq!(read("kept.toml"), own);
    }

    #[test]
    fn settings_export_leaves_out_the_accounts() {
        let (dir, path) = export(TransferScope::Settings);
        let (saved, _) = import(
            &path,
            &FsStore::new(dir.path()),
            dir.path(),
            TransferScope::Settings,
        )
        .unwrap();
        assert!(saved.accounts.is_empty());
        assert_eq!(saved.settings.active, None);
        assert!(saved.settings.app.dnd);
    }

    #[test]
    fn full_import_is_refused_while_accounts_exist() {
        let (_source, path) = export(TransferScope::Full);
        let (target, store) = source();
        let error = import(&path, &store, target.path(), TransferScope::Full).unwrap_err();
        assert!(error.contains("without accounts"), "{error}");
        assert!(services::list_service_files(target.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn export_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let (_dir, path) = export(TransferScope::Full);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, ACCOUNTS_FILE_MODE);
    }

    #[test]
    fn too_large_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.json");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let store = FsStore::new(dir.path());
        let error = import(&path, &store, dir.path(), TransferScope::Settings).unwrap_err();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn other_formats_and_newer_versions_are_refused() {
        for (json, expected) in [
            ("{}", "not an Atrium export file"),
            (
                r#"{"format":"other","version":1}"#,
                "not an Atrium export file",
            ),
            (
                r#"{"format":"atrium-export","version":2,"accounts":[]}"#,
                "newer Atrium",
            ),
            (
                r#"{"format":"atrium-export","version":1,"accounts":[]}"#,
                "damaged",
            ),
        ] {
            let error = parse(json).err().expect(json);
            assert!(error.contains(expected), "{json}: {error}");
        }
    }
}
