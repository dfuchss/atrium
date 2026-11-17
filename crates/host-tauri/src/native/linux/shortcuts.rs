//! Keyboard shortcuts: Linux has no app menu to carry them, so the [`window_shortcuts`] are
//! accelerators of the main `gtk::ApplicationWindow`, with the numeric keypad's zoom keys. GTK
//! runs a window's accelerators before it hands a key press to the focused widget, so they work
//! while a service's webview has the focus, like the macOS menu does.

use std::rc::Rc;

use domain::MenuModel;
use gtk::gdk::ModifierType;
use gtk::gdk::keys::{Key, constants};
use gtk::prelude::*;
use tauri::{AppHandle, Manager, Runtime};

use crate::menus::{ID_ZOOM_IN, ID_ZOOM_OUT, ID_ZOOM_RESET, Shortcut};
use crate::window_shortcuts::{ShortcutAccounts, ShortcutActions, Target, window_shortcuts};
use crate::{Feed, Logger};

/// A GTK key and its modifiers.
type Accelerator = (u32, ModifierType);

/// The zoom shortcuts of the numeric keypad, which has keys of its own, besides the main keys'.
fn keypad_bindings() -> [(Accelerator, Target); 3] {
    [
        (*constants::KP_Add, ID_ZOOM_IN),
        (*constants::KP_Subtract, ID_ZOOM_OUT),
        (*constants::KP_0, ID_ZOOM_RESET),
    ]
    .map(|(key, id)| ((key, ModifierType::CONTROL_MASK), Target::Item(id)))
}

/// All shortcuts: the [`window_shortcuts`] and the keypad's zoom keys.
fn bindings() -> Vec<(Accelerator, Target)> {
    window_shortcuts()
        .map(|(shortcut, target)| (gtk_accelerator(shortcut), target))
        .chain(keypad_bindings())
        .collect()
}

/// The GTK key and modifiers of `shortcut`, with Ctrl for Cmd.
fn gtk_accelerator(shortcut: Shortcut) -> Accelerator {
    let key = *Key::from_unicode(shortcut.key);
    let shift = if shortcut.shift {
        ModifierType::SHIFT_MASK
    } else {
        ModifierType::empty()
    };
    (key, ModifierType::CONTROL_MASK | shift)
}

/// The keyboard shortcuts of the main window.
pub(crate) struct Shortcuts {
    accounts: ShortcutAccounts,
}

impl Shortcuts {
    pub(crate) fn new() -> Shortcuts {
        Shortcuts {
            accounts: ShortcutAccounts::default(),
        }
    }

    /// Adds the shortcuts to the window `window_label`; they feed their inputs to `feed`.
    pub(crate) fn install<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        window_label: &str,
        feed: &Feed,
    ) -> tauri::Result<()> {
        let actions = ShortcutActions {
            app: app.clone(),
            label: window_label.to_string(),
            feed: feed.clone(),
            accounts: self.accounts.clone(),
        };
        app.run_on_main_thread(move || {
            let actions = Rc::new(actions);
            if let Err(error) = add_accelerators(&actions) {
                Logger::new(&actions.app).log(&format!("no keyboard shortcuts: {error}"));
            }
        })
    }

    /// Takes the accounts for the account shortcuts from `model`.
    pub(crate) fn set_accounts(&self, model: &MenuModel) {
        self.accounts.set(model);
    }
}

/// Adds an accelerator group with all [`bindings`] to the window of `actions` (main thread).
fn add_accelerators<R: Runtime>(actions: &Rc<ShortcutActions<R>>) -> tauri::Result<()> {
    let window = actions
        .app
        .get_window(&actions.label)
        .ok_or(tauri::Error::WindowNotFound)?;
    let group = gtk::AccelGroup::new();
    for ((key, modifiers), target) in bindings() {
        let actions = Rc::clone(actions);
        group.connect_accel_group(
            key,
            modifiers,
            gtk::AccelFlags::empty(),
            move |_, _, _, _| {
                actions.run(target);
                true
            },
        );
    }
    window.gtk_window()?.add_accel_group(&group);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_the_gtk_keys_with_ctrl() {
        let control = ModifierType::CONTROL_MASK;
        for (key, shortcut) in [
            (*constants::comma, Shortcut::key(',')),
            (*constants::bracketleft, Shortcut::key('[')),
            (*constants::equal, Shortcut::key('=')),
            (*constants::plus, Shortcut::key('+')),
            (*constants::_1, Shortcut::key('1')),
        ] {
            assert_eq!(gtk_accelerator(shortcut), (key, control), "{shortcut:?}");
        }
        assert_eq!(
            gtk_accelerator(Shortcut {
                key: 'o',
                shift: true
            }),
            (*constants::o, control | ModifierType::SHIFT_MASK)
        );
    }

    #[test]
    fn the_window_shortcuts_and_the_keypad_have_accelerators() {
        let bindings = bindings();
        assert_eq!(bindings.len(), window_shortcuts().count() + 3);
        let control = ModifierType::CONTROL_MASK;
        assert!(bindings.contains(&((*constants::_9, control), Target::Account(8))));
        for (key, id) in [
            (*constants::KP_Add, ID_ZOOM_IN),
            (*constants::KP_Subtract, ID_ZOOM_OUT),
            (*constants::KP_0, ID_ZOOM_RESET),
        ] {
            assert!(bindings.contains(&((key, control), Target::Item(id))));
        }
    }
}
