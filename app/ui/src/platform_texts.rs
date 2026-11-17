//! Settings wording that depends on the operating system the host runs on.

use leptos::prelude::*;
use protocol::{OperatingSystem, Snapshot};

/// The texts of the settings that differ between platforms.
pub(crate) struct Texts {
    pub(crate) theme_hint: &'static str,
    pub(crate) tray_label: &'static str,
    pub(crate) tray_hint: &'static str,
    pub(crate) badge_tab: &'static str,
    pub(crate) account_badge_toggle: &'static str,
    /// Hint of the after-wake selects.
    pub(crate) after_wake_hint: &'static str,
    /// About: the state of `WebKit`'s web process sandbox where it is not the system's default.
    pub(crate) web_process_sandbox: Option<&'static str>,
    /// The user agent option that leaves the system webview's own.
    pub(crate) system_user_agent: &'static str,
    /// Desktop integration: the menu Atrium can install itself in, as a label and in a sentence.
    pub(crate) app_menu_label: &'static str,
    pub(crate) app_menu: &'static str,
}

const MACOS: Texts = Texts {
    theme_hint: "Auto follows macOS. Service pages follow if they support it.",
    tray_label: "Menu bar icon",
    tray_hint: "Unread count and account menu in the menu bar.",
    badge_tab: "Dock badge",
    account_badge_toggle: "Count in dock badge",
    after_wake_hint: "Check: reload only services that stop responding after your Mac wakes. Reload: reload every service. Background wakes (Power Nap) change nothing.",
    web_process_sandbox: None,
    system_user_agent: "System WebKit",
    app_menu_label: "Application menu",
    app_menu: "application menu",
};

const LINUX: Texts = Texts {
    theme_hint: "Auto follows your desktop's light or dark setting. Service pages follow if they support it.",
    tray_label: "Tray icon",
    tray_hint: "Unread count and account menu in the system tray.",
    badge_tab: "Launcher badge",
    account_badge_toggle: "Count in launcher badge",
    after_wake_hint: "Check: reload only services that stop responding after your computer wakes from sleep. Reload: reload every service.",
    web_process_sandbox: Some("not available on Linux builds"),
    system_user_agent: "System WebKit",
    app_menu_label: "Application menu",
    app_menu: "application menu",
};

const WINDOWS: Texts = Texts {
    theme_hint: "Auto follows the light or dark app mode of Windows. Service pages follow if they support it.",
    tray_label: "Tray icon",
    tray_hint: "Account menu in the notification area of the taskbar.",
    badge_tab: "Taskbar badge",
    account_badge_toggle: "Count in taskbar badge",
    after_wake_hint: "Check: reload only services that stop responding after your computer wakes from sleep. Reload: reload every service.",
    web_process_sandbox: None,
    system_user_agent: "System WebView2",
    app_menu_label: "Start menu",
    app_menu: "Start menu",
};

impl Texts {
    fn of(os: OperatingSystem) -> &'static Texts {
        match os {
            OperatingSystem::MacOs => &MACOS,
            OperatingSystem::Linux => &LINUX,
            OperatingSystem::Windows => &WINDOWS,
        }
    }
}

/// The snapshot signal, kept in the context to look up the platform.
#[derive(Clone, Copy)]
struct SnapshotSource(ReadSignal<Snapshot>);

pub(crate) fn provide(snapshot: ReadSignal<Snapshot>) {
    provide_context(SnapshotSource(snapshot));
}

/// Texts for the platform of the host. The settings dialog opens only after the first
/// snapshot, so the platform is known by then and does not change.
pub(crate) fn texts() -> &'static Texts {
    let source = expect_context::<SnapshotSource>();
    Texts::of(source.0.with_untracked(|snapshot| snapshot.app.os))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_and_windows_wording_avoids_macos_terms() {
        for os in [OperatingSystem::Linux, OperatingSystem::Windows] {
            assert_avoids_macos_terms(Texts::of(os));
        }
    }

    fn assert_avoids_macos_terms(texts: &Texts) {
        for text in [
            texts.theme_hint,
            texts.tray_label,
            texts.tray_hint,
            texts.badge_tab,
            texts.account_badge_toggle,
            texts.after_wake_hint,
            texts.system_user_agent,
            texts.app_menu_label,
            texts.app_menu,
        ]
        .into_iter()
        .chain(texts.web_process_sandbox)
        {
            for term in ["macOS", "Mac", "Dock", "dock", "menu bar", "Power Nap"] {
                assert!(!text.contains(term), "{text:?} mentions {term}");
            }
        }
    }

    #[test]
    fn only_linux_tells_about_the_web_process_sandbox() {
        assert_eq!(Texts::of(OperatingSystem::MacOs).web_process_sandbox, None);
        assert_eq!(
            Texts::of(OperatingSystem::Windows).web_process_sandbox,
            None
        );
        assert!(
            Texts::of(OperatingSystem::Linux)
                .web_process_sandbox
                .is_some()
        );
    }
}
