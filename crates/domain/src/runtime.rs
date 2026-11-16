//! Maps [`Effect`]s onto the ports. The app's actor calls [`apply`] for every effect returned
//! by [`crate::Core::handle`]; errors are logged via [`Platform::log`] and never stop the loop.

use crate::core::Effect;
use crate::model::Persisted;
use crate::ports::{Notifier, Platform, Store, UiSink, WebviewHost};
use crate::ports::{PortError, PortResult};
use crate::security::opener_url_allowed;

pub struct Ports<'a> {
    pub host: &'a dyn WebviewHost,
    pub notifier: &'a dyn Notifier,
    pub platform: &'a dyn Platform,
    pub ui: &'a dyn UiSink,
    pub store: &'a dyn Store,
    /// Saved data was unreadable at startup: [`Effect::Persist`] is dropped.
    pub read_only: bool,
}

/// What the saved settings gave at startup. A store reports a missing file as an empty
/// `Persisted`, so any error here means a file that may exist but cannot be read.
#[derive(Debug, PartialEq)]
pub enum StartupLoad {
    Loaded(Persisted),
    /// The saved accounts could not be read: run empty and never overwrite the file.
    Unreadable(String),
}

impl StartupLoad {
    pub fn from_store(result: PortResult<Persisted>) -> StartupLoad {
        match result {
            Ok(persisted) => StartupLoad::Loaded(persisted),
            Err(error) => StartupLoad::Unreadable(error.to_string()),
        }
    }

    /// True if nothing may be written back this run.
    pub fn is_read_only(&self) -> bool {
        matches!(self, StartupLoad::Unreadable(_))
    }

    /// The saved data, or an empty state if unreadable.
    pub fn into_persisted(self) -> Persisted {
        match self {
            StartupLoad::Loaded(persisted) => persisted,
            StartupLoad::Unreadable(_) => Persisted::default(),
        }
    }
}

/// True if `effect` must be dropped: persisting while the saved data was unreadable would
/// replace it with the empty fallback.
pub fn skip_effect(effect: &Effect, read_only: bool) -> bool {
    read_only && matches!(effect, Effect::Persist(_))
}

#[expect(clippy::too_many_lines, reason = "one flat arm per effect")]
pub fn apply(effect: Effect, ports: &Ports<'_>) {
    if skip_effect(&effect, ports.read_only) {
        return;
    }
    let (name, result) = match effect {
        Effect::CreateView {
            spec,
            bounds,
            visible,
        } => ("CreateView", ports.host.create(&spec, bounds, visible)),
        Effect::SetBounds { account, bounds } => {
            ("SetBounds", ports.host.set_bounds(account, bounds))
        }
        Effect::SetUiBounds(bounds) => ("SetUiBounds", ports.host.set_ui_bounds(bounds)),
        Effect::Show(account) => ("Show", ports.host.show(account)),
        Effect::Hide(account) => ("Hide", ports.host.hide(account)),
        Effect::Destroy(account) => ("Destroy", ports.host.destroy(account)),
        Effect::DeleteProfile { store_id } => {
            ("DeleteProfile", ports.host.delete_profile(store_id))
        }
        Effect::ClearData(account) => ("ClearData", ports.host.clear_data(account)),
        Effect::Reload(account) => ("Reload", ports.host.reload(account)),
        Effect::ForceReload(account) => ("ForceReload", ports.host.force_reload(account)),
        Effect::SetViewPolicy { account, policy } => {
            ("SetViewPolicy", ports.host.set_view_policy(account, policy))
        }
        Effect::SetZoom { account, zoom } => ("SetZoom", ports.host.set_zoom(account, zoom)),
        Effect::FindIcon { account, script } => {
            ("FindIcon", ports.host.find_icon(account, &script))
        }
        Effect::FetchIcon { account, url } => {
            ("FetchIcon", ports.platform.fetch_icon(account, &url))
        }
        Effect::FetchSimpleIcon { account, slug } => (
            "FetchSimpleIcon",
            ports.platform.fetch_simple_icon(account, &slug),
        ),
        Effect::OpenInBrowser { account, browser } => (
            "OpenInBrowser",
            ports
                .host
                .current_url(account)
                .and_then(|url| open_checked(&url, browser.as_deref(), ports)),
        ),
        Effect::UpdateMenus(model) => ("UpdateMenus", ports.platform.set_menus(&model)),
        Effect::Poll { account, script } => ("Poll", ports.host.poll(account, &script)),
        Effect::Eval { account, script } => ("Eval", ports.host.eval(account, &script)),
        Effect::Notify(request) => ("Notify", ports.notifier.notify(&request)),
        Effect::SetDockBadge(badge) => ("SetDockBadge", ports.platform.set_dock_badge(badge)),
        Effect::OpenExternal { url, browser } => (
            "OpenExternal",
            open_checked(&url, browser.as_deref(), ports),
        ),
        Effect::OpenServicesFolder => ("OpenServicesFolder", ports.platform.open_services_folder()),
        Effect::LoadServices => ("LoadServices", ports.platform.load_services()),
        Effect::FocusWindow => ("FocusWindow", ports.platform.focus_window()),
        Effect::ApplySystemSettings(settings) => (
            "ApplySystemSettings",
            ports.platform.apply_system_settings(&settings),
        ),
        Effect::ApplyTheme(theme) => ("ApplyTheme", ports.platform.apply_theme(theme)),
        Effect::Persist(persisted) => ("Persist", ports.store.save(&persisted)),
        Effect::EmitSnapshot(snapshot) => ("EmitSnapshot", ports.ui.emit_snapshot(&snapshot)),
        Effect::Log(message) => {
            ports.platform.log(&message);
            return;
        }
    };
    if let Err(error) = result {
        ports.platform.log(&format!("{name} failed: {error}"));
    }
}

/// Hands only http(s)/mailto URLs to the opener (the core already filters its own effects;
/// URLs read from a view, as for `OpenInBrowser`, are checked here).
fn open_checked(url: &str, browser: Option<&str>, ports: &Ports<'_>) -> PortResult {
    if !opener_url_allowed(url) {
        return Err(PortError(format!(
            "not opening {url:.200}: scheme not allowed"
        )));
    }
    ports.platform.open_external(url, browser)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use protocol::{AppSettings, Snapshot, Theme};
    use uuid::Uuid;

    use super::*;
    use crate::core::{Bounds, DockBadge, MenuModel, NotificationReq, ViewPolicy, ViewSpec};
    use crate::model::Persisted;
    use crate::ports::{Notifier, Platform, Store, UiSink, WebviewHost};

    /// Ports that only record opened URLs and log lines; the view shows `url`.
    struct Fake {
        url: String,
        opened: Mutex<Vec<String>>,
        logs: Mutex<Vec<String>>,
    }

    impl WebviewHost for Fake {
        fn create(&self, _: &ViewSpec, _: Bounds, _: bool) -> PortResult {
            Ok(())
        }
        fn set_bounds(&self, _: Uuid, _: Bounds) -> PortResult {
            Ok(())
        }
        fn set_ui_bounds(&self, _: Bounds) -> PortResult {
            Ok(())
        }
        fn show(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn hide(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn destroy(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn reload(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn force_reload(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn eval(&self, _: Uuid, _: &str) -> PortResult {
            Ok(())
        }
        fn poll(&self, _: Uuid, _: &str) -> PortResult {
            Ok(())
        }
        fn delete_profile(&self, _: [u8; 16]) -> PortResult {
            Ok(())
        }
        fn set_view_policy(&self, _: Uuid, _: ViewPolicy) -> PortResult {
            Ok(())
        }
        fn set_zoom(&self, _: Uuid, _: f64) -> PortResult {
            Ok(())
        }
        fn clear_data(&self, _: Uuid) -> PortResult {
            Ok(())
        }
        fn current_url(&self, _: Uuid) -> PortResult<String> {
            Ok(self.url.clone())
        }
        fn find_icon(&self, _: Uuid, _: &str) -> PortResult {
            Ok(())
        }
    }

    impl Notifier for Fake {
        fn notify(&self, _: &NotificationReq) -> PortResult {
            Ok(())
        }
    }

    impl Platform for Fake {
        fn set_dock_badge(&self, _: DockBadge) -> PortResult {
            Ok(())
        }
        fn open_external(&self, url: &str, _: Option<&str>) -> PortResult {
            self.opened.lock().unwrap().push(url.to_string());
            Ok(())
        }
        fn apply_system_settings(&self, _: &AppSettings) -> PortResult {
            Ok(())
        }
        fn apply_theme(&self, _: Theme) -> PortResult {
            Ok(())
        }
        fn open_services_folder(&self) -> PortResult {
            Ok(())
        }
        fn load_services(&self) -> PortResult {
            Ok(())
        }
        fn focus_window(&self) -> PortResult {
            Ok(())
        }
        fn fetch_icon(&self, _: Uuid, _: &str) -> PortResult {
            Ok(())
        }
        fn fetch_simple_icon(&self, _: Uuid, _: &str) -> PortResult {
            Ok(())
        }
        fn set_menus(&self, _: &MenuModel) -> PortResult {
            Ok(())
        }
        fn log(&self, message: &str) {
            self.logs.lock().unwrap().push(message.to_string());
        }
    }

    impl UiSink for Fake {
        fn emit_snapshot(&self, _: &Snapshot) -> PortResult {
            Ok(())
        }
    }

    impl Store for Fake {
        fn load(&self) -> PortResult<Persisted> {
            Ok(Persisted::default())
        }
        fn save(&self, _: &Persisted) -> PortResult {
            Ok(())
        }
    }

    fn open_in_browser(url: &str) -> (Vec<String>, Vec<String>) {
        let f = Fake {
            url: url.into(),
            opened: Mutex::default(),
            logs: Mutex::default(),
        };
        let ports = Ports {
            host: &f,
            notifier: &f,
            platform: &f,
            ui: &f,
            store: &f,
            read_only: false,
        };
        apply(
            Effect::OpenInBrowser {
                account: Uuid::nil(),
                browser: None,
            },
            &ports,
        );
        apply(
            Effect::OpenExternal {
                url: url.into(),
                browser: None,
            },
            &ports,
        );
        (f.opened.into_inner().unwrap(), f.logs.into_inner().unwrap())
    }

    #[test]
    fn unreadable_store_makes_the_run_read_only() {
        let error = PortError("read accounts.json: permission denied".into());
        let unreadable = StartupLoad::from_store(Err(error));
        assert!(unreadable.is_read_only());
        assert_eq!(unreadable.into_persisted(), Persisted::default());
        let loaded = StartupLoad::from_store(Ok(Persisted::default()));
        assert!(!loaded.is_read_only(), "a fresh start saves normally");
    }

    #[test]
    fn read_only_drops_only_persist_effects() {
        let persist = Effect::Persist(Persisted::default());
        assert!(skip_effect(&persist, true));
        assert!(!skip_effect(&persist, false));
        assert!(!skip_effect(&Effect::FocusWindow, true));
    }

    #[test]
    fn opener_gets_only_web_and_mail_urls() {
        let (opened, logs) = open_in_browser("https://app.slack.com/client/T1");
        assert_eq!(opened, vec!["https://app.slack.com/client/T1"; 2]);
        assert!(logs.is_empty());
        let (opened, logs) = open_in_browser("file:///etc/hosts");
        assert!(opened.is_empty());
        assert_eq!(logs.len(), 2);
        assert!(
            logs[0].starts_with("OpenInBrowser failed: not opening file:"),
            "{logs:?}"
        );
    }
}
