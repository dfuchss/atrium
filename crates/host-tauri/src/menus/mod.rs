//! Native app menu (`app_menu`, with the shortcuts) and the tray (menu bar) icon (`tray`),
//! both derived from the core's [`MenuModel`], and the menu of an account's sidebar icon
//! (`account_menu`). Menu events of all of them arrive at one global handler, which turns them
//! into [`Input`]s with [`menu_command`]; the item ids and shortcuts live here.

mod account_menu;
mod app_menu;
mod tray;

use domain::{AccountMenuAction, Input, MenuAccount, MenuAction, MenuModel};
use protocol::UiCommand;
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use crate::native::Shortcuts;
use crate::{Feed, bring_to_front, graceful_quit, native};
pub(crate) use account_menu::popup_account_menu;
use account_menu::{ID_ACCOUNT_MENU, account_menu_input};
use app_menu::{app_menu_key, build_app_menu};
use tray::Tray;

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
/// Accounts 1 to 9 get the shortcuts Cmd+1 to Cmd+9 (Ctrl elsewhere).
pub(crate) const ACCOUNT_SHORTCUTS: usize = 9;

/// A keyboard shortcut: `key` held with Cmd on macOS and Ctrl elsewhere, and with Shift if
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

/// The menu items with a shortcut, by id: the macOS menu and the window shortcuts elsewhere
/// (`window_shortcuts`) both take their shortcuts from here.
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
/// [`ACCOUNT_SHORTCUTS`] accounts: Cmd+1 to Cmd+9 (Ctrl elsewhere).
pub(crate) fn account_shortcut(index: usize) -> Option<Shortcut> {
    if index >= ACCOUNT_SHORTCUTS {
        return None;
    }
    char::from_digit(u32::try_from(index + 1).ok()?, 10).map(Shortcut::key)
}

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
        _ if id.starts_with(ID_ACCOUNT_MENU) => {
            let input = account_menu_input(id)?;
            MenuCommand {
                show_window: matches!(
                    input,
                    Input::AccountMenu {
                        action: AccountMenuAction::Settings,
                        ..
                    }
                ),
                input: Some(input),
            }
        }
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

/// Live native menu state kept by the host.
pub(crate) struct MenuState<R: Runtime> {
    last: Option<MenuModel>,
    tray: Tray<R>,
    shortcuts: Shortcuts,
}

impl<R: Runtime> Default for MenuState<R> {
    fn default() -> Self {
        MenuState {
            last: None,
            tray: Tray::default(),
            shortcuts: Shortcuts::new(),
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
        self.tray.apply(app, last.as_ref(), model)?;
        self.last = Some(model.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn model(unread: u32, dnd: bool) -> MenuModel {
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
        assert_eq!(menu_command("account-menu:sleep:nope"), None);
        assert_eq!(menu_command(&format!("account-menu:nap:{id}")), None);
        assert_eq!(
            menu_command(ID_QUIT),
            None,
            "quit is handled by graceful_quit"
        );
    }
}
