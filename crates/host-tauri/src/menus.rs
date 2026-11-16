//! Native app menu (with the shortcuts) and the tray (menu bar) icon, both derived from the
//! core's [`MenuModel`]. Menu events of both arrive at one global handler, which turns them
//! into [`Input`]s with [`menu_command`].

use domain::{Input, MenuAccount, MenuAction, MenuModel};
use protocol::UiCommand;
use tauri::image::Image;
use tauri::menu::{
    AboutMetadata, AboutMetadataBuilder, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItem,
    MenuItemBuilder, Submenu, SubmenuBuilder,
};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use crate::{Feed, bring_to_front, graceful_quit, native};

const ID_SHOW: &str = "show";
const ID_SETTINGS: &str = "settings";
const ID_DND: &str = "dnd";
const ID_RELOAD: &str = "reload";
const ID_BACK: &str = "back";
const ID_FORWARD: &str = "forward";
pub(crate) const ID_ZOOM_IN: &str = "zoom-in";
pub(crate) const ID_ZOOM_OUT: &str = "zoom-out";
pub(crate) const ID_ZOOM_RESET: &str = "zoom-reset";
const ID_OPEN_BROWSER: &str = "open-in-browser";
/// Custom Quit items (app menu, tray): the predefined Quit sends `terminate:`, which ends the
/// process without a `RunEvent::ExitRequested`, so pages would not be unloaded first.
/// Handled by [`graceful_quit`], not by [`menu_command`].
const ID_QUIT: &str = "quit";
/// Prefix of the account items (app menu "Accounts" and tray), followed by the account id.
const ID_ACCOUNT: &str = "account:";
const TRAY_ID: &str = "atrium-tray";
/// Accounts 1 to 9 get the shortcuts Cmd+1 to Cmd+9 (Ctrl on Linux).
pub(crate) const ACCOUNT_SHORTCUTS: usize = 9;

/// A keyboard shortcut: `key` held with Cmd on macOS and Ctrl on Linux, and with Shift if
/// `shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shortcut {
    /// The character of the key; letters in lower case.
    pub(crate) key: char,
    pub(crate) shift: bool,
}

impl Shortcut {
    pub(crate) const fn key(key: char) -> Shortcut {
        Shortcut { key, shift: false }
    }

    const fn shift_key(key: char) -> Shortcut {
        Shortcut { key, shift: true }
    }

    /// The accelerator of a menu item, e.g. `CmdOrCtrl+Shift+O`.
    fn accelerator(self) -> String {
        let shift = if self.shift { "Shift+" } else { "" };
        format!("CmdOrCtrl+{shift}{}", self.key.to_ascii_uppercase())
    }
}

/// The menu items with a shortcut, by id: the macOS menu and the Linux window both take their
/// shortcuts from here.
pub(crate) const ITEM_SHORTCUTS: [(&str, Shortcut); 9] = [
    (ID_SETTINGS, Shortcut::key(',')),
    (ID_QUIT, Shortcut::key('q')),
    (ID_RELOAD, Shortcut::key('r')),
    (ID_BACK, Shortcut::key('[')),
    (ID_FORWARD, Shortcut::key(']')),
    (ID_ZOOM_IN, Shortcut::key('=')),
    (ID_ZOOM_OUT, Shortcut::key('-')),
    (ID_ZOOM_RESET, Shortcut::key('0')),
    (ID_OPEN_BROWSER, Shortcut::shift_key('o')),
];

/// The shortcut of the account at `index` (from 0) in the menu, for the first
/// [`ACCOUNT_SHORTCUTS`] accounts: Cmd+1 to Cmd+9 (Ctrl on Linux).
pub(crate) fn account_shortcut(index: usize) -> Option<Shortcut> {
    if index >= ACCOUNT_SHORTCUTS {
        return None;
    }
    char::from_digit(u32::try_from(index + 1).ok()?, 10).map(Shortcut::key)
}

const ABOUT_ICON: &[u8] = include_bytes!("../assets/about-icon.png");

/// What a menu item does: optionally bring the main window to the front, then feed an input.
#[derive(Debug, Clone, PartialEq)]
struct MenuCommand {
    show_window: bool,
    input: Option<Input>,
}

/// Maps a menu item id to its command; None for ids we do not own (predefined items).
fn menu_command(id: &str) -> Option<MenuCommand> {
    let menu = |action| MenuCommand {
        show_window: false,
        input: Some(Input::Menu(action)),
    };
    Some(match id {
        ID_SHOW => MenuCommand {
            show_window: true,
            input: None,
        },
        ID_SETTINGS => MenuCommand {
            show_window: true,
            input: Some(Input::Ui(UiCommand::SetDialog { open: true })),
        },
        ID_DND => menu(MenuAction::ToggleDnd),
        ID_RELOAD => menu(MenuAction::Reload),
        ID_BACK => menu(MenuAction::Back),
        ID_FORWARD => menu(MenuAction::Forward),
        ID_ZOOM_IN => menu(MenuAction::ZoomIn),
        ID_ZOOM_OUT => menu(MenuAction::ZoomOut),
        ID_ZOOM_RESET => menu(MenuAction::ZoomReset),
        ID_OPEN_BROWSER => menu(MenuAction::OpenInBrowser),
        _ => {
            let account = Uuid::parse_str(id.strip_prefix(ID_ACCOUNT)?).ok()?;
            MenuCommand {
                show_window: true,
                input: Some(Input::Ui(UiCommand::Select { account })),
            }
        }
    })
}

/// Handles a click on an item of the app menu or the tray menu (main thread): quits for the
/// Quit items, otherwise shows the window `window_label` if the item asks for it, then feeds
/// the item's input.
pub fn handle_menu_event<R: Runtime>(
    app: &AppHandle<R>,
    window_label: &str,
    id: &str,
    feed: &Feed,
) {
    if id == ID_QUIT {
        graceful_quit(app);
        return;
    }
    let Some(command) = menu_command(id) else {
        return;
    };
    if command.show_window
        && let Some(window) = app.get_window(window_label)
    {
        // Nothing sensible to do if the window cannot come to the front.
        let _ = bring_to_front(&window);
    }
    if let Some(input) = command.input {
        feed(input);
    }
}

/// Id of the item (and shortcut) that selects `account`.
pub(crate) fn account_item_id(account: &MenuAccount) -> String {
    format!("{ID_ACCOUNT}{}", account.id)
}

/// "Label (n)", or just the label without unread.
fn tray_account_text(account: &MenuAccount) -> String {
    if account.unread > 0 {
        format!("{} ({})", account.label, account.unread)
    } else {
        account.label.clone()
    }
}

/// What the app menu shows: ids and labels of the enabled accounts.
fn app_menu_key(model: &MenuModel) -> Vec<(Uuid, String)> {
    model
        .accounts
        .iter()
        .map(|account| (account.id, account.label.clone()))
        .collect()
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

fn build_app_menu<R: Runtime>(app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<Menu<R>> {
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

/// Live native menu state kept by the host.
pub(crate) struct MenuState<R: Runtime> {
    last: Option<MenuModel>,
    tray: Option<TrayIcon<R>>,
    /// The menu the tray was built with.
    tray_menu: Option<Menu<R>>,
    shortcuts: native::Shortcuts,
}

impl<R: Runtime> Default for MenuState<R> {
    fn default() -> Self {
        MenuState {
            last: None,
            tray: None,
            tray_menu: None,
            shortcuts: native::Shortcuts::new(),
        }
    }
}

impl<R: Runtime> MenuState<R> {
    /// Applies `model`, rebuilding only what changed. The native shortcuts are added to the
    /// window `window_label` on the first call and feed their inputs to `feed`.
    pub(crate) fn apply(
        &mut self,
        app: &AppHandle<R>,
        window_label: &str,
        feed: &Feed,
        model: &MenuModel,
    ) -> tauri::Result<()> {
        let last = self.last.take();
        if last.is_none() {
            self.shortcuts.install(app, window_label, feed)?;
        }
        self.shortcuts.set_accounts(model);
        // Without an app menu (Linux), Settings and Quit stay reachable from the tray.
        if native::USES_APP_MENU
            && last
                .as_ref()
                .is_none_or(|last| app_menu_key(last) != app_menu_key(model))
        {
            app.set_menu(build_app_menu(app, model)?)?;
        }
        self.apply_tray(app, last.as_ref(), model)?;
        self.last = Some(model.clone());
        Ok(())
    }

    /// Creates, shows, updates or hides the tray icon.
    fn apply_tray(
        &mut self,
        app: &AppHandle<R>,
        last: Option<&MenuModel>,
        model: &MenuModel,
    ) -> tauri::Result<()> {
        match (&self.tray, model.show_tray) {
            (None, true) => {
                let menu = build_tray_menu(app, model)?;
                let tray = build_tray(app, &menu)?;
                tray.set_title(tray_title(model))?;
                self.tray = Some(tray);
                self.tray_menu = Some(menu);
            }
            (Some(tray), true) => {
                let shown_before = last.filter(|last| last.show_tray);
                if shown_before.is_none() {
                    tray.set_visible(true)?;
                }
                if shown_before.is_none_or(|last| tray_menu_key(last) != tray_menu_key(model)) {
                    let fresh = build_tray_menu(app, model)?;
                    replace_tray_menu(tray, self.tray_menu.as_ref(), fresh)?;
                }
                if shown_before.is_none_or(|last| last.tray_title != model.tray_title) {
                    tray.set_title(tray_title(model))?;
                }
            }
            (Some(tray), false) => tray.set_visible(false)?,
            (None, false) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(unread: u32, dnd: bool) -> MenuModel {
        MenuModel {
            accounts: vec![MenuAccount {
                id: Uuid::from_u128(7),
                label: "Work".into(),
                unread,
            }],
            dnd,
            show_tray: true,
            tray_title: if unread > 0 {
                unread.to_string()
            } else {
                String::new()
            },
        }
    }

    #[test]
    fn shortcuts_render_as_menu_accelerators() {
        let accelerators: Vec<String> = ITEM_SHORTCUTS
            .iter()
            .map(|(_, shortcut)| shortcut.accelerator())
            .collect();
        assert_eq!(
            accelerators,
            [
                "CmdOrCtrl+,",
                "CmdOrCtrl+Q",
                "CmdOrCtrl+R",
                "CmdOrCtrl+[",
                "CmdOrCtrl+]",
                "CmdOrCtrl+=",
                "CmdOrCtrl+-",
                "CmdOrCtrl+0",
                "CmdOrCtrl+Shift+O",
            ]
        );
        assert_eq!(item_shortcut(ID_DND), None);
    }

    #[test]
    fn the_first_nine_accounts_get_a_digit() {
        assert_eq!(account_shortcut(0), Some(Shortcut::key('1')));
        assert_eq!(account_shortcut(8), Some(Shortcut::key('9')));
        assert_eq!(account_shortcut(ACCOUNT_SHORTCUTS), None);
    }

    #[test]
    fn menu_ids_map_to_inputs() {
        let id = Uuid::from_u128(7);
        let account = &model(0, false).accounts[0];
        assert_eq!(
            menu_command(&account_item_id(account)),
            Some(MenuCommand {
                show_window: true,
                input: Some(Input::Ui(UiCommand::Select { account: id }))
            })
        );
        assert_eq!(
            menu_command(ID_ZOOM_IN).unwrap().input,
            Some(Input::Menu(MenuAction::ZoomIn))
        );
        assert_eq!(
            menu_command(ID_DND).unwrap(),
            MenuCommand {
                show_window: false,
                input: Some(Input::Menu(MenuAction::ToggleDnd))
            }
        );
        let settings = menu_command(ID_SETTINGS).unwrap();
        assert!(settings.show_window);
        assert_eq!(
            settings.input,
            Some(Input::Ui(UiCommand::SetDialog { open: true }))
        );
        assert_eq!(
            menu_command(ID_SHOW),
            Some(MenuCommand {
                show_window: true,
                input: None
            })
        );
        assert_eq!(menu_command("account:nope"), None);
        assert_eq!(
            menu_command(ID_QUIT),
            None,
            "quit is handled by graceful_quit"
        );
    }

    #[test]
    fn keys_and_texts() {
        assert_eq!(tray_account_text(&model(3, false).accounts[0]), "Work (3)");
        assert_eq!(tray_account_text(&model(0, false).accounts[0]), "Work");
        // unread changes the tray menu, not the app menu
        assert_eq!(
            app_menu_key(&model(0, false)),
            app_menu_key(&model(3, true))
        );
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
