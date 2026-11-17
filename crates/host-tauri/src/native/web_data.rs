//! A web data folder per account, named by the account id, where the platform's webviews keep
//! their data in a folder given to them (not on macOS, where `WebKit` keeps data stores): on
//! Linux each folder is a `WebContext` of its own.

use std::io;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime, WebviewBuilder, WebviewWindowBuilder};
use uuid::Uuid;

/// Keeps the view's web data in the account's own folder.
pub(crate) fn isolate_view<R: Runtime>(
    builder: WebviewBuilder<R>,
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewBuilder<R>> {
    Ok(builder.data_directory(private_store_dir(&web_data_root(app)?, store_id)?))
}

/// Keeps the window's web data in the account's folder, with its view: on Linux the same
/// `WebContext`.
pub(crate) fn isolate_window<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<WebviewWindowBuilder<'a, R, M>> {
    Ok(builder.data_directory(private_store_dir(&web_data_root(app)?, store_id)?))
}

/// The accounts that have a web data folder.
pub(crate) fn data_store_ids<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<[u8; 16]>> {
    Ok(list_store_dirs(&web_data_root(app)?)?)
}

/// Deletes the web data folder of `store_id`. The engine may still use the folder of a
/// just-closed account: on Linux Tauri keeps a `WebContext` alive until the app exits
/// (tauri#14626). What it leaves is removed again at the next start.
pub(crate) fn remove_data_store<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
) -> tauri::Result<()> {
    Ok(remove_store_dir(&web_data_root(app)?, store_id)?)
}

/// Folder of the per-account web data folders, `<app local data>/webdata` (on Linux the app
/// data folder). A development config folder (`ATRIUM_CONFIG_DIR`) does not move it, just as
/// the data stores of macOS belong to the app: the startup sweep's guards keep protecting the
/// real data.
fn web_data_root<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(app.path().app_local_data_dir()?.join("webdata"))
}

/// Web data folder of the store `store_id` (an account id) in `root`.
fn store_dir(root: &Path, store_id: [u8; 16]) -> PathBuf {
    root.join(Uuid::from_bytes(store_id).hyphenated().to_string())
}

/// Web data folder of `store_id` in `root`, made private before the webview writes to it: it
/// keeps the account's cookies.
fn private_store_dir(root: &Path, store_id: [u8; 16]) -> io::Result<PathBuf> {
    let dir = store_dir(root, store_id);
    for folder in [root, dir.as_path()] {
        create_private_dir(folder)?;
    }
    Ok(dir)
}

/// Mode of the web data folders: only the user may enter them.
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

/// Creates `folder` with mode `0700`; a folder of an earlier run is tightened too.
#[cfg(unix)]
fn create_private_dir(folder: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(folder)?;
    fs_util::set_mode(folder, PRIVATE_DIR_MODE)
}

/// The stores in `root`: its folders named by a hyphenated UUID; anything else is ignored. A
/// missing `root` holds none.
fn list_store_dirs(root: &Path) -> io::Result<Vec<[u8; 16]>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| Uuid::try_parse(name).ok()) else {
            continue;
        };
        if store_dir(root, id.into_bytes()) == entry.path() {
            ids.push(id.into_bytes());
        }
    }
    Ok(ids)
}

/// Deletes the web data folder of `store_id` in `root`; one that is already gone counts as
/// deleted.
fn remove_store_dir(root: &Path, store_id: [u8; 16]) -> io::Result<()> {
    match std::fs::remove_dir_all(store_dir(root, store_id)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_are_the_uuid_folders() {
        let root = tempfile::tempdir().unwrap();
        let (kept, removed) = (Uuid::from_u128(1), Uuid::from_u128(2));
        for id in [kept, removed] {
            std::fs::create_dir(store_dir(root.path(), id.into_bytes())).unwrap();
        }
        std::fs::create_dir(root.path().join(kept.simple().to_string())).unwrap();
        std::fs::create_dir(root.path().join("not-a-store")).unwrap();
        std::fs::write(root.path().join(Uuid::from_u128(3).to_string()), "x").unwrap();
        let mut listed = list_store_dirs(root.path()).unwrap();
        listed.sort_unstable();
        assert_eq!(listed, vec![kept.into_bytes(), removed.into_bytes()]);

        remove_store_dir(root.path(), removed.into_bytes()).unwrap();
        remove_store_dir(root.path(), removed.into_bytes()).unwrap();
        assert_eq!(
            list_store_dirs(root.path()).unwrap(),
            vec![kept.into_bytes()]
        );
        assert!(
            list_store_dirs(&root.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn store_folders_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("webdata");
        let (fresh, earlier) = (Uuid::from_u128(1), Uuid::from_u128(2));
        std::fs::create_dir_all(store_dir(&root, earlier.into_bytes())).unwrap();
        fs_util::set_mode(&root, 0o755).unwrap();
        for id in [fresh, earlier] {
            let dir = private_store_dir(&root, id.into_bytes()).unwrap();
            assert_eq!(dir, store_dir(&root, id.into_bytes()));
            for folder in [&root, &dir] {
                let mode = std::fs::metadata(folder).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700, "{}", folder.display());
            }
        }
    }
}
