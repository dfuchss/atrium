//! The menu of an account's sidebar icon ([`AccountMenu`]): its items and their ids.

use domain::{AccountMenu, AccountMenuAction, Input};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::{AppHandle, Runtime, Window};
use uuid::Uuid;

/// Prefix of the account menu items, followed by `<action>:<account id>`.
pub(super) const ID_ACCOUNT_MENU: &str = "account-menu:";
/// The account menu actions, to find the one an item id names.
const ACCOUNT_MENU_ACTIONS: [AccountMenuAction; 6] = [
    AccountMenuAction::Sleep,
    AccountMenuAction::WakeUp,
    AccountMenuAction::Settings,
    AccountMenuAction::EnableNotifications,
    AccountMenuAction::DisableNotifications,
    AccountMenuAction::Reload,
];

/// The input of the account menu item `id` (starting with [`ID_ACCOUNT_MENU`]).
pub(super) fn account_menu_input(id: &str) -> Option<Input> {
    let (key, account) = id.strip_prefix(ID_ACCOUNT_MENU)?.split_once(':')?;
    let action = ACCOUNT_MENU_ACTIONS
        .into_iter()
        .find(|&action| account_menu_key(action) == key)?;
    Some(Input::AccountMenu {
        account: Uuid::parse_str(account).ok()?,
        action,
    })
}

fn account_menu_item_id(account: Uuid, action: AccountMenuAction) -> String {
    format!("{ID_ACCOUNT_MENU}{}:{account}", account_menu_key(action))
}

/// Id part of an account menu item.
fn account_menu_key(action: AccountMenuAction) -> &'static str {
    match action {
        AccountMenuAction::Sleep => "sleep",
        AccountMenuAction::WakeUp => "wake-up",
        AccountMenuAction::Settings => "settings",
        AccountMenuAction::EnableNotifications => "enable-notifications",
        AccountMenuAction::DisableNotifications => "disable-notifications",
        AccountMenuAction::Reload => "reload",
    }
}

fn account_menu_text(action: AccountMenuAction) -> &'static str {
    match action {
        AccountMenuAction::Sleep => "Sleep",
        AccountMenuAction::WakeUp => "Wake Up",
        AccountMenuAction::Settings => "Settings…",
        AccountMenuAction::EnableNotifications => "Enable Notifications",
        AccountMenuAction::DisableNotifications => "Disable Notifications",
        AccountMenuAction::Reload => "Reload",
    }
}

/// Pops up `menu` at the mouse pointer over `window` (main thread).
pub(crate) fn popup_account_menu<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    menu: &AccountMenu,
) -> tauri::Result<()> {
    let mut builder = MenuBuilder::new(app);
    for item in &menu.items {
        builder = builder.item(
            &MenuItemBuilder::with_id(
                account_menu_item_id(menu.account, item.action),
                account_menu_text(item.action),
            )
            .enabled(item.enabled)
            .build(app)?,
        );
    }
    window.popup_menu(&builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::super::{MenuCommand, menu_command};
    use super::*;

    #[test]
    fn account_menu_ids_map_to_inputs() {
        let account = Uuid::from_u128(7);
        for action in ACCOUNT_MENU_ACTIONS {
            assert_eq!(
                menu_command(&account_menu_item_id(account, action)),
                Some(MenuCommand {
                    show_window: action == AccountMenuAction::Settings,
                    input: Some(Input::AccountMenu { account, action })
                })
            );
        }
        assert_eq!(account_menu_text(AccountMenuAction::Settings), "Settings…");
        assert_eq!(
            account_menu_item_id(account, AccountMenuAction::WakeUp),
            format!("account-menu:wake-up:{account}")
        );
    }
}
