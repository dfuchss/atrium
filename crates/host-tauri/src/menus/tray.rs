//! The tray (menu bar) icon: its unread title and its menu of the accounts.

use domain::{MenuAccount, MenuModel};
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use super::{ID_DND, ID_QUIT, ID_SETTINGS, ID_SHOW, account_item_id};
use crate::native;

const TRAY_ID: &str = "atrium-tray";

/// "Label (n)", or just the label without unread.
fn tray_account_text(account: &MenuAccount) -> String {
    if account.unread > 0 {
        format!("{} ({})", account.label, account.unread)
    } else {
        account.label.clone()
    }
}

/// What the tray menu shows: account texts and the `DnD` state.
fn tray_menu_key(model: &MenuModel) -> (Vec<(Uuid, String)>, bool) {
    (
        model
            .accounts
            .iter()
            .map(|account| (account.id, tray_account_text(account)))
            .collect(),
        model.dnd,
    )
}

/// Tray title (unread total); None when empty so no space is reserved.
fn tray_title(model: &MenuModel) -> Option<String> {
    Some(model.tray_title.clone()).filter(|title| !title.is_empty())
}

fn build_tray_menu<R: Runtime>(app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<Menu<R>> {
    let mut menu = MenuBuilder::new(app)
        .text(ID_SHOW, "Show Atrium")
        .separator();
    for account in &model.accounts {
        menu = menu.text(account_item_id(account), tray_account_text(account));
    }
    if !model.accounts.is_empty() {
        menu = menu.separator();
    }
    let dnd = CheckMenuItemBuilder::with_id(ID_DND, "Do Not Disturb")
        .checked(model.dnd)
        .build(app)?;
    menu.item(&dnd)
        .text(ID_SETTINGS, "Settings…")
        .separator()
        .text(ID_QUIT, "Quit")
        .build()
}

fn build_tray<R: Runtime>(app: &AppHandle<R>, menu: &Menu<R>) -> tauri::Result<TrayIcon<R>> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(native::TRAY_ICON)?)
        .icon_as_template(native::TRAY_ICON_IS_TEMPLATE)
        .tooltip("Atrium")
        .menu(menu)
        .show_menu_on_left_click(true)
        .build(app)
}

/// Gives `tray` the items of `fresh`: as its new menu or, where a tray keeps its first menu
/// (see `native::TRAY_MENU_REPLACEABLE`), as the new items of that menu, `current`.
fn replace_tray_menu<R: Runtime>(
    tray: &TrayIcon<R>,
    current: Option<&Menu<R>>,
    fresh: Menu<R>,
) -> tauri::Result<()> {
    match current {
        Some(current) if !native::TRAY_MENU_REPLACEABLE => {
            for item in current.items()? {
                current.remove(&item)?;
            }
            for item in fresh.items()? {
                current.append(&item)?;
            }
            Ok(())
        }
        _ => tray.set_menu(Some(fresh)),
    }
}

/// The tray icon once created, with the menu it was created with.
pub(super) struct Tray<R: Runtime> {
    icon: Option<TrayIcon<R>>,
    menu: Option<Menu<R>>,
}

impl<R: Runtime> Default for Tray<R> {
    fn default() -> Self {
        Tray {
            icon: None,
            menu: None,
        }
    }
}

impl<R: Runtime> Tray<R> {
    /// Creates, shows, updates or hides the tray icon for `model`; `last` is the model applied
    /// before.
    pub(super) fn apply(
        &mut self,
        app: &AppHandle<R>,
        last: Option<&MenuModel>,
        model: &MenuModel,
    ) -> tauri::Result<()> {
        match (&self.icon, model.show_tray) {
            (None, true) => {
                let menu = build_tray_menu(app, model)?;
                let icon = build_tray(app, &menu)?;
                icon.set_title(tray_title(model))?;
                self.icon = Some(icon);
                self.menu = Some(menu);
            }
            (Some(icon), true) => {
                let shown_before = last.filter(|last| last.show_tray);
                if shown_before.is_none() {
                    icon.set_visible(true)?;
                }
                if shown_before.is_none_or(|last| tray_menu_key(last) != tray_menu_key(model)) {
                    let fresh = build_tray_menu(app, model)?;
                    replace_tray_menu(icon, self.menu.as_ref(), fresh)?;
                }
                if shown_before.is_none_or(|last| last.tray_title != model.tray_title) {
                    icon.set_title(tray_title(model))?;
                }
            }
            (Some(icon), false) => icon.set_visible(false)?,
            (None, false) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::model;
    use super::*;

    #[test]
    fn keys_and_texts() {
        assert_eq!(tray_account_text(&model(3, false).accounts[0]), "Work (3)");
        assert_eq!(tray_account_text(&model(0, false).accounts[0]), "Work");
        assert_ne!(
            tray_menu_key(&model(0, false)),
            tray_menu_key(&model(3, false))
        );
        assert_ne!(
            tray_menu_key(&model(0, false)),
            tray_menu_key(&model(0, true))
        );
        assert_eq!(tray_title(&model(0, false)), None);
        assert_eq!(tray_title(&model(2, false)).as_deref(), Some("2"));
    }

    #[test]
    fn tray_icon_is_a_png() {
        assert!(Image::from_bytes(native::TRAY_ICON).is_ok());
    }
}
