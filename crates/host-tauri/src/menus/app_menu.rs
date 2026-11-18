//! The app menu (macOS menu bar): Atrium, Edit, View, Accounts and Window, with the shortcuts.

use domain::MenuModel;
use tauri::image::Image;
use tauri::menu::{
    AboutMetadata, AboutMetadataBuilder, Menu, MenuBuilder, MenuItem, MenuItemBuilder, Submenu,
    SubmenuBuilder,
};
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use super::{
    ID_BACK, ID_FORWARD, ID_OPEN_BROWSER, ID_QUIT, ID_RELOAD, ID_SETTINGS, ID_ZOOM_IN, ID_ZOOM_OUT,
    ID_ZOOM_RESET, ITEM_SHORTCUTS, Shortcut, account_item_id, account_shortcut,
};

const ABOUT_ICON: &[u8] = include_bytes!("../../assets/about-icon.png");

/// What the app menu shows: ids and labels of the enabled accounts.
pub(super) fn app_menu_key(model: &MenuModel) -> Vec<(Uuid, String)> {
    model
        .accounts
        .iter()
        .map(|account| (account.id, account.label.clone()))
        .collect()
}

/// The menu item `id` with its shortcut from [`ITEM_SHORTCUTS`], if it has one.
fn shortcut_item<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    text: &str,
) -> tauri::Result<MenuItem<R>> {
    let mut item = MenuItemBuilder::with_id(id, text);
    if let Some(shortcut) = item_shortcut(id) {
        item = item.accelerator(shortcut.accelerator());
    }
    item.build(app)
}

fn item_shortcut(id: &str) -> Option<Shortcut> {
    ITEM_SHORTCUTS
        .iter()
        .find(|(item, _)| *item == id)
        .map(|&(_, shortcut)| shortcut)
}

fn about_metadata<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<AboutMetadata<'static>> {
    let version = app.package_info().version.to_string();
    Ok(AboutMetadataBuilder::new()
        .name(Some("Atrium"))
        .version(Some(version.clone()))
        .short_version(Some(version))
        .authors(Some(vec!["Dominik Fuchß".to_string()]))
        .comments(Some("All your messengers in one calm place."))
        .copyright(Some("© 2026 Dominik Fuchß"))
        .license(Some("GPL-3.0-or-later"))
        .website(Some(protocol::PROJECT_URL))
        .website_label(Some("github.com/dfuchss/atrium"))
        .icon(Some(Image::from_bytes(ABOUT_ICON)?))
        .build())
}

fn atrium_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "Atrium")
        .about(Some(about_metadata(app)?))
        .separator()
        .item(&shortcut_item(app, ID_SETTINGS, "Settings…")?)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&shortcut_item(app, ID_QUIT, "Quit Atrium")?)
        .build()
}

/// Without an Edit menu, Cmd+C/V/X/A/Z do not reach the webviews on macOS.
fn edit_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()
}

fn view_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "View")
        .item(&shortcut_item(app, ID_RELOAD, "Reload")?)
        .item(&shortcut_item(app, ID_BACK, "Back")?)
        .item(&shortcut_item(app, ID_FORWARD, "Forward")?)
        .separator()
        .item(&shortcut_item(app, ID_ZOOM_IN, "Zoom In")?)
        .item(&shortcut_item(app, ID_ZOOM_OUT, "Zoom Out")?)
        .item(&shortcut_item(app, ID_ZOOM_RESET, "Actual Size")?)
        .separator()
        .item(&shortcut_item(
            app,
            ID_OPEN_BROWSER,
            "Open Page in Browser",
        )?)
        .separator()
        .fullscreen()
        .build()
}

fn accounts_submenu<R: Runtime>(
    app: &AppHandle<R>,
    model: &MenuModel,
) -> tauri::Result<Submenu<R>> {
    let mut submenu = SubmenuBuilder::new(app, "Accounts");
    for (index, account) in model.accounts.iter().enumerate() {
        let mut item = MenuItemBuilder::with_id(account_item_id(account), &account.label);
        if let Some(shortcut) = account_shortcut(index) {
            item = item.accelerator(shortcut.accelerator());
        }
        submenu = submenu.item(&item.build(app)?);
    }
    if model.accounts.is_empty() {
        submenu = submenu.item(
            &MenuItemBuilder::new("No accounts")
                .enabled(false)
                .build(app)?,
        );
    }
    submenu.build()
}

/// Close = performClose: on the key window -> `CloseRequested`, so the main window follows
/// close-to-background and popups simply close.
fn window_submenu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "Window")
        .minimize()
        .close_window()
        .build()
}

pub(super) fn build_app_menu<R: Runtime>(
    app: &AppHandle<R>,
    model: &MenuModel,
) -> tauri::Result<Menu<R>> {
    MenuBuilder::new(app)
        .items(&[
            &atrium_submenu(app)?,
            &edit_submenu(app)?,
            &view_submenu(app)?,
            &accounts_submenu(app, model)?,
            &window_submenu(app)?,
        ])
        .build()
}

#[cfg(test)]
mod tests {
    use super::super::ID_DND;
    use super::super::tests::model;
    use super::*;

    #[test]
    fn only_shortcut_items_get_an_accelerator() {
        assert_eq!(item_shortcut(ID_ZOOM_IN), Some(Shortcut::key('=')));
        assert_eq!(item_shortcut(ID_DND), None);
    }

    #[test]
    fn unread_and_dnd_do_not_change_the_app_menu() {
        assert_eq!(
            app_menu_key(&model(0, false)),
            app_menu_key(&model(3, true))
        );
    }
}
