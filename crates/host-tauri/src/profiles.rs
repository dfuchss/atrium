//! Per-account browsing data (cookies, storage, caches), keyed by the account's store id.
//!
//! macOS: a persistent `WebKit` data store per identifier (macOS 14+), managed by `WebKit`.
//! Windows and Linux: a data directory per account under `<app local data>/profiles`; wry
//! gives every directory its own `WebView2` user data folder or `WebKitGTK` web context.

#[cfg(target_os = "macos")]
pub(crate) use data_stores::{list, remove};
#[cfg(not(target_os = "macos"))]
pub(crate) use directories::{directory, list, remove};

#[cfg(target_os = "macos")]
mod data_stores {
    use tauri::{AppHandle, Runtime};

    /// Ids of all stores of this app.
    pub(crate) fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<[u8; 16]>, String> {
        tauri::async_runtime::block_on(app.fetch_data_store_identifiers())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn remove<R: Runtime>(app: &AppHandle<R>, store_id: [u8; 16]) -> Result<(), String> {
        tauri::async_runtime::block_on(app.remove_data_store(store_id))
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod directories {
    use std::io::ErrorKind;
    use std::path::PathBuf;

    use tauri::{AppHandle, Manager, Runtime};
    use uuid::Uuid;

    fn profiles_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
        let data_dir = app
            .path()
            .app_local_data_dir()
            .map_err(|error| error.to_string())?;
        Ok(data_dir.join("profiles"))
    }

    /// `<app local data>/profiles/<store id>`.
    pub(crate) fn directory<R: Runtime>(
        app: &AppHandle<R>,
        store_id: [u8; 16],
    ) -> Result<PathBuf, String> {
        Ok(profiles_dir(app)?.join(Uuid::from_bytes(store_id).simple().to_string()))
    }

    /// Ids of all stores of this app: the profile directories named by an id.
    pub(crate) fn list<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<[u8; 16]>, String> {
        let dir = profiles_dir(app)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("{}: {error}", dir.display())),
        };
        Ok(entries
            .filter_map(Result::ok)
            .filter_map(|entry| Uuid::try_parse(entry.file_name().to_str()?).ok())
            .map(Uuid::into_bytes)
            .collect())
    }

    /// Fails while an engine process still holds files of the profile (`WebView2` lingers a
    /// moment after its last view closed); the caller retries.
    pub(crate) fn remove<R: Runtime>(app: &AppHandle<R>, store_id: [u8; 16]) -> Result<(), String> {
        let dir = directory(app, store_id)?;
        match std::fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                Err(format!("{}: {error}", dir.display()))
            }
            _ => Ok(()),
        }
    }
}
