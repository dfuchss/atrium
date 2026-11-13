//! The release zip: its entry names are checked before `ditto` extracts it, and the extracted
//! tree is checked for symbolic links that leave it (zip slip).

use std::path::{Component, Path, PathBuf};
use std::process::Command;

const END_OF_CENTRAL_DIRECTORY: &[u8; 4] = b"PK\x05\x06";
const CENTRAL_FILE_HEADER: &[u8; 4] = b"PK\x01\x02";
const END_RECORD_LEN: usize = 22;
const CENTRAL_HEADER_LEN: usize = 46;
const MAX_COMMENT_LEN: usize = 0xFFFF;
const FILE_TYPE_MASK: u32 = 0o170_000;
const SYMLINK_TYPE: u32 = 0o120_000;

/// One entry of the zip's central directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZipEntry {
    pub(crate) name: String,
    /// The Unix mode `ditto` stores in the upper half of the external attributes.
    pub(crate) unix_mode: u32,
}

impl ZipEntry {
    fn is_symlink(&self) -> bool {
        self.unix_mode & FILE_TYPE_MASK == SYMLINK_TYPE
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Option<usize> {
    let pair = bytes.get(at..at + 2)?;
    Some(usize::from(u16::from_le_bytes([pair[0], pair[1]])))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let quad = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

/// Lists the central directory of a zip archive. ZIP64 archives are refused; release zips
/// are far below 4 GiB.
pub(crate) fn list_entries(zip: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let earliest = zip.len().saturating_sub(END_RECORD_LEN + MAX_COMMENT_LEN);
    let end = (earliest..=zip.len().saturating_sub(END_RECORD_LEN))
        .rev()
        .find(|&at| zip[at..].starts_with(END_OF_CENTRAL_DIRECTORY))
        .ok_or("not a zip archive")?;
    let count = read_u16(zip, end + 10).ok_or("truncated zip")?;
    let offset = read_u32(zip, end + 16).ok_or("truncated zip")?;
    if count == 0xFFFF || offset == u32::MAX {
        return Err("ZIP64 archives are not supported".into());
    }
    let mut position = usize::try_from(offset).map_err(|_| "zip offset out of range")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let (entry, next) = central_entry(zip, position).ok_or("broken zip directory")?;
        entries.push(entry);
        position = next;
    }
    Ok(entries)
}

/// The entry at `position` and where the next one starts.
fn central_entry(zip: &[u8], position: usize) -> Option<(ZipEntry, usize)> {
    if !zip.get(position..)?.starts_with(CENTRAL_FILE_HEADER) {
        return None;
    }
    let name_len = read_u16(zip, position + 28)?;
    let extra_len = read_u16(zip, position + 30)?;
    let comment_len = read_u16(zip, position + 32)?;
    let attributes = read_u32(zip, position + 38)?;
    let name_start = position + CENTRAL_HEADER_LEN;
    let name = zip.get(name_start..name_start + name_len)?;
    let entry = ZipEntry {
        name: String::from_utf8_lossy(name).into_owned(),
        unix_mode: attributes >> 16,
    };
    Some((entry, name_start + name_len + extra_len + comment_len))
}

/// Refuses absolute names, `..`, backslashes and entries below a symbolic link entry (which
/// would be written wherever the link points).
pub(crate) fn check_entries(entries: &[ZipEntry]) -> Result<(), String> {
    let links: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.is_symlink())
        .map(|entry| entry.name.trim_end_matches('/'))
        .collect();
    for entry in entries {
        let name = entry.name.as_str();
        let unsafe_name = name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains('\0')
            || name.split('/').any(|part| part == "..");
        if unsafe_name {
            return Err(format!("refusing zip entry {name:?}"));
        }
        if links.iter().any(|link| {
            name.strip_prefix(link)
                .is_some_and(|rest| rest.starts_with('/'))
        }) {
            return Err(format!("refusing zip entry {name:?} below a symbolic link"));
        }
    }
    Ok(())
}

/// Whether the link at `link` (relative to the extraction root) pointing to `target` stays
/// inside the root.
pub(crate) fn link_stays_inside(link: &Path, target: &Path) -> bool {
    let mut depth: usize = 0;
    let parent_parts = link.parent().map(Path::components).into_iter().flatten();
    for component in parent_parts.chain(target.components()) {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// Every symbolic link below `root`, relative to it, with its target.
fn symlinks_below(root: &Path) -> std::io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut links = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            if kind.is_symlink() {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                links.push((relative, std::fs::read_link(&path)?));
            } else if kind.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(links)
}

/// Checks `zip` and extracts it with `ditto` (keeps modes and bundle structure) into the new
/// folder `target`; then refuses any symbolic link pointing outside of `target`.
pub(crate) fn extract(zip: &Path, target: &Path) -> Result<(), String> {
    let bytes = std::fs::read(zip).map_err(|error| format!("cannot read the archive: {error}"))?;
    check_entries(&list_entries(&bytes)?)?;
    std::fs::create_dir_all(target).map_err(|error| error.to_string())?;
    let output = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(target)
        .output()
        .map_err(|error| format!("cannot run ditto: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ditto failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let links = symlinks_below(target).map_err(|error| error.to_string())?;
    match links
        .iter()
        .find(|(link, destination)| !link_stays_inside(link, destination))
    {
        Some((link, _)) => Err(format!(
            "refusing symbolic link {} that leaves the archive",
            link.display()
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, unix_mode: u32) -> ZipEntry {
        ZipEntry {
            name: name.into(),
            unix_mode,
        }
    }

    /// Zips `source` with `ditto -c -k --keepParent`, like the release workflow.
    fn ditto_zip(source: &Path, zip: &Path) {
        let status = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(source)
            .arg(zip)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn lists_the_entries_of_a_ditto_zip() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Atrium.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/atrium"), "binary").unwrap();
        std::os::unix::fs::symlink("MacOS/atrium", app.join("Contents/link")).unwrap();
        let zip = dir.path().join("atrium.zip");
        ditto_zip(&app, &zip);
        let entries = list_entries(&std::fs::read(&zip).unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(
            names.contains(&"Atrium.app/Contents/MacOS/atrium"),
            "{names:?}"
        );
        let link = entries
            .iter()
            .find(|entry| entry.name == "Atrium.app/Contents/link")
            .unwrap();
        assert!(link.is_symlink());
        check_entries(&entries).unwrap();
        let target = dir.path().join("extracted");
        extract(&zip, &target).unwrap();
        assert!(target.join("Atrium.app/Contents/MacOS/atrium").is_file());
    }

    #[test]
    fn refuses_garbage() {
        assert!(list_entries(b"no zip here").is_err());
        assert!(list_entries(b"").is_err());
    }

    #[test]
    fn refuses_escaping_entry_names() {
        for name in ["/etc/passwd", "../x", "a/../../x", "a\\..\\x", ""] {
            assert!(check_entries(&[entry(name, 0o100_644)]).is_err(), "{name}");
        }
        let fine = [
            entry("Atrium.app/", 0o40_755),
            entry("Atrium.app/a..b", 0o100_644),
        ];
        check_entries(&fine).unwrap();
    }

    #[test]
    fn refuses_entries_below_a_symlink() {
        let entries = [
            entry("Atrium.app/escape", 0o120_755),
            entry("Atrium.app/escape/file", 0o100_644),
        ];
        assert!(check_entries(&entries).is_err());
        let sibling = [
            entry("Atrium.app/link", 0o120_755),
            entry("Atrium.app/linked-file", 0o100_644),
        ];
        check_entries(&sibling).unwrap();
    }

    #[test]
    fn symlinks_must_stay_inside() {
        let inside = [
            ("Atrium.app/Contents/link", "MacOS/atrium"),
            ("Atrium.app/Contents/link", "../Contents/Info.plist"),
            ("Atrium.app/link", "./Contents"),
        ];
        for (link, target) in inside {
            assert!(
                link_stays_inside(Path::new(link), Path::new(target)),
                "{target}"
            );
        }
        let outside = [
            ("Atrium.app/link", "/etc"),
            ("Atrium.app/link", "../../outside"),
            ("link", ".."),
            ("Atrium.app/a/link", "../../../x"),
        ];
        for (link, target) in outside {
            assert!(
                !link_stays_inside(Path::new(link), Path::new(target)),
                "{target}"
            );
        }
    }

    #[test]
    fn extraction_refuses_links_out_of_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Atrium.app");
        std::fs::create_dir_all(&app).unwrap();
        std::os::unix::fs::symlink("/etc", app.join("escape")).unwrap();
        let zip = dir.path().join("atrium.zip");
        ditto_zip(&app, &zip);
        let error = extract(&zip, &dir.path().join("extracted")).unwrap_err();
        assert!(error.contains("symbolic link"), "{error}");
    }
}
