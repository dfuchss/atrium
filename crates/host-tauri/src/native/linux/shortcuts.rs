//! Keyboard shortcuts: Linux has no app menu to carry them, so they are accelerators of the
//! main `gtk::ApplicationWindow`. GTK runs a window's accelerators before it hands a key press
//! to the focused widget, so they work while a service's webview has the focus, like the macOS
//! menu does. The shortcuts are the macOS menu's ([`ITEM_SHORTCUTS`], [`account_shortcut`])
//! with Ctrl for Cmd, and route to the same menu item ids.

use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use domain::MenuModel;
use gtk::gdk::ModifierType;
use gtk::gdk::keys::{Key, constants};
use gtk::prelude::*;
use tauri::{AppHandle, Manager, Runtime};

use crate::menus::{
    ACCOUNT_SHORTCUTS, ID_ZOOM_IN, ID_ZOOM_OUT, ID_ZOOM_RESET, ITEM_SHORTCUTS, Shortcut,
    account_item_id, account_shortcut, handle_menu_event,
};
use crate::{Feed, Logger};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    /// The menu item with this id.
    Item(&'static str),
    /// The account at this position (from 0) in the menu.
    Account(usize),
    /// Close the window, like the Close item of the macOS Window menu.
    Close,
}

/// Closes the window, which the predefined Close item of the macOS Window menu does with Cmd+W.
const CLOSE_SHORTCUT: Shortcut = Shortcut::key('w');

/// A GTK key and its modifiers.
type Accelerator = (u32, ModifierType);

/// Zooms in besides Ctrl+=, as in the Linux browsers: `=` shares its key with `+` only on US-like
/// layouts (German: Shift+0). With Shift, a US Ctrl+Shift+= is Ctrl++ too.
const ZOOM_IN_PLUS_SHORTCUT: Shortcut = Shortcut::key('+');

/// The zoom shortcuts of the numeric keypad, which has keys of its own, besides the main keys'.
fn keypad_bindings() -> [(Accelerator, Target); 3] {
    [
        (*constants::KP_Add, ID_ZOOM_IN),
        (*constants::KP_Subtract, ID_ZOOM_OUT),
        (*constants::KP_0, ID_ZOOM_RESET),
    ]
    .map(|(key, id)| ((key, ModifierType::CONTROL_MASK), Target::Item(id)))
}

/// All shortcuts: the menu items', the accounts', Close, Ctrl++ and the keypad's zoom keys.
fn bindings() -> Vec<(Accelerator, Target)> {
    let items = ITEM_SHORTCUTS
        .iter()
        .map(|&(id, shortcut)| (shortcut, Target::Item(id)));
    let accounts = (0..ACCOUNT_SHORTCUTS)
        .filter_map(|index| Some((account_shortcut(index)?, Target::Account(index))));
    items
        .chain(accounts)
        .chain([
            (CLOSE_SHORTCUT, Target::Close),
            (ZOOM_IN_PLUS_SHORTCUT, Target::Item(ID_ZOOM_IN)),
        ])
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
    /// Menu item ids of the accounts that have a shortcut, in menu order.
    accounts: Arc<Mutex<Vec<String>>>,
}

impl Shortcuts {
    pub(crate) fn new() -> Shortcuts {
        Shortcuts {
            accounts: Arc::default(),
        }
    }

    /// Adds the shortcuts to the window `window_label`; they feed their inputs to `feed`.
    pub(crate) fn install<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        window_label: &str,
        feed: &Feed,
    ) -> tauri::Result<()> {
        let (handle, label) = (app.clone(), window_label.to_string());
        let (feed, accounts) = (Arc::clone(feed), Arc::clone(&self.accounts));
        app.run_on_main_thread(move || {
            let context = Rc::new(Context {
                app: handle,
                label,
                feed,
                accounts,
            });
            if let Err(error) = context.add_accelerators() {
                Logger::new(&context.app).log(&format!("no keyboard shortcuts: {error}"));
            }
        })
    }

    /// Takes the accounts for the account shortcuts from `model`.
    pub(crate) fn set_accounts(&self, model: &MenuModel) {
        let ids = model
            .accounts
            .iter()
            .take(ACCOUNT_SHORTCUTS)
            .map(account_item_id)
            .collect();
        *self.accounts.lock().unwrap_or_else(PoisonError::into_inner) = ids;
    }
}

/// What a shortcut needs to act; cloned into each accelerator.
struct Context<R: Runtime> {
    app: AppHandle<R>,
    label: String,
    feed: Feed,
    accounts: Arc<Mutex<Vec<String>>>,
}

impl<R: Runtime> Context<R> {
    /// Adds an accelerator group with all [`bindings`] to the window (main thread).
    fn add_accelerators(self: &Rc<Self>) -> tauri::Result<()> {
        let window = self
            .app
            .get_window(&self.label)
            .ok_or(tauri::Error::WindowNotFound)?;
        let group = gtk::AccelGroup::new();
        for ((key, modifiers), target) in bindings() {
            let context = Rc::clone(self);
            group.connect_accel_group(
                key,
                modifiers,
                gtk::AccelFlags::empty(),
                move |_, _, _, _| {
                    context.run(target);
                    true
                },
            );
        }
        window.gtk_window()?.add_accel_group(&group);
        Ok(())
    }

    fn run(&self, target: Target) {
        match target {
            Target::Item(id) => self.item(id),
            Target::Account(index) => {
                let id = self
                    .accounts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(index)
                    .cloned();
                if let Some(id) = id {
                    self.item(&id);
                }
            }
            Target::Close => {
                if let Some(window) = self.app.get_window(&self.label) {
                    // The window is already going away or gone: nothing to do.
                    let _ = window.close();
                }
            }
        }
    }

    fn item(&self, id: &str) {
        handle_menu_event(&self.app, &self.label, id, &self.feed);
    }
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
    fn the_menu_items_accounts_close_ctrl_plus_and_the_keypad_have_shortcuts() {
        let bindings = bindings();
        assert_eq!(bindings.len(), ITEM_SHORTCUTS.len() + ACCOUNT_SHORTCUTS + 5);
        let control = ModifierType::CONTROL_MASK;
        let accelerator = |shortcut| gtk_accelerator(shortcut);
        assert!(bindings.contains(&(accelerator(Shortcut::key('9')), Target::Account(8))));
        assert!(bindings.contains(&(accelerator(CLOSE_SHORTCUT), Target::Close)));
        assert!(bindings.contains(&(accelerator(Shortcut::key('+')), Target::Item(ID_ZOOM_IN))));
        for (key, id) in [
            (*constants::KP_Add, ID_ZOOM_IN),
            (*constants::KP_Subtract, ID_ZOOM_OUT),
            (*constants::KP_0, ID_ZOOM_RESET),
        ] {
            assert!(bindings.contains(&((key, control), Target::Item(id))));
        }
    }
}
