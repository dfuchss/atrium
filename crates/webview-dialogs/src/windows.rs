//! Windows: the Win32 calls Tauri has no API for. For now only the warning box.

use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TASKMODAL, MessageBoxW,
};
use windows::core::HSTRING;

/// Shows a warning with one OK button, modal for the windows of the calling thread, and returns
/// once it is dismissed. It pumps messages meanwhile, so call it from the running event loop.
pub fn show_warning(title: &str, message: &str) {
    let text = HSTRING::from(format!("{title}\n\n{message}"));
    // SAFETY: both strings are valid, null-terminated UTF-16 that outlives the call, no owner
    // window is passed, and the style is a valid combination of flags.
    unsafe {
        MessageBoxW(
            None,
            &text,
            &HSTRING::from("Atrium"),
            MB_OK | MB_ICONWARNING | MB_TASKMODAL | MB_SETFOREGROUND,
        );
    }
}
