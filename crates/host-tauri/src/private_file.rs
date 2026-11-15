//! Files that name accounts, hosts and paths: readable by their owner only. On Unix that is
//! mode 0600; on Windows they live in the user profile, which only the user can read anyway.

use std::fs::{File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;

#[cfg(unix)]
pub(crate) const PRIVATE_MODE: u32 = 0o600;

/// Opens `path` for writing, appending or truncating; a new file is created private.
pub(crate) fn open(path: &Path, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append);
    #[cfg(unix)]
    options.mode(PRIVATE_MODE);
    options.open(path)
}

/// Makes an existing file private: opening keeps the mode of a file written before.
#[cfg(unix)]
pub(crate) fn restrict(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_MODE))
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature of the Unix version"
)]
pub(crate) fn restrict(_path: &Path) -> io::Result<()> {
    Ok(())
}
