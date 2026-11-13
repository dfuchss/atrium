//! OS integration: dock badge, browser, login item, services folder, icons, menus and log.

use std::sync::PoisonError;

use domain::core::SimpleIconResult;
use domain::ports::{Platform, PortError, PortResult};
use domain::{DockBadge, MenuModel};
use protocol::AppSettings;
use tauri::{Manager, Runtime};
use tauri_plugin_autostart::AutoLaunchManager;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::{Input, TauriHost, bring_to_front, icons, port_err, services};

/// Runs `job` on a new named thread.
fn spawn_named(name: &str, job: impl FnOnce() + Send + 'static) -> PortResult {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(job)
        .map(drop)
        .map_err(port_err)
}

fn describe_simple_icon(result: &SimpleIconResult) -> String {
    match result {
        SimpleIconResult::Found(data_url) => format!("{} bytes as data URL", data_url.len()),
        SimpleIconResult::NotFound => "not found".into(),
        SimpleIconResult::Failed(error) => format!("failed: {error}"),
    }
}

impl<R: Runtime> Platform for TauriHost<R> {
    fn set_dock_badge(&self, badge: DockBadge) -> PortResult {
        let window = self.window()?;
        match badge {
            DockBadge::Count(count) if count > 0 => window.set_badge_count(Some(i64::from(count))),
            DockBadge::Dot => window.set_badge_label(Some("•".to_string())),
            DockBadge::None | DockBadge::Count(_) => window.set_badge_label(None),
        }
        .map_err(port_err)
    }

    fn open_external(&self, url: &str, browser: Option<&str>) -> PortResult {
        // With a browser this runs `/usr/bin/open -a <browser> -- <url>` and waits for it.
        let result = self.app.opener().open_url(url, browser).map_err(port_err);
        self.logger.log(&format!(
            "open external {url} (browser {browser:?}) -> {result:?}"
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
        spawn_named("icon", move || {
            let data_url = match icons::download_icon(&url) {
                Ok(data_url) => {
                    logger.log(&format!(
                        "[{account}] icon {url} ({} bytes as data URL)",
                        data_url.len()
                    ));
                    Some(data_url)
                }
                Err(error) => {
                    logger.log(&format!("[{account}] icon {url} failed: {error}"));
                    None
                }
            };
            feed(Input::IconFetched { account, data_url });
        })
    }

    fn fetch_simple_icon(&self, account: Uuid, slug: &str) -> PortResult {
        let (feed, logger, slug) = (self.feed.clone(), self.logger.clone(), slug.to_string());
        spawn_named("simple-icon", move || {
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
        })
    }

    fn set_menus(&self, model: &MenuModel) -> PortResult {
        self.menus
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .apply(&self.app, model)
            .map_err(port_err)
    }

    fn log(&self, message: &str) {
        self.logger.log(message);
    }
}
