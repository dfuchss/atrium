//! The updater's long-running steps: checking and staging, preparing the swap and the
//! clean-up after the previous run.

use std::path::Path;

use protocol::UpdateState;

use super::{
    CheckDecision, HelperPlan, InstallLocation, Release, StageRequest, Updater, UpdaterAction,
    Version, decide, locked, native, new_update_path, now_ms, old_update_path, partial_update_path,
    ready_or, remove_if_present, stage,
};

impl Updater {
    /// Runs one check, unless one is already running.
    pub(crate) fn check(&self) {
        let Ok(_running) = self.shared.checking.try_lock() else {
            return;
        };
        let Some(current) = self.shared.config.current_version else {
            return;
        };
        self.set_state(ready_or(self.staged(), UpdateState::Checking));
        if let Err(message) = self.check_and_stage(current) {
            self.log(&format!("update check failed: {message}"));
            self.set_state(ready_or(self.staged(), UpdateState::Failed { message }));
        }
    }

    fn check_and_stage(&self, current: Version) -> Result<(), String> {
        let release = Release::parse(&self.shared.source.latest_release()?)?;
        self.change_settings(|settings| settings.last_check_ms = now_ms());
        if let Some(release) = &release {
            locked(&self.shared.page_url).clone_from(&release.page_url);
        }
        let decision = decide(
            current,
            release.as_ref(),
            &self.shared.config,
            self.staged(),
        );
        self.log(&format!(
            "latest release {}, running {current}: {decision:?}",
            release
                .as_ref()
                .map_or_else(|| "none".to_string(), |release| release.version.tag())
        ));
        let Some(release) = release else {
            self.set_state(ready_or(self.staged(), UpdateState::UpToDate));
            return Ok(());
        };
        let version = release.version.to_string();
        match decision {
            CheckDecision::UpToDate => {
                self.set_state(ready_or(self.staged(), UpdateState::UpToDate));
            }
            CheckDecision::ManualOnly { reason } => self.set_status(|status| {
                status.manual_reason = Some(native::manual_only_text(&reason));
                status.state = UpdateState::Available { version };
            }),
            CheckDecision::AlreadyStaged => self.set_state(UpdateState::Ready { version }),
            CheckDecision::Stage {
                install,
                asset_name,
            } => self.stage_release(&release, &install, &asset_name)?,
        }
        Ok(())
    }

    fn stage_release(
        &self,
        release: &Release,
        install: &Path,
        asset_name: &str,
    ) -> Result<(), String> {
        // A newer release supersedes an older staged one.
        self.set_staged(None);
        let version = release.version.to_string();
        let updates_dir = self.shared.files.updates_dir();
        let request = StageRequest {
            release,
            asset_name,
            install,
            updates_dir: &updates_dir,
        };
        let mut on_progress = |percent| {
            self.set_state(UpdateState::Downloading {
                version: version.clone(),
                percent,
            });
        };
        stage(self.shared.source.as_ref(), &request, &mut on_progress)?;
        self.set_staged(Some(release.version));
        self.log(&format!(
            "staged {} next to the installation",
            release.version.tag()
        ));
        self.set_state(UpdateState::Ready { version });
        Ok(())
    }

    /// Prepares the helper while the app still runs, so a problem keeps the app open.
    pub(crate) fn prepare_apply_update(&self) -> UpdaterAction {
        let Some(version) = self.staged() else {
            return UpdaterAction::None;
        };
        let InstallLocation::SelfUpdatable { bundle } = &self.shared.config.location else {
            return UpdaterAction::None;
        };
        if !new_update_path(bundle).exists() {
            self.set_staged(None);
            self.set_state(UpdateState::Failed {
                message: "the downloaded update is gone; check again".into(),
            });
            return UpdaterAction::None;
        }
        *locked(&self.shared.pending) = Some(HelperPlan {
            app_pid: std::process::id(),
            install: bundle.clone(),
            updates_dir: self.shared.files.updates_dir(),
            version: version.to_string(),
        });
        self.log(&format!("update to {version} requested"));
        UpdaterAction::Quit
    }

    /// Reports the previous swap, keeps a still useful staged update and removes the rest:
    /// download folders, `<install>.old-update` and an unowned `<install>.new-update`. Holds the
    /// check lock, so a "Check now" right after the start neither has its download removed nor
    /// its staged update taken for a leftover.
    pub(crate) fn clean_up_after_previous_run(&self) {
        let _checking = locked(&self.shared.checking);
        self.report_previous_swap();
        let updates_dir = self.shared.files.updates_dir();
        if let Ok(entries) = std::fs::read_dir(&updates_dir) {
            for entry in entries.filter_map(Result::ok) {
                let _ = remove_if_present(&entry.path());
            }
        }
        let InstallLocation::SelfUpdatable { bundle } = &self.shared.config.location else {
            self.set_staged(None);
            return;
        };
        let staged_version = locked(&self.shared.settings)
            .staged_version
            .as_deref()
            .and_then(Version::parse);
        let usable = staged_version.filter(|staged| {
            self.shared
                .config
                .current_version
                .is_some_and(|current| *staged > current)
                && new_update_path(bundle).exists()
        });
        self.set_staged(usable);
        for leftover in [old_update_path(bundle), partial_update_path(bundle)] {
            self.remove_leftover(&leftover);
        }
        match usable {
            Some(version) => self.set_state(UpdateState::Ready {
                version: version.to_string(),
            }),
            None => self.remove_leftover(&new_update_path(bundle)),
        }
    }

    fn remove_leftover(&self, path: &Path) {
        if let Err(error) = remove_if_present(path) {
            self.log(&error);
        }
    }

    fn report_previous_swap(&self) {
        let Some((result, helper_log)) = self.shared.files.take_swap_report() else {
            return;
        };
        for line in helper_log.lines() {
            self.log(line);
        }
        if let Some(result) = result.filter(|result| !result.success) {
            self.set_status(|status| {
                status.last_failure = Some(format!("{}: {}", result.version, result.message));
            });
        }
    }
}
