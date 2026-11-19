//! Notification clicks by protocol activation: each toast carries the link
//! `<scheme>://notification/<native id>`, which Windows launches when the toast is clicked, on
//! screen or in the notification centre. The scheme is the app's identifier in lower case, so a
//! development identifier gets links of its own. Windows starts Atrium with the link, the
//! single-instance guard hands it to the running Atrium, and [`follow_notification_links`]
//! feeds the click like the notification plugin's handler does on the other platforms.
//!
//! The scheme is registered for the user (`HKEY_CURRENT_USER\Software\Classes\<scheme>`) at
//! every start of a guarded instance, so it starts the executable that runs last; any page can
//! open such a link, so only notifications Atrium still tracks are followed. A development
//! instance with its own config folder runs unguarded and keeps the plugin's notifications,
//! whose clicks count only while they are on screen.

use std::path::Path;

use domain::NotificationReq;
use domain::ports::PortResult;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Runtime};
use windows_registry::CURRENT_USER;

use super::NOTIFICATION_SOUND;
use crate::notifications::NotificationClicks;
use crate::{Logger, port_err};

/// Host and path prefix of a notification link, before the native id.
const NOTIFICATION_PATH: &str = "notification/";

/// The registered scheme and the `AppUserModelID` the toasts are shown under; in the app state
/// once the scheme is registered.
pub(crate) struct NotificationLinks {
    scheme: String,
    app_id: String,
}

/// Registers the scheme at start and puts the [`NotificationLinks`] in the app state; a failure
/// is logged and leaves the notifications to the plugin. Add it after the single-instance guard,
/// so a second start that hands over registers nothing.
pub(crate) fn register_notification_links<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("notification-links")
        .setup(|app, _api| {
            let identifier = &app.config().identifier;
            let scheme = identifier.to_ascii_lowercase();
            match std::env::current_exe() {
                Ok(executable) => match register_scheme(&scheme, &executable) {
                    Ok(()) => {
                        let app_id = toast_app_id(identifier, &executable).to_string();
                        app.manage(NotificationLinks { scheme, app_id });
                    }
                    Err(error) => Logger::new(app)
                        .log(&format!("cannot register the notification links: {error}")),
                },
                Err(error) => Logger::new(app).log(&format!("cannot locate Atrium: {error}")),
            }
            Ok(())
        })
        .build()
}

/// Lets `scheme` links start `executable` with the link as its argument.
fn register_scheme(scheme: &str, executable: &Path) -> windows_registry::Result<()> {
    let key = CURRENT_USER.create(format!(r"Software\Classes\{scheme}"))?;
    key.set_string("", "URL:Atrium notification")?;
    key.set_string("URL Protocol", "")?;
    key.create(r"shell\open\command")?
        .set_string("", format!("\"{}\" \"%1\"", executable.display()))
}

/// The `AppUserModelID` of the toasts, as the notification plugin chooses it: `PowerShell`'s for
/// a build in `target\debug` or `target\release`, else `identifier`.
fn toast_app_id<'a>(identifier: &'a str, executable: &Path) -> &'a str {
    let in_target = executable
        .parent()
        .is_some_and(|dir| dir.ends_with(r"target\debug") || dir.ends_with(r"target\release"));
    if in_target {
        platform_ffi::POWERSHELL_APP_ID
    } else {
        identifier
    }
}

/// Shows `request` as a toast whose click launches the link to `native_id`.
pub(crate) fn show_linked_notification(
    links: &NotificationLinks,
    native_id: i32,
    request: &NotificationReq,
) -> PortResult {
    let xml = platform_ffi::toast_xml(
        &request.title,
        &request.body,
        &notification_link(&links.scheme, native_id),
        request.sound.then_some(NOTIFICATION_SOUND),
    );
    platform_ffi::show_toast(&links.app_id, &xml).map_err(port_err)
}

/// Feeds a click for each notification link among `args` (the command line of Atrium started
/// again). A link to a notification that is not tracked (any more), or that is malformed, is
/// logged and ignored; so is everything before Atrium tracks notifications.
pub(crate) fn follow_notification_links<R: Runtime>(
    app: &AppHandle<R>,
    args: &[String],
    logger: &Logger,
) {
    let (Some(links), Some(clicks)) = (
        app.try_state::<NotificationLinks>(),
        app.try_state::<NotificationClicks>(),
    ) else {
        return;
    };
    let scheme_prefix = format!("{}:", links.scheme);
    for link in args.iter().skip(1) {
        if strip_prefix_ignoring_case(link, &scheme_prefix).is_none() {
            continue;
        }
        if !notification_id(&links.scheme, link).is_some_and(|id| clicks.clicked(id)) {
            logger.log(&format!("ignored notification link {link}"));
        }
    }
}

fn notification_link(scheme: &str, native_id: i32) -> String {
    format!("{scheme}://{NOTIFICATION_PATH}{native_id}")
}

/// The native id in the notification `link`; Windows may add a slash to it.
fn notification_id(scheme: &str, link: &str) -> Option<i32> {
    let id = strip_prefix_ignoring_case(link, &format!("{scheme}://{NOTIFICATION_PATH}"))?;
    let id = id.strip_suffix('/').unwrap_or(id);
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEME: &str = "org.fuchss.atrium";

    #[test]
    fn a_link_names_its_notification() {
        let link = notification_link(SCHEME, 42);
        assert_eq!(link, "org.fuchss.atrium://notification/42");
        assert_eq!(notification_id(SCHEME, &link), Some(42));
        assert_eq!(notification_id(SCHEME, &format!("{link}/")), Some(42));
        assert_eq!(
            notification_id(SCHEME, "ORG.Fuchss.Atrium://Notification/7"),
            Some(7)
        );
    }

    #[test]
    fn anything_else_names_no_notification() {
        for link in [
            "org.fuchss.atrium://notification/",
            "org.fuchss.atrium://notification/-1",
            "org.fuchss.atrium://notification/+1",
            "org.fuchss.atrium://notification/1?x=2",
            "org.fuchss.atrium://notification/1//",
            "org.fuchss.atrium://notification/99999999999",
            "org.fuchss.atrium://settings/1",
            "org.fuchss.atrium.dev://notification/1",
            "other://notification/1",
            "org.fuchss.atrium://notification/é",
            "",
        ] {
            assert_eq!(notification_id(SCHEME, link), None, "{link}");
        }
    }

    #[test]
    fn builds_in_target_show_toasts_as_powershell() {
        let identifier = "org.fuchss.atrium";
        for build in [
            r"D:\atrium\target\debug\atrium.exe",
            r"D:\atrium\target\release\atrium.exe",
        ] {
            assert_eq!(
                toast_app_id(identifier, Path::new(build)),
                platform_ffi::POWERSHELL_APP_ID
            );
        }
        let installed = Path::new(r"C:\Users\me\AppData\Local\Atrium\atrium.exe");
        assert_eq!(toast_app_id(identifier, installed), identifier);
    }
}
