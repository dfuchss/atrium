//! The keyboard shortcuts of the main window where there is no app menu to carry them (Linux
//! and Windows): the macOS menu's ([`ITEM_SHORTCUTS`], [`account_shortcut`]) with Ctrl for Cmd,
//! routed to the same menu item ids, and Close. The platform maps them to its keys and runs
//! them with [`ShortcutActions`].

use std::sync::{Arc, Mutex, PoisonError};

use domain::MenuModel;
use tauri::{AppHandle, Manager, Runtime};

use crate::menus::{
    ACCOUNT_SHORTCUTS, ID_ZOOM_IN, ITEM_SHORTCUTS, Shortcut, account_item_id, account_shortcut,
    handle_menu_event,
};
use crate::{Feed, native};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Target {
    /// The menu item with this id.
    Item(&'static str),
    /// The account at this position (from 0) in the menu.
    Account(usize),
    /// Close the window, like the Close item of the macOS Window menu.
    Close,
}

/// Closes the window, which the predefined Close item of the macOS Window menu does with Cmd+W.
const CLOSE_SHORTCUT: Shortcut = Shortcut::key('w');

/// Zooms in besides Ctrl+=, as in the browsers: `=` shares its key with `+` only on US-like
/// layouts (German: Shift+0). With Shift, a US Ctrl+Shift+= is Ctrl++ too.
const ZOOM_IN_PLUS_SHORTCUT: Shortcut = Shortcut::key('+');

/// All shortcuts: the menu items', the accounts', Close and Ctrl++.
pub(crate) fn window_shortcuts() -> impl Iterator<Item = (Shortcut, Target)> {
    let items = ITEM_SHORTCUTS
        .iter()
        .map(|&(id, shortcut)| (shortcut, Target::Item(id)));
    let accounts = (0..ACCOUNT_SHORTCUTS)
        .filter_map(|index| Some((account_shortcut(index)?, Target::Account(index))));
    items.chain(accounts).chain([
        (CLOSE_SHORTCUT, Target::Close),
        (ZOOM_IN_PLUS_SHORTCUT, Target::Item(ID_ZOOM_IN)),
    ])
}

/// The accounts that have a shortcut, as menu item ids in menu order; shared by the platform's
/// shortcuts and the [`ShortcutActions`] that select them.
#[derive(Clone, Default)]
pub(crate) struct ShortcutAccounts(Arc<Mutex<Vec<String>>>);

impl ShortcutAccounts {
    /// Takes the accounts for the account shortcuts from `model`.
    pub(crate) fn set(&self, model: &MenuModel) {
        let ids = model
            .accounts
            .iter()
            .take(ACCOUNT_SHORTCUTS)
            .map(account_item_id)
            .collect();
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = ids;
    }

    fn item_id(&self, index: usize) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(index)
            .cloned()
    }
}

/// What the shortcuts act on: the window `label`, whose menu inputs go to `feed`.
pub(crate) struct ShortcutActions<R: Runtime> {
    pub(crate) app: AppHandle<R>,
    pub(crate) label: String,
    pub(crate) feed: Feed,
    pub(crate) accounts: ShortcutAccounts,
}

impl<R: Runtime> ShortcutActions<R> {
    /// Does what `target` says.
    pub(crate) fn run(&self, target: Target) {
        match target {
            Target::Item(id) => self.item(id),
            Target::Account(index) => {
                if let Some(id) = self.accounts.item_id(index) {
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

/// The keyboard shortcuts of the main window; the platform maps them to its keys
/// (`native::install_shortcuts`).
pub(crate) struct Shortcuts {
    accounts: ShortcutAccounts,
}

impl Shortcuts {
    pub(crate) fn new() -> Shortcuts {
        Shortcuts {
            accounts: ShortcutAccounts::default(),
        }
    }

    /// Makes the shortcuts of the window `window_label` work; they feed their inputs to `feed`.
    pub(crate) fn install<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        window_label: &str,
        feed: &Feed,
    ) -> tauri::Result<()> {
        native::install_shortcuts(ShortcutActions {
            app: app.clone(),
            label: window_label.to_string(),
            feed: feed.clone(),
            accounts: self.accounts.clone(),
        })
    }

    /// Takes the accounts for the account shortcuts from `model`.
    pub(crate) fn set_accounts(&self, model: &MenuModel) {
        self.accounts.set(model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_items_accounts_close_and_ctrl_plus_have_shortcuts() {
        let shortcuts: Vec<_> = window_shortcuts().collect();
        assert_eq!(
            shortcuts.len(),
            ITEM_SHORTCUTS.len() + ACCOUNT_SHORTCUTS + 2
        );
        assert!(shortcuts.contains(&(Shortcut::key('9'), Target::Account(8))));
        assert!(shortcuts.contains(&(Shortcut::key('w'), Target::Close)));
        assert!(shortcuts.contains(&(Shortcut::key('+'), Target::Item(ID_ZOOM_IN))));
    }
}
