//! Windows shell shortcuts (`.lnk` files) that start a program under an application user
//! model id (`System.AppUserModel.ID`), so Windows names notifications and taskbar buttons
//! after the app; Tauri has no API for them. The shell's `ShellLink` COM object writes and
//! reads them.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SHStrDupW, ShellLink};
use windows::core::{HSTRING, Interface as _};

/// What a shortcut starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The program it starts.
    pub target: PathBuf,
    /// Its application user model id; `None` if it has none.
    pub app_id: Option<String>,
}

/// COM on the calling thread for as long as it lives.
struct ComScope {
    /// Whether this scope initialised COM and has to uninitialise it again.
    initialised: bool,
}

impl ComScope {
    fn enter() -> windows::core::Result<ComScope> {
        // SAFETY: no reserved pointer is passed; every successful call is balanced by
        // `CoUninitialize` in `drop` on the same thread (the scope is neither `Send` nor kept).
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            // The thread already joined the multi-threaded apartment, where COM works as well.
            return Ok(ComScope { initialised: false });
        }
        result.ok()?;
        Ok(ComScope { initialised: true })
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialised {
            // SAFETY: balances the successful `CoInitializeEx` of `enter` on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// A new, empty `ShellLink`.
fn new_shell_link() -> windows::core::Result<IShellLinkW> {
    // SAFETY: `ShellLink` is the class id of the shell's link object, which implements
    // `IShellLinkW`; COM is initialised by the caller's `ComScope`.
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
}

/// Writes the shortcut `link` (replacing one that is there) that starts `target` under the
/// application user model id `app_id`.
pub fn write_shortcut(link: &Path, target: &Path, app_id: &str) -> windows::core::Result<()> {
    let _com = ComScope::enter()?;
    let shell_link = new_shell_link()?;
    let mut app_id = string_value(app_id)?;
    // SAFETY: the strings are valid null-terminated UTF-16 for the duration of each call, the
    // property key is a static and `app_id` a valid `PROPVARIANT`, which `SetValue` copies.
    let saved = unsafe {
        shell_link.SetPath(&HSTRING::from(target.as_os_str()))?;
        let store: IPropertyStore = shell_link.cast()?;
        store.SetValue(&PKEY_AppUserModel_ID, &raw const app_id)?;
        store.Commit()?;
        shell_link
            .cast::<IPersistFile>()?
            .Save(&HSTRING::from(link.as_os_str()), true)
    };
    // SAFETY: `app_id` comes from `string_value` and is cleared once.
    unsafe { PropVariantClear(&raw mut app_id)? };
    saved
}

/// A `VT_LPWSTR` value holding a COM allocated copy of `text`, as `InitPropVariantFromString`
/// makes it (the shell expects the string on the COM heap); clear it with `PropVariantClear`.
fn string_value(text: &str) -> windows::core::Result<PROPVARIANT> {
    // SAFETY: the string is valid null-terminated UTF-16 for the duration of the call.
    let copy = unsafe { SHStrDupW(&HSTRING::from(text))? };
    let mut value = PROPVARIANT::default();
    // SAFETY: writing the plain fields of a zeroed `PROPVARIANT` makes it a valid `VT_LPWSTR`
    // that owns `copy`.
    unsafe {
        let fields = &mut value.Anonymous.Anonymous;
        fields.vt = VT_LPWSTR;
        fields.Anonymous.pwszVal = copy;
    }
    Ok(value)
}

/// Reads the shortcut `link`.
pub fn read_shortcut(link: &Path) -> windows::core::Result<Shortcut> {
    let _com = ComScope::enter()?;
    let shell_link = new_shell_link()?;
    let mut target = [0u16; 1024];
    // SAFETY: the path is valid null-terminated UTF-16 during the call, `target` is a
    // writable buffer whose length `GetPath` receives, and no find data is asked for.
    unsafe {
        shell_link
            .cast::<IPersistFile>()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)?;
        shell_link.GetPath(&mut target, std::ptr::null_mut(), 0)?;
    }
    let length = target.iter().position(|&unit| unit == 0).unwrap_or(0);
    Ok(Shortcut {
        target: PathBuf::from(String::from_utf16_lossy(&target[..length])),
        app_id: app_id_of(&shell_link.cast()?)?,
    })
}

/// The application user model id in the property store of a loaded shortcut.
fn app_id_of(store: &IPropertyStore) -> windows::core::Result<Option<String>> {
    // SAFETY: the property key is a static; the returned `PROPVARIANT` is owned here.
    let mut value = unsafe { store.GetValue(&PKEY_AppUserModel_ID)? };
    // SAFETY: `vt` says which union field is valid; a `VT_LPWSTR` holds a null-terminated
    // string (or null), read before `PropVariantClear` frees it.
    let app_id = unsafe {
        let fields = &value.Anonymous.Anonymous;
        let text = fields.Anonymous.pwszVal;
        (fields.vt == VT_LPWSTR && !text.is_null())
            .then(|| String::from_utf16_lossy(text.as_wide()))
    };
    // SAFETY: `value` is a valid `PROPVARIANT` from `GetValue`, cleared once.
    unsafe { PropVariantClear(&raw mut value)? };
    Ok(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_shortcut_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("Atrium.lnk");
        let target = dir.path().join("Atrium Folder").join("atrium.exe");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "exe").unwrap();

        write_shortcut(&link, &target, "org.fuchss.atrium.dev").unwrap();
        let shortcut = read_shortcut(&link).unwrap();
        // Windows keeps the long form of the target, while the temporary folder may come as a
        // short (8.3) path, e.g. `RUNNER~1`.
        assert_eq!(
            std::fs::canonicalize(&shortcut.target).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
        assert_eq!(shortcut.app_id.as_deref(), Some("org.fuchss.atrium.dev"));
        write_shortcut(&link, &target, "org.example.other").unwrap();
        assert_eq!(
            read_shortcut(&link).unwrap().app_id.as_deref(),
            Some("org.example.other")
        );
        assert!(read_shortcut(&dir.path().join("missing.lnk")).is_err());
    }
}
