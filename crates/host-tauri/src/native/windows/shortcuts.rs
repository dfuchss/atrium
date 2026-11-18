//! Keyboard shortcuts: Windows has no app menu to carry them either, but unlike a GTK window
//! the main window never sees the keys of the webview that has the focus. So every webview of
//! the main window (the sidebar and each service view, see [`attach`]) offers its key presses
//! with Ctrl to the [`window_shortcuts`] and the numeric keypad's zoom keys, before the page
//! and `WebView2`'s own shortcuts get them; the ones that match go no further.
//!
//! Keys are matched by their place on a US keyboard, as Chromium's own shortcuts are: Ctrl+=
//! is the key right of 0 (the `+` key on a German keyboard), Ctrl+[ and Ctrl+] the two keys
//! right of P. Ctrl with Alt is no shortcut: Windows reports `AltGr` as Ctrl+Alt, which types
//! characters like `[` or `@` on many layouts.

use std::sync::{LazyLock, OnceLock};

use platform_ffi::{
    KeyPress, VIRTUAL_KEY, VK_ADD, VK_NUMPAD0, VK_OEM_4, VK_OEM_6, VK_OEM_COMMA, VK_OEM_MINUS,
    VK_OEM_PLUS, VK_SUBTRACT,
};
use tauri::{Manager, Runtime, Webview};

use crate::Logger;
use crate::menus::{ID_ZOOM_IN, ID_ZOOM_OUT, ID_ZOOM_RESET, Shortcut};
use crate::native::with_platform_webview;
use crate::window_shortcuts::{ShortcutActions, Target, window_shortcuts};

/// A virtual key and whether Shift is held with it (Ctrl always is).
type Accelerator = (VIRTUAL_KEY, bool);

/// Runs a shortcut's target; set by [`install_shortcuts`].
type Run = Box<dyn Fn(Target) + Send + Sync>;

static RUN: OnceLock<Run> = OnceLock::new();

/// All shortcuts: the [`window_shortcuts`] and the keypad's zoom keys.
static BINDINGS: LazyLock<Vec<(Accelerator, Target)>> = LazyLock::new(|| {
    window_shortcuts()
        .filter_map(|(shortcut, target)| Some((accelerator(shortcut)?, target)))
        .chain(keypad_bindings())
        .collect()
});

/// The zoom shortcuts of the numeric keypad, which has keys of its own, besides the main keys'.
fn keypad_bindings() -> [(Accelerator, Target); 3] {
    [
        (VK_ADD, ID_ZOOM_IN),
        (VK_SUBTRACT, ID_ZOOM_OUT),
        (VK_NUMPAD0, ID_ZOOM_RESET),
    ]
    .map(|(key, id)| ((key, false), Target::Item(id)))
}

/// The key of `shortcut` on a US keyboard, with Shift where the character needs it there
/// (`+` is Shift+=); None for a character without a key of its own.
fn accelerator(shortcut: Shortcut) -> Option<Accelerator> {
    let (key, shift) = match shortcut.key {
        ',' => (VK_OEM_COMMA, false),
        '-' => (VK_OEM_MINUS, false),
        '=' => (VK_OEM_PLUS, false),
        '+' => (VK_OEM_PLUS, true),
        '[' => (VK_OEM_4, false),
        ']' => (VK_OEM_6, false),
        // Letters and digits are their virtual keys, letters in upper case.
        key if key.is_ascii_alphanumeric() => {
            let code = u8::try_from(key.to_ascii_uppercase()).ok()?;
            (VIRTUAL_KEY(code.into()), false)
        }
        _ => return None,
    };
    Some((key, shift || shortcut.shift))
}

/// The shortcut `press` stands for, if any.
fn target_of(press: KeyPress) -> Option<Target> {
    if !press.ctrl || press.alt {
        return None;
    }
    BINDINGS
        .iter()
        .find(|((key, shift), _)| *key == press.key && *shift == press.shift)
        .map(|&(_, target)| target)
}

/// Offers the key presses of `webview`, a webview of the main window, to the shortcuts.
pub(crate) fn attach<R: Runtime>(webview: &Webview<R>) {
    let logger = Logger::new(webview.app_handle());
    with_platform_webview(webview, &logger, "no keyboard shortcuts", |platform| {
        platform_ffi::on_key_down(&platform.controller(), take_shortcut)
    });
}

/// Takes `press` if it is a shortcut and runs that off `WebView2`'s event, which must not touch
/// windows (closing one, bringing one to the front). Before [`install_shortcuts`] no key is
/// taken.
fn take_shortcut(press: KeyPress) -> bool {
    let Some((target, run)) = target_of(press).zip(RUN.get()) else {
        return false;
    };
    tauri::async_runtime::spawn_blocking(move || run(target));
    true
}

/// Makes the shortcuts run by `actions` work in their window's webviews. Only the first call
/// takes effect: there is one main window.
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as on Linux, where adding the shortcuts can fail"
)]
pub(crate) fn install_shortcuts<R: Runtime>(actions: ShortcutActions<R>) -> tauri::Result<()> {
    // A second main window does not exist: an earlier one keeps its shortcuts.
    let _ = RUN.set(Box::new(move |target| actions.run(target)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(key: VIRTUAL_KEY, shift: bool) -> KeyPress {
        KeyPress {
            key,
            ctrl: true,
            shift,
            alt: false,
        }
    }

    #[test]
    fn shortcuts_are_the_keys_of_a_us_keyboard() {
        for (shortcut, key) in [
            (Shortcut::key(','), (VK_OEM_COMMA, false)),
            (Shortcut::key('['), (VK_OEM_4, false)),
            (Shortcut::key('='), (VK_OEM_PLUS, false)),
            (Shortcut::key('+'), (VK_OEM_PLUS, true)),
            (Shortcut::key('1'), (VIRTUAL_KEY(u16::from(b'1')), false)),
            (
                Shortcut {
                    key: 'o',
                    shift: true,
                },
                (VIRTUAL_KEY(u16::from(b'O')), true),
            ),
        ] {
            assert_eq!(accelerator(shortcut), Some(key), "{shortcut:?}");
        }
    }

    #[test]
    fn every_window_shortcut_and_the_keypad_have_a_key() {
        assert_eq!(BINDINGS.len(), window_shortcuts().count() + 3);
    }

    #[test]
    fn ctrl_with_a_shortcut_key_is_its_target() {
        let digit_nine = VIRTUAL_KEY(u16::from(b'9'));
        assert_eq!(target_of(ctrl(digit_nine, false)), Some(Target::Account(8)));
        assert_eq!(
            target_of(ctrl(VIRTUAL_KEY(u16::from(b'W')), false)),
            Some(Target::Close)
        );
        for (key, shift) in [(VK_OEM_PLUS, false), (VK_OEM_PLUS, true), (VK_ADD, false)] {
            assert_eq!(target_of(ctrl(key, shift)), Some(Target::Item(ID_ZOOM_IN)));
        }
        assert_eq!(
            target_of(ctrl(VK_NUMPAD0, false)),
            Some(Target::Item(ID_ZOOM_RESET))
        );
    }

    #[test]
    fn other_keys_and_modifiers_are_left_to_the_page() {
        let key_r = VIRTUAL_KEY(u16::from(b'R'));
        assert_eq!(target_of(ctrl(key_r, true)), None);
        assert_eq!(target_of(ctrl(VIRTUAL_KEY(u16::from(b'K')), false)), None);
        let without_ctrl = KeyPress {
            ctrl: false,
            ..ctrl(key_r, false)
        };
        assert_eq!(target_of(without_ctrl), None);
        let alt_gr = KeyPress {
            alt: true,
            ..ctrl(VIRTUAL_KEY(u16::from(b'8')), false)
        };
        assert_eq!(target_of(alt_gr), None);
    }
}
