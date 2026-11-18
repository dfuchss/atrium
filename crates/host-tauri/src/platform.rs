//! OS integration: dock badge, browser, login item, services folder, icons, menus and log.

use std::sync::PoisonError;

use domain::ports::{Platform, PortError, PortResult};
use domain::{AccountMenu, DockBadge, MenuModel, SimpleIconResult, redact_url};
use protocol::{AppSettings, Theme};
use tauri::{Manager, Runtime};
use tauri_plugin_autostart::AutoLaunchManager;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::logger::spawn_logged;
use crate::{Input, TauriHost, bring_to_front, icons, menus, native, port_err, services};

fn describe_simple_icon(result: &SimpleIconResult) -> String {
    match result {
        SimpleIconResult::Found(data_url) => format!("{} bytes as data URL", data_url.len()),
        SimpleIconResult::NotFound => "not found".into(),
        SimpleIconResult::Failed(error) => format!("failed: {error}"),
    }
}

impl<R: Runtime> Platform for TauriHost<R> {
    fn set_dock_badge(&self, badge: DockBadge) -> PortResult {
        native::set_badge(&self.window()?, badge).map_err(port_err)
    }

    fn open_external(&self, url: &str, browser: Option<&str>) -> PortResult {
        let result = native::open_url_with(&self.app, url, browser, &self.logger);
        self.logger.log(&format!(
            "open external {} (browser {browser:?}) -> {result:?}",
            redact_url(url)
        ));
        result
    }

    fn apply_system_settings(&self, settings: &AppSettings) -> PortResult {
        if self.config_dir.is_some() {
            // Development instance: the login item belongs to the regular installation.
            return Ok(());
        }
        let autostart = self
            .app
            .try_state::<AutoLaunchManager>()
            .ok_or_else(|| PortError("autostart plugin not registered".into()))?;
        let enabled = autostart.is_enabled().map_err(port_err)?;
        if enabled != settings.launch_at_login {
            if settings.launch_at_login {
                autostart.enable().map_err(port_err)?;
            } else {
                autostart.disable().map_err(port_err)?;
            }
            self.logger.log(&format!(
                "launch at login: {enabled} -> {}",
                settings.launch_at_login
            ));
        }
        Ok(())
    }

    /// `AppHandle::set_theme` covers every window, the menus, the tray, the dialogs and the web
    /// views' `prefers-color-scheme` (macOS: `NSApp.appearance`; Linux: GTK's dark-theme
    /// preference; Windows: each window's dark mode and its webviews' preferred colour scheme).
    /// `Auto` follows the system: on Linux, tao follows the desktop's colour scheme from the XDG
    /// desktop portal while Atrium runs; on Windows, tao follows the app mode
    /// (`AppsUseLightTheme`) and Tauri hands each change to the window's webviews.
    fn apply_theme(&self, theme: Theme) -> PortResult {
        self.app.set_theme(crate::native_theme(theme));
        // A forced theme sends no system theme-changed event, so refresh the title bar color
        // ourselves; on the main thread after the theme message, `theme()` is up to date.
        let window = self.window()?;
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                let theme = target.theme().unwrap_or(tauri::Theme::Light);
                let _ = target.set_background_color(Some(crate::window_background(theme)));
            })
            .map_err(port_err)
    }

    fn open_services_folder(&self) -> PortResult {
        let dir = self.services_dir()?;
        services::prepare_services_dir(&dir).map_err(port_err)?;
        // Our own config folder: not subject to the URL allow-list.
        self.app
            .opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(port_err)
    }

    fn load_services(&self) -> PortResult {
        let (defs, errors) = services::read_user_services(&self.services_dir()?);
        (self.feed)(Input::ServicesLoaded { defs, errors });
        Ok(())
    }

    fn focus_window(&self) -> PortResult {
        bring_to_front(&self.window()?).map_err(port_err)
    }

    fn fetch_icon(&self, account: Uuid, url: &str) -> PortResult {
        let (feed, logger, url) = (self.feed.clone(), self.logger.clone(), url.to_string());
        spawn_logged("icon", &self.logger, move || {
            let data_url = match icons::download_icon(&url) {
                Ok(data_url) => {
                    logger.log(&format!(
                        "[{account}] icon {} ({} bytes as data URL)",
                        redact_url(&url),
                        data_url.len()
                    ));
                    Some(data_url)
                }
                Err(error) => {
                    logger.log(&format!(
                        "[{account}] icon {} failed: {error}",
                        redact_url(&url)
                    ));
                    None
                }
            };
            feed(Input::IconFetched { account, data_url });
        });
        Ok(())
    }

    fn fetch_simple_icon(&self, account: Uuid, slug: &str) -> PortResult {
        let (feed, logger, slug) = (self.feed.clone(), self.logger.clone(), slug.to_string());
        spawn_logged("simple-icon", &self.logger, move || {
            let result = icons::download_simple_icon(&slug);
            logger.log(&format!(
                "[{account}] simple icon {slug}: {}",
                describe_simple_icon(&result)
            ));
            feed(Input::SimpleIconFetched {
                account,
                slug,
                result,
            });
        });
        Ok(())
    }

    fn set_menus(&self, model: &MenuModel) -> PortResult {
        self.menus
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&self.app, &self.window_label, &self.feed, model)
            .map_err(port_err)
    }

    /// On the main thread without waiting: the popup blocks until it closes (macOS).
    fn show_account_menu(&self, menu: &AccountMenu) -> PortResult {
        let window = self.window()?;
        let (app, logger, menu) = (self.app.clone(), self.logger.clone(), menu.clone());
        self.app
            .run_on_main_thread(move || {
                if let Err(error) = menus::popup_account_menu(&app, &window, &menu) {
                    logger.log(&format!("[{}] account menu failed: {error}", menu.account));
                }
            })
            .map_err(port_err)
    }

    fn log(&self, message: &str) {
        self.logger.log(message);
    }
}
