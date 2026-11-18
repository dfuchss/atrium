//! Deleting the data stores of removed accounts, in the background: once the account's views
//! and windows are gone, with retries while `WebKit` still holds the store, and at startup for
//! stores no account owns any more.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use domain::{Account, unused_store_ids};
use protocol::Snapshot;
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use crate::labels::belongs_to_account;
use crate::logger::spawn_logged;
use crate::{Logger, TauriHost, native};

/// Pauses before retrying a failed data store deletion: right after its view closed, `WebKit`
/// may still hold the store.
const REMOVE_STORE_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];

/// Longest wait for the views and windows of a removed account to disappear before its data
/// store is deleted anyway (the retries below may still cover a slow close).
const ACCOUNT_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const ACCOUNT_CLOSE_POLL: Duration = Duration::from_millis(100);

/// Deletes the data store `store_id` of a removed account in the background, once the
/// account's views and windows are closed.
pub(crate) fn delete_data_store<R: Runtime>(
    app: &AppHandle<R>,
    logger: &Logger,
    store_id: [u8; 16],
) {
    let (app, thread_logger) = (app.clone(), logger.clone());
    // A data store id is its account's id.
    let account = Uuid::from_bytes(store_id);
    spawn_logged("remove-store", logger, move || {
        wait_until_account_closed(&app, account, &thread_logger);
        remove_data_store_with_retries(&app, store_id, &thread_logger);
    });
}

/// Blocks until no view or window of `account` remains, for at most [`ACCOUNT_CLOSE_TIMEOUT`]:
/// `WebKit` refuses to delete a data store that a live page still uses.
fn wait_until_account_closed<R: Runtime>(app: &AppHandle<R>, account: Uuid, logger: &Logger) {
    let mut waited = Duration::ZERO;
    while app
        .webviews()
        .keys()
        .any(|label| belongs_to_account(label, account))
    {
        if waited >= ACCOUNT_CLOSE_TIMEOUT {
            logger.log(&format!(
                "[{account}] views or windows still open after {} s, deleting data anyway",
                waited.as_secs()
            ));
            return;
        }
        std::thread::sleep(ACCOUNT_CLOSE_POLL);
        waited += ACCOUNT_CLOSE_POLL;
    }
}

/// Deletes the data store `store_id`, retrying after [`REMOVE_STORE_RETRY_DELAYS`]. Blocks, so
/// it runs on a thread of its own, never on the actor or the main thread.
fn remove_data_store_with_retries<R: Runtime>(
    app: &AppHandle<R>,
    store_id: [u8; 16],
    logger: &Logger,
) {
    let store = Uuid::from_bytes(store_id);
    let mut result = native::remove_data_store(app, store_id);
    for delay in REMOVE_STORE_RETRY_DELAYS {
        let Err(error) = &result else {
            break;
        };
        logger.log(&format!(
            "cannot delete data store {store}: {error}; retrying in {} s",
            delay.as_secs()
        ));
        std::thread::sleep(delay);
        result = native::remove_data_store(app, store_id);
    }
    match result {
        Ok(()) => logger.log(&format!("deleted data store {store}")),
        Err(error) => logger.log(&format!(
            "giving up deleting data store {store}: {error} (removed again at the next start)"
        )),
    }
}

/// Whether the data store of `account` is in use now: the account is one of `current` (the
/// accounts of the live snapshot) or one of its views or windows is open (`labels`). The sweep
/// lists the stores while the app already runs, so an account added meanwhile has a store that
/// the startup list lacks; its view is open before the snapshot shows the account.
fn store_in_use<'a>(
    account: Uuid,
    current: &[Uuid],
    mut labels: impl Iterator<Item = &'a str>,
) -> bool {
    current.contains(&account) || labels.any(|label| belongs_to_account(label, account))
}

fn account_ids(snapshot: &Mutex<Snapshot>) -> Vec<Uuid> {
    let snapshot = snapshot.lock().unwrap_or_else(PoisonError::into_inner);
    snapshot.accounts.iter().map(|view| view.id).collect()
}

impl<R: Runtime> TauriHost<R> {
    /// Deletes, in the background, the data stores of this app that belong to none of
    /// `accounts` and are not in use when they are deleted (see [`store_in_use`]; `current` is
    /// the live snapshot). Call it only with the complete account list (see
    /// `FsStore::lists_all_accounts`).
    pub fn remove_unused_data_stores(&self, accounts: Vec<Account>, current: Arc<Mutex<Snapshot>>) {
        let (app, logger) = (self.app.clone(), self.logger.clone());
        spawn_logged("unused-stores", &self.logger, move || {
            let existing = match native::data_store_ids(&app) {
                Ok(existing) => existing,
                Err(error) => {
                    logger.log(&format!("cannot list data stores: {error}"));
                    return;
                }
            };
            let unused = unused_store_ids(&existing, &accounts);
            logger.log(&format!(
                "{} data stores, {} without account",
                existing.len(),
                unused.len()
            ));
            for store_id in unused {
                let account = Uuid::from_bytes(store_id);
                let in_use = store_in_use(
                    account,
                    &account_ids(&current),
                    app.webviews().keys().map(String::as_str),
                );
                if in_use {
                    logger.log(&format!(
                        "keeping data store {account}: its account was added"
                    ));
                } else {
                    remove_data_store_with_retries(&app, store_id, &logger);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use protocol::UI_WEBVIEW;

    use super::*;
    use crate::labels::{popup_label, view_label};

    #[test]
    fn stores_of_current_accounts_or_open_views_are_in_use() {
        let (listed, viewed, orphan) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let current = [listed];
        let labels = [view_label(viewed), UI_WEBVIEW.to_string()];
        let open = || labels.iter().map(String::as_str);
        assert!(store_in_use(listed, &current, open()));
        assert!(store_in_use(viewed, &current, open()));
        assert!(!store_in_use(orphan, &current, open()));
        let popup = [popup_label(orphan, 4)];
        assert!(store_in_use(
            orphan,
            &current,
            popup.iter().map(String::as_str)
        ));
    }
}
