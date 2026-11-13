//! Flow tests of the updater against a fake release source and a fake installation.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use sha2::{Digest as _, Sha256};

use super::*;
use crate::bundle::BUNDLE_IDENTIFIER;
use crate::bundle::tests::fake_bundle;
use crate::source::Download;

const ASSET: &str = "atrium-macos-arm64.zip";
const DOWNLOADS: &str = "https://github.com/dfuchss/atrium/releases/download/v0.2.0/";

struct FakeSource {
    release: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl ReleaseSource for FakeSource {
    fn latest_release(&self) -> Result<String, String> {
        Ok(self.release.clone())
    }

    fn open(&self, url: &str, _max_bytes: u64) -> Result<Download, String> {
        let bytes = self.files.get(url).ok_or("404")?.clone();
        Ok(Download {
            length: Some(bytes.len() as u64),
            reader: Box::new(Cursor::new(bytes)),
        })
    }
}

/// A fake installation in `<dir>/Applications`, a release zip and the data folder.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Applications")).unwrap();
        fake_bundle(&dir.path().join("Applications"), BUNDLE_IDENTIFIER, "0.1.0");
        Fixture { dir }
    }

    fn install(&self) -> PathBuf {
        self.dir.path().join("Applications/Atrium.app")
    }

    fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// The zip of an `Atrium.app` with the given identifier and version.
    fn release_zip(&self, identifier: &str, version: &str) -> Vec<u8> {
        let build = self.dir.path().join(format!("build-{version}"));
        let bundle = fake_bundle(&build, identifier, version);
        let zip = build.join(ASSET);
        let status = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&bundle)
            .arg(&zip)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read(zip).unwrap()
    }

    fn updater(&self, source: FakeSource, location: InstallLocation, current: &str) -> Updater {
        let config = UpdaterConfig {
            current_version: Version::parse(current),
            location,
            asset_name: Some(ASSET),
            data_dir: self.data_dir(),
        };
        Updater::new(config, Box::new(source), Arc::new(|_| {}), Arc::new(|_| {}))
    }

    fn self_updatable(&self) -> InstallLocation {
        InstallLocation::SelfUpdatable {
            bundle: self.install(),
        }
    }
}

fn release_json(tag: &str, assets: &[&str]) -> String {
    let assets: Vec<String> = assets
        .iter()
        .map(|name| format!(r#"{{"name":"{name}","browser_download_url":"{DOWNLOADS}{name}"}}"#))
        .collect();
    format!(
        r#"{{"tag_name":"{tag}","html_url":"https://github.com/dfuchss/atrium/releases/tag/{tag}","assets":[{}]}}"#,
        assets.join(",")
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    crate::staging::hex(&Sha256::digest(bytes))
}

/// A v0.2.0 release serving `zip` and (optionally) `sums`.
fn source(zip: Vec<u8>, sums: Option<String>) -> FakeSource {
    let mut files = BTreeMap::from([(format!("{DOWNLOADS}{ASSET}"), zip)]);
    let mut assets = vec![ASSET];
    if let Some(sums) = sums {
        files.insert(format!("{DOWNLOADS}SHA256SUMS"), sums.into_bytes());
        assets.push("SHA256SUMS");
    }
    FakeSource {
        release: release_json("v0.2.0", &assets),
        files,
    }
}

fn valid_source(fixture: &Fixture) -> FakeSource {
    let zip = fixture.release_zip(BUNDLE_IDENTIFIER, "0.2.0");
    // Upper case digits: the comparison ignores case.
    let sums = format!("{}  {ASSET}\n", sha256_hex(&zip).to_ascii_uppercase());
    source(zip, Some(sums))
}

fn ready(version: &str) -> UpdateState {
    UpdateState::Ready {
        version: version.into(),
    }
}

fn failure(updater: &Updater) -> String {
    match updater.status().state {
        UpdateState::Failed { message } => message,
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn stages_a_verified_release_next_to_the_install() {
    let fixture = Fixture::new();
    let updater = fixture.updater(valid_source(&fixture), fixture.self_updatable(), "0.1.0");
    updater.check();
    assert_eq!(updater.status().state, ready("0.2.0"));
    let staged = new_update_path(&fixture.install());
    crate::bundle::verify(&staged, Version::parse("0.2.0").unwrap()).unwrap();
    crate::bundle::verify(&fixture.install(), Version::parse("0.1.0").unwrap()).unwrap();
    let leftovers = std::fs::read_dir(fixture.data_dir().join("updates/v0.2.0")).unwrap();
    assert_eq!(leftovers.count(), 0);

    assert_eq!(
        updater.handle(&UpdateCommand::RestartToUpdate),
        UpdaterAction::Quit
    );
    let plan = locked(&updater.shared.pending).clone().unwrap();
    assert_eq!(plan.install, fixture.install());
    assert_eq!(plan.version, "0.2.0");
    assert_eq!(plan.app_pid, std::process::id());
    assert_eq!(
        updater.handle(&UpdateCommand::OpenReleasePage),
        UpdaterAction::OpenUrl("https://github.com/dfuchss/atrium/releases/tag/v0.2.0".into())
    );
}

#[test]
fn refuses_a_release_without_checksums() {
    let fixture = Fixture::new();
    let zip = fixture.release_zip(BUNDLE_IDENTIFIER, "0.2.0");
    let updater = fixture.updater(source(zip.clone(), None), fixture.self_updatable(), "0.1.0");
    updater.check();
    assert!(failure(&updater).contains("SHA256SUMS"));
    let other_entry = format!("{}  atrium-macos-x86_64.zip\n", sha256_hex(&zip));
    let updater = fixture.updater(
        source(zip, Some(other_entry)),
        fixture.self_updatable(),
        "0.1.0",
    );
    updater.check();
    assert!(failure(&updater).contains("no entry"));
    assert!(!new_update_path(&fixture.install()).exists());
}

#[test]
fn refuses_a_checksum_mismatch() {
    let fixture = Fixture::new();
    let zip = fixture.release_zip(BUNDLE_IDENTIFIER, "0.2.0");
    let sums = format!("{}  {ASSET}\n", sha256_hex(b"something else"));
    let updater = fixture.updater(source(zip, Some(sums)), fixture.self_updatable(), "0.1.0");
    updater.check();
    assert!(failure(&updater).contains("checksum mismatch"));
    assert!(!new_update_path(&fixture.install()).exists());
    assert!(
        !fixture
            .data_dir()
            .join("updates/v0.2.0")
            .join(ASSET)
            .exists()
    );
}

#[test]
fn refuses_a_bundle_of_another_app_or_version() {
    for (identifier, version, expected) in [
        ("org.example.other", "0.2.0", "identifier"),
        (BUNDLE_IDENTIFIER, "0.3.0", "version"),
    ] {
        let fixture = Fixture::new();
        let zip = fixture.release_zip(identifier, version);
        let sums = format!("{}  {ASSET}\n", sha256_hex(&zip));
        let updater = fixture.updater(source(zip, Some(sums)), fixture.self_updatable(), "0.1.0");
        updater.check();
        assert!(
            failure(&updater).contains(expected),
            "{identifier} {version}"
        );
        assert!(!new_update_path(&fixture.install()).exists());
    }
}

#[test]
fn never_offers_the_same_or_an_older_version() {
    for current in ["0.2.0", "0.3.0"] {
        let fixture = Fixture::new();
        let updater = fixture.updater(valid_source(&fixture), fixture.self_updatable(), current);
        updater.check();
        assert_eq!(updater.status().state, UpdateState::UpToDate);
        assert!(!new_update_path(&fixture.install()).exists());
    }
}

#[test]
fn manual_installs_only_announce() {
    let fixture = Fixture::new();
    let location = InstallLocation::ManualOnly {
        reason: "this is a development build".into(),
    };
    let updater = fixture.updater(valid_source(&fixture), location, "0.1.0");
    updater.check();
    let status = updater.status();
    assert_eq!(
        status.state,
        UpdateState::Available {
            version: "0.2.0".into()
        }
    );
    assert!(status.has_update());
    assert_eq!(
        status.manual_reason.as_deref(),
        Some("this is a development build")
    );
    assert_eq!(
        updater.handle(&UpdateCommand::RestartToUpdate),
        UpdaterAction::None
    );
    assert!(!new_update_path(&fixture.install()).exists());
}

#[test]
fn a_release_without_this_macs_asset_is_manual() {
    let fixture = Fixture::new();
    let config = UpdaterConfig {
        current_version: Version::parse("0.1.0"),
        location: fixture.self_updatable(),
        asset_name: Some("atrium-macos-x86_64.zip"),
        data_dir: fixture.data_dir(),
    };
    let release = Release::parse(&release_json("v0.2.0", &[ASSET])).unwrap();
    let decision = decide(
        Version::parse("0.1.0").unwrap(),
        release.as_ref(),
        &config,
        None,
    );
    assert!(
        matches!(decision, CheckDecision::ManualOnly { .. }),
        "{decision:?}"
    );
}

/// Writes the result the helper leaves for the next start.
fn write_swap_result(fixture: &Fixture, success: bool) {
    let updates = fixture.data_dir().join("updates");
    let result = crate::store::SwapResult {
        success,
        version: "0.2.0".into(),
        message: if success {
            "updated"
        } else {
            "cannot move the update in place"
        }
        .into(),
    };
    crate::store::write_atomically(
        &UpdaterFiles::result_file(&updates),
        &serde_json::to_string(&result).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_failed_swap_is_reported_and_the_update_offered_again() {
    let fixture = Fixture::new();
    fixture
        .updater(valid_source(&fixture), fixture.self_updatable(), "0.1.0")
        .check();
    write_swap_result(&fixture, false);
    std::fs::create_dir(old_update_path(&fixture.install())).unwrap();

    let restarted = fixture.updater(valid_source(&fixture), fixture.self_updatable(), "0.1.0");
    restarted.clean_up_after_previous_run();
    let status = restarted.status();
    assert_eq!(status.state, ready("0.2.0"));
    assert!(status.last_failure.unwrap().contains("cannot move"));
    assert!(new_update_path(&fixture.install()).is_dir());
    assert!(!old_update_path(&fixture.install()).exists());
    assert!(!fixture.data_dir().join("updates/v0.2.0").exists());
}

#[test]
fn after_a_successful_swap_leftovers_are_removed() {
    let fixture = Fixture::new();
    fixture
        .updater(valid_source(&fixture), fixture.self_updatable(), "0.1.0")
        .check();
    write_swap_result(&fixture, true);
    std::fs::create_dir(old_update_path(&fixture.install())).unwrap();

    let updated = fixture.updater(valid_source(&fixture), fixture.self_updatable(), "0.2.0");
    updated.clean_up_after_previous_run();
    let status = updated.status();
    assert_eq!(status.state, UpdateState::Idle);
    assert_eq!(status.last_failure, None);
    assert!(!new_update_path(&fixture.install()).exists());
    assert!(!old_update_path(&fixture.install()).exists());
    assert_eq!(
        UpdaterFiles::new(fixture.data_dir())
            .load_settings()
            .staged_version,
        None
    );
}

#[test]
fn automatic_checks_follow_the_setting() {
    let fixture = Fixture::new();
    let updater = fixture.updater(valid_source(&fixture), fixture.self_updatable(), "0.1.0");
    assert!(updater.status().auto_check);
    updater.handle(&UpdateCommand::SetAutoCheck { enabled: false });
    assert!(!updater.status().auto_check);
    updater.check_if_due(CHECK_INTERVAL_MS * 2);
    assert_eq!(updater.status().state, UpdateState::Idle);
    updater.handle(&UpdateCommand::SetAutoCheck { enabled: true });
    updater.check_if_due(CHECK_INTERVAL_MS * 2);
    assert_eq!(updater.status().state, ready("0.2.0"));
    let last_check = UpdaterFiles::new(fixture.data_dir())
        .load_settings()
        .last_check_ms;
    assert!(last_check > 0);
    let path = Path::new(&fixture.data_dir()).join("updater.json");
    assert!(path.exists());
}
