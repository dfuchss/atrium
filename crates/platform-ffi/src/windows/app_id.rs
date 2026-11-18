//! The app's identity on Windows: its `AppUserModelID`.

use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::HSTRING;

/// Gives the process the `AppUserModelID` `id`; call it before the first window exists. The
/// taskbar groups the windows by it and, once a Start menu shortcut carries the same id, shows
/// them with that shortcut's name and icon.
pub fn set_app_user_model_id(id: &str) -> windows::core::Result<()> {
    // SAFETY: the id is valid, null-terminated UTF-16 for the duration of the call.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(id)) }
}
