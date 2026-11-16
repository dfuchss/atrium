//! The appearance: Tauri's theme sets GTK's `gtk-application-prefer-dark-theme` for the window,
//! the dialogs and the pages' `prefers-color-scheme`. For `Auto` no theme is forced, and tao
//! follows the desktop's colour scheme from the XDG desktop portal while Atrium runs. Two tao
//! 0.37.2 bugs remain: `Auto` is light at start until the desktop's scheme changes
//! (tauri-apps/tao#1378), and a change of the desktop's scheme overrides a forced Light or Dark
//! (tauri-apps/tao#1382).
//!
//! The GTK hook of the `AppImage` sets `GTK_THEME` to the desktop's theme and colour scheme at
//! launch (e.g. `Adwaita:light`), which GTK prefers to every setting, so Light, Dark and Auto
//! would have no effect. [`clean_launch_environment`] removes it before GTK starts.

use std::env;

const GTK_THEME: &str = "GTK_THEME";

/// Removes the `GTK_THEME` that the GTK hook of the `AppImage` set, so the theme setting
/// applies; the web processes, started later, inherit the environment without it. A `GTK_THEME`
/// outside an `AppImage` is the user's and stays. Call it on the first line of `main`, before
/// any thread starts.
#[expect(
    unsafe_code,
    reason = "std::env::remove_var; nothing else can change GTK's theme source"
)]
pub(crate) fn clean_launch_environment() {
    if env::var_os("APPIMAGE").is_some() {
        // SAFETY: called on the first line of `main`, before any thread starts, so no other
        // thread reads or writes the environment meanwhile.
        unsafe { env::remove_var(GTK_THEME) };
    }
}
