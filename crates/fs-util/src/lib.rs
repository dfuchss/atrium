//! File operations shared by Atrium's crates.

use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;

/// Writes `bytes` to `path` with the permission bits `mode`, whatever the umask is, through the
/// temporary file `.<name>.tmp` next to it: a reader never sees half of it, and a crash leaves
/// the old file. Creates the missing folders of `path` and syncs the folder after the rename so
/// the new file survives a power loss.
pub fn write_atomically(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(folder)?;
    let name = path
        .file_name()
        .map_or_default(|name| name.to_string_lossy().into_owned());
    let temporary = path.with_file_name(format!(".{name}.tmp"));
    let written = write_and_sync(&temporary, bytes, mode)
        .and_then(|()| std::fs::rename(&temporary, path))
        .and_then(|()| File::open(folder)?.sync_all());
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// Writes `bytes` to `path` with the permission bits `mode` and syncs it. A file left by a
/// crash keeps its old mode on open, so the mode is set again.
fn write_and_sync(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    file.set_permissions(Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODE: u32 = 0o640;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn writes_with_the_mode_into_new_folders_and_leaves_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("nested/file.json");
        write_atomically(&path, b"first", MODE).unwrap();
        write_atomically(&path, b"second", MODE).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(mode(&path), MODE);
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(files, 1);
    }

    #[test]
    fn replaces_a_file_of_another_mode() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o666)).unwrap();
        write_atomically(&path, b"new", 0o600).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn a_leftover_temporary_file_gets_the_mode() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.json");
        let leftover = root.path().join(".file.json.tmp");
        std::fs::write(&leftover, "left over by a crash").unwrap();
        std::fs::set_permissions(&leftover, Permissions::from_mode(0o644)).unwrap();
        write_atomically(&path, b"new", MODE).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(mode(&path), MODE);
        assert!(!leftover.exists());
    }

    #[test]
    fn a_failed_write_leaves_no_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("folder");
        std::fs::create_dir_all(path.join("inside")).unwrap();
        assert!(write_atomically(&path, b"text", MODE).is_err());
        let files = std::fs::read_dir(root.path()).unwrap().count();
        assert_eq!(files, 1);
    }
}
