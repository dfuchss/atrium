//! The browser for external links: `AppSettings.browser` holds the registry key name of an
//! installed web browser under `Software\Clients\StartMenuInternet` (e.g. `Google Chrome`),
//! which browsers register in `HKEY_CURRENT_USER` or `HKEY_LOCAL_MACHINE`. `mailto:` links and
//! links without a usable choice go to the default handler of Windows.

use std::os::windows::process::CommandExt;
use std::process::Command;

use domain::ports::PortResult;
use protocol::BrowserChoice;
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;
use url::Url;
use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};

use crate::{Logger, port_err};

const CLIENTS: &str = r"SOFTWARE\Clients\StartMenuInternet";
const NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const NO_WINDOW: u32 = 0x0800_0000;

/// Opens `url` with the installed browser `browser` (a registry key name), or with the default
/// handler: always for links other than `http` and `https`, and when the browser cannot be
/// started (logged).
pub(crate) fn open_url_with<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    browser: Option<&str>,
    logger: &Logger,
) -> PortResult {
    let chosen = browser.filter(|_| is_web_link(url));
    if let Some(id) = chosen {
        match launch(id, url) {
            Ok(()) => return Ok(()),
            Err(reason) => logger.log(&format!("browser {id} {reason}, using the default")),
        }
    }
    app.opener().open_url(url, None::<&str>).map_err(port_err)
}

fn is_web_link(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Starts the browser `id` with `url` as its only argument, detached and without a console.
fn launch(id: &str, url: &str) -> Result<(), String> {
    let command = open_command(id).ok_or("is not an installed browser")?;
    let program = executable(&command).ok_or("has no usable command")?;
    Command::new(program)
        .arg(url)
        .creation_flags(NEW_PROCESS_GROUP | NO_WINDOW)
        .spawn()
        .map(drop)
        .map_err(|error| format!("cannot start: {error}"))
}

/// The `shell\open\command` of the browser `id`, from the user's registrations first.
fn open_command(id: &str) -> Option<String> {
    [CURRENT_USER, LOCAL_MACHINE].iter().find_map(|root| {
        let key = root
            .open(format!(r"{CLIENTS}\{id}\shell\open\command"))
            .ok()?;
        key.get_string("").ok()
    })
}

/// The executable of a registered command line: the quoted path, else the text up to the
/// first `.exe` (paths with spaces are common), else the first word.
fn executable(command: &str) -> Option<String> {
    let command = command.trim();
    let program = if let Some(quoted) = command.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        match command.to_ascii_lowercase().find(".exe") {
            Some(end) => &command[..end + ".exe".len()],
            None => command.split_whitespace().next()?,
        }
    };
    (!program.is_empty()).then(|| program.to_string())
}

/// The browsers registered for the user or the machine, by name.
#[expect(
    clippy::unnecessary_wraps,
    reason = "same function as on macOS, which has no list"
)]
pub(crate) fn installed_browsers() -> Option<Vec<BrowserChoice>> {
    let choices = [CURRENT_USER, LOCAL_MACHINE]
        .iter()
        .filter_map(|root| root.open(CLIENTS).ok())
        .flat_map(|clients| registered(&clients));
    Some(offered(choices))
}

/// The browsers below `clients`: key name and display name.
fn registered(clients: &Key) -> Vec<BrowserChoice> {
    let names = clients.keys().into_iter().flatten();
    names
        .map(|id| {
            let name = clients
                .open(&id)
                .ok()
                .and_then(|key| key.get_string("").ok())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            BrowserChoice { id, name }
        })
        .collect()
}

/// Drops repeated ids (the first wins) and sorts by name.
fn offered(choices: impl IntoIterator<Item = BrowserChoice>) -> Vec<BrowserChoice> {
    let mut offered: Vec<BrowserChoice> = Vec::new();
    for choice in choices {
        if offered.iter().all(|known| known.id != choice.id) {
            offered.push(choice);
        }
    }
    offered.sort_by_key(|choice| choice.name.to_lowercase());
    offered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, name: &str) -> BrowserChoice {
        BrowserChoice {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn takes_the_quoted_path() {
        assert_eq!(
            executable(r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#),
            Some(r"C:\Program Files\Mozilla Firefox\firefox.exe".into())
        );
        assert_eq!(
            executable(r#""C:\Program Files\Edge\msedge.exe""#),
            Some(r"C:\Program Files\Edge\msedge.exe".into())
        );
    }

    #[test]
    fn takes_an_unquoted_path_up_to_its_exe() {
        assert_eq!(
            executable(r"C:\Program Files\Brave\brave.exe --flag"),
            Some(r"C:\Program Files\Brave\brave.exe".into())
        );
        assert_eq!(
            executable(r"C:\Opera\launcher.EXE"),
            Some(r"C:\Opera\launcher.EXE".into())
        );
        assert_eq!(executable("browser --flag"), Some("browser".into()));
    }

    #[test]
    fn rejects_empty_commands() {
        assert_eq!(executable(""), None);
        assert_eq!(executable("  "), None);
        assert_eq!(executable(r#""""#), None);
    }

    #[test]
    fn offers_browsers_by_name_without_duplicates() {
        let list = offered([
            choice("Firefox-1", "Firefox"),
            choice("Brave", "brave"),
            choice("Firefox-1", "Other"),
        ]);
        let ids: Vec<&str> = list.iter().map(|choice| choice.id.as_str()).collect();
        assert_eq!(ids, ["Brave", "Firefox-1"]);
        assert_eq!(list[1].name, "Firefox");
    }

    #[test]
    fn only_web_links_use_the_chosen_browser() {
        assert!(is_web_link("https://example.org"));
        assert!(is_web_link("HTTP://example.org"));
        assert!(!is_web_link("mailto:a@b.org"));
    }

    #[test]
    #[ignore = "reads this machine's registry"]
    fn lists_this_machines_browsers() {
        println!("{:?}", installed_browsers());
    }
}
