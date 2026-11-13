//! Native app menu (with the shortcuts) and the tray (menu bar) icon, both derived from the
//! core's [`MenuModel`]. Menu events of both arrive at one global handler, which turns them
//! into [`Input`]s with [`menu_command`].

use domain::{Input, MenuAccount, MenuAction, MenuModel};
use protocol::UiCommand;
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

pub const ID_SHOW: &str = "show";
pub const ID_SETTINGS: &str = "settings";
pub const ID_DND: &str = "dnd";
pub const ID_RELOAD: &str = "reload";
pub const ID_BACK: &str = "back";
pub const ID_FORWARD: &str = "forward";
pub const ID_ZOOM_IN: &str = "zoom-in";
pub const ID_ZOOM_OUT: &str = "zoom-out";
pub const ID_ZOOM_RESET: &str = "zoom-reset";
pub const ID_OPEN_BROWSER: &str = "open-in-browser";
/// Prefix of the account items (app menu "Accounts" and tray), followed by the account id.
pub const ID_ACCOUNT: &str = "account:";
const TRAY_ID: &str = "messengers-tray";

/// Monochrome template image (black + alpha); macOS tints it for light/dark menu bars.
const TRAY_ICON: &[u8] = include_bytes!("../assets/tray-icon.png");

/// What a menu item does: optionally bring the main window to the front, then feed an input.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuCommand {
    pub show_window: bool,
    pub input: Option<Input>,
}

/// Maps a menu item id to its command; None for ids we do not own (predefined items).
pub fn menu_command(id: &str) -> Option<MenuCommand> {
    let menu = |action| MenuCommand { show_window: false, input: Some(Input::Menu(action)) };
    Some(match id {
        ID_SHOW => MenuCommand { show_window: true, input: None },
        ID_SETTINGS => {
            MenuCommand { show_window: true, input: Some(Input::Ui(UiCommand::SetDialog { open: true })) }
        }
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
            MenuCommand { show_window: true, input: Some(Input::Ui(UiCommand::Select { account })) }
        }
    })
}

fn account_id(a: &MenuAccount) -> String {
    format!("{ID_ACCOUNT}{}", a.id)
}

/// "Label (n)", or just the label without unread.
pub fn tray_account_text(a: &MenuAccount) -> String {
    if a.unread > 0 { format!("{} ({})", a.label, a.unread) } else { a.label.clone() }
}

/// What the app menu shows: ids and labels of the enabled accounts.
pub fn app_menu_key(m: &MenuModel) -> Vec<(Uuid, String)> {
    m.accounts.iter().map(|a| (a.id, a.label.clone())).collect()
}

/// What the tray menu shows: account texts and the DnD state.
pub fn tray_menu_key(m: &MenuModel) -> (Vec<(Uuid, String)>, bool) {
    (m.accounts.iter().map(|a| (a.id, tray_account_text(a))).collect(), m.dnd)
}

pub fn build_app_menu<R: Runtime>(app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<Menu<R>> {
    let item = |id: &str, text: &str, accel: &str| MenuItemBuilder::with_id(id, text).accelerator(accel).build(app);
    let app_menu = SubmenuBuilder::new(app, "Messengers")
        .about(None)
        .separator()
        .item(&item(ID_SETTINGS, "Settings…", "CmdOrCtrl+,")?)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    // Without an Edit menu, Cmd+C/V/X/A/Z do not reach the webviews on macOS.
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item(ID_RELOAD, "Reload", "CmdOrCtrl+R")?)
        .item(&item(ID_BACK, "Back", "CmdOrCtrl+[")?)
        .item(&item(ID_FORWARD, "Forward", "CmdOrCtrl+]")?)
        .separator()
        .item(&item(ID_ZOOM_IN, "Zoom In", "CmdOrCtrl+=")?)
        .item(&item(ID_ZOOM_OUT, "Zoom Out", "CmdOrCtrl+-")?)
        .item(&item(ID_ZOOM_RESET, "Actual Size", "CmdOrCtrl+0")?)
        .separator()
        .item(&item(ID_OPEN_BROWSER, "Open Page in Browser", "CmdOrCtrl+Shift+O")?)
        .separator()
        .fullscreen()
        .build()?;
    let mut accounts = SubmenuBuilder::new(app, "Accounts");
    for (i, a) in model.accounts.iter().enumerate() {
        let mut b = MenuItemBuilder::with_id(account_id(a), &a.label);
        if i < 9 {
            b = b.accelerator(format!("CmdOrCtrl+{}", i + 1));
        }
        accounts = accounts.item(&b.build(app)?);
    }
    if model.accounts.is_empty() {
        accounts = accounts.item(&MenuItemBuilder::new("No accounts").enabled(false).build(app)?);
    }
    let accounts = accounts.build()?;
    // Close = performClose: on the key window -> CloseRequested, so the main window follows
    // close-to-background and popups simply close.
    let window = SubmenuBuilder::new(app, "Window").minimize().close_window().build()?;
    MenuBuilder::new(app).items(&[&app_menu, &edit, &view, &accounts, &window]).build()
}

pub fn build_tray_menu<R: Runtime>(app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<Menu<R>> {
    let mut menu = MenuBuilder::new(app).text(ID_SHOW, "Show Messengers").separator();
    for a in &model.accounts {
        menu = menu.text(account_id(a), tray_account_text(a));
    }
    if !model.accounts.is_empty() {
        menu = menu.separator();
    }
    let dnd = CheckMenuItemBuilder::with_id(ID_DND, "Do Not Disturb").checked(model.dnd).build(app)?;
    menu.item(&dnd).text(ID_SETTINGS, "Settings…").separator().quit_with_text("Quit").build()
}

pub fn build_tray<R: Runtime>(app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<TrayIcon<R>> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip("Messengers")
        .menu(&build_tray_menu(app, model)?)
        .show_menu_on_left_click(true)
        .build(app)
}

/// Tray title (unread total); None when empty so no space is reserved.
pub fn tray_title(model: &MenuModel) -> Option<String> {
    Some(model.tray_title.clone()).filter(|t| !t.is_empty())
}

/// Live native menu state kept by the host.
pub(crate) struct MenuState<R: Runtime> {
    pub last: Option<MenuModel>,
    pub tray: Option<TrayIcon<R>>,
}

impl<R: Runtime> Default for MenuState<R> {
    fn default() -> Self {
        MenuState { last: None, tray: None }
    }
}

impl<R: Runtime> MenuState<R> {
    /// Applies `model`, rebuilding only what changed.
    pub fn apply(&mut self, app: &AppHandle<R>, model: &MenuModel) -> tauri::Result<()> {
        let last = self.last.take();
        let changed = |key: &dyn Fn(&MenuModel) -> String| last.as_ref().is_none_or(|l| key(l) != key(model));
        if changed(&|m| format!("{:?}", app_menu_key(m))) {
            app.set_menu(build_app_menu(app, model)?)?;
        }
        match (&self.tray, model.show_tray) {
            (None, true) => {
                let tray = build_tray(app, model)?;
                tray.set_title(tray_title(model))?;
                self.tray = Some(tray);
            }
            (Some(tray), true) => {
                let was_visible = last.as_ref().is_some_and(|l| l.show_tray);
                if !was_visible {
                    tray.set_visible(true)?;
                }
                if !was_visible || changed(&|m| format!("{:?}", tray_menu_key(m))) {
                    tray.set_menu(Some(build_tray_menu(app, model)?))?;
                }
                if !was_visible || changed(&|m| m.tray_title.clone()) {
                    tray.set_title(tray_title(model))?;
                }
            }
            (Some(tray), false) => tray.set_visible(false)?,
            (None, false) => {}
        }
        self.last = Some(model.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(unread: u32, dnd: bool) -> MenuModel {
        MenuModel {
            accounts: vec![MenuAccount { id: Uuid::from_u128(7), label: "Work".into(), unread }],
            dnd,
            show_tray: true,
            tray_title: if unread > 0 { unread.to_string() } else { String::new() },
        }
    }

    #[test]
    fn menu_ids_map_to_inputs() {
        let id = Uuid::from_u128(7);
        let a = &model(0, false).accounts[0];
        assert_eq!(
            menu_command(&account_id(a)),
            Some(MenuCommand { show_window: true, input: Some(Input::Ui(UiCommand::Select { account: id })) })
        );
        assert_eq!(menu_command(ID_ZOOM_IN).unwrap().input, Some(Input::Menu(MenuAction::ZoomIn)));
        assert_eq!(menu_command(ID_DND).unwrap(), MenuCommand { show_window: false, input: Some(Input::Menu(MenuAction::ToggleDnd)) });
        let settings = menu_command(ID_SETTINGS).unwrap();
        assert!(settings.show_window);
        assert_eq!(settings.input, Some(Input::Ui(UiCommand::SetDialog { open: true })));
        assert_eq!(menu_command(ID_SHOW), Some(MenuCommand { show_window: true, input: None }));
        assert_eq!(menu_command("account:nope"), None);
        assert_eq!(menu_command("quit"), None);
    }

    #[test]
    fn keys_and_texts() {
        assert_eq!(tray_account_text(&model(3, false).accounts[0]), "Work (3)");
        assert_eq!(tray_account_text(&model(0, false).accounts[0]), "Work");
        // unread changes the tray menu, not the app menu
        assert_eq!(app_menu_key(&model(0, false)), app_menu_key(&model(3, true)));
        assert_ne!(tray_menu_key(&model(0, false)), tray_menu_key(&model(3, false)));
        assert_ne!(tray_menu_key(&model(0, false)), tray_menu_key(&model(0, true)));
        assert_eq!(tray_title(&model(0, false)), None);
        assert_eq!(tray_title(&model(2, false)).as_deref(), Some("2"));
    }

    #[test]
    fn tray_icon_is_a_png() {
        assert!(Image::from_bytes(TRAY_ICON).is_ok());
    }
}
