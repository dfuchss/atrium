//! Tests of the reducer, grouped by feature; shared fixtures live here.

use super::snapshot::initials;
use super::*;
use crate::catalog::ServiceDef;
use protocol::{
    AccountSettings, AfterWake, CustomService, DockBadgeMode, IconSize, LinkTarget,
    OperatingSystem, Snapshot, Theme,
};

/// The payloads of the effects that match `pattern`, in order:
/// `effects_of!(effects, Effect::Reload(account) => *account)`.
macro_rules! effects_of {
    ($effects:expr, $pattern:pat => $payload:expr) => {
        $effects
            .iter()
            .filter_map(|effect| match effect {
                $pattern => Some($payload),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
}

/// The host's part of the user agent presets in the tests.
const UA_PLATFORM: &str = "X11; Linux x86_64";

mod accounts;
mod hibernation;
mod icons;
mod layout;
mod menus;
mod services;
mod settings;
mod transfer;
mod unread;
mod view_policy;
mod wake;

/// Sidebar width at the default icon size.
const SIDEBAR: f64 = 56.0;
const WINDOW_WIDTH: f64 = 1000.0;
const WINDOW_HEIGHT: f64 = 600.0;
/// The first input, with the window size of all tests.
const STARTED: Input = Input::Started {
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn account(n: u128, service: &str, label: &str) -> Account {
    Account::new(id(n), service, label, None)
}

fn core_with(accounts: Vec<Account>) -> Core {
    let mut counter = 100u128;
    let gen_id = Box::new(move || {
        counter += 1;
        Uuid::from_u128(counter)
    });
    let persisted = Persisted {
        accounts,
        ..Default::default()
    };
    Core::new(Catalog::builtin(), persisted, gen_id, UA_PLATFORM.into())
}

fn started(accounts: Vec<Account>) -> (Core, Vec<Effect>) {
    let mut core = core_with(accounts);
    let effects = core.handle(STARTED);
    (core, effects)
}

fn badges(effects: &[Effect]) -> Vec<DockBadge> {
    effects_of!(effects, Effect::SetDockBadge(badge) => *badge)
}

fn notifies(effects: &[Effect]) -> Vec<&NotificationReq> {
    effects_of!(effects, Effect::Notify(notification) => notification)
}

fn last_snapshot(effects: &[Effect]) -> Snapshot {
    let snapshots = effects_of!(effects, Effect::EmitSnapshot(snapshot) => snapshot);
    snapshots.last().copied().expect("snapshot").clone()
}

fn raw_notification(body: &str) -> String {
    format!(r#"{{"n":[{{"id":1,"title":"Alice","body":"{body}"}}],"u":null}}"#)
}

fn views(effects: &[Effect]) -> Vec<(&ViewSpec, bool)> {
    effects_of!(effects, Effect::CreateView { spec, visible, .. } => (spec, *visible))
}

fn update_account(core: &mut Core, n: u128, f: impl FnOnce(&mut AccountSettings)) -> Vec<Effect> {
    let mut settings = core.account(id(n)).unwrap().settings.clone();
    f(&mut settings);
    core.handle(Input::Ui(UiCommand::UpdateAccount {
        account: id(n),
        settings,
    }))
}

fn destroys(effects: &[Effect]) -> Vec<Uuid> {
    effects_of!(effects, Effect::Destroy(account) => *account)
}

fn polls(effects: &[Effect]) -> Vec<Uuid> {
    effects_of!(effects, Effect::Poll { account, .. } => *account)
}

fn logs(effects: &[Effect]) -> Vec<&str> {
    effects_of!(effects, Effect::Log(line) => line.as_str())
}

fn menus(effects: &[Effect]) -> Vec<&MenuModel> {
    effects_of!(effects, Effect::UpdateMenus(menus) => menus)
}

/// Ticks every 10 s from `from` to `to` (inclusive), collecting all effects.
fn tick_range(core: &mut Core, from: u64, to: u64) -> Vec<Effect> {
    let mut effects = vec![];
    let mut t = from;
    while t <= to {
        effects.extend(core.handle(Input::Tick { now_ms: t }));
        t += 10_000;
    }
    effects
}

fn custom(name: &str, url: &str) -> CustomService {
    CustomService {
        id: String::new(),
        name: name.into(),
        url: url.into(),
        color: "#123456".into(),
        instance_label: None,
        title_regex: None,
        user_agent: None,
    }
}

fn policy_with(link_target: LinkTarget, allow_media: bool) -> ViewPolicy {
    ViewPolicy {
        link_target,
        allow_media,
        app_hosts: vec![],
    }
}

fn policies(effects: &[Effect]) -> Vec<(Uuid, ViewPolicy)> {
    effects_of!(effects, Effect::SetViewPolicy { account, policy } => (*account, policy.clone()))
}

fn set_global_hibernate(core: &mut Core, m: Option<u32>) {
    let mut settings = core.settings().clone();
    settings.hibernate_after_min = m;
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
}

fn svg(tag: &str) -> String {
    format!("data:image/svg+xml;base64,{tag}")
}

fn fetched(core: &mut Core, n: u128, slug: &str, r: SimpleIconResult) -> Vec<Effect> {
    core.handle(Input::SimpleIconFetched {
        account: id(n),
        slug: slug.into(),
        result: r,
    })
}

fn user_def(id: &str, url: &str) -> ServiceDef {
    Catalog::parse(
        "t.toml",
        &format!("id = \"{id}\"\nname = \"X {id}\"\ncolor = \"#111111\"\nurl = \"{url}\"\n"),
    )
    .unwrap()
}

fn created(effects: &[Effect]) -> Vec<Uuid> {
    effects_of!(effects, Effect::CreateView { spec, .. } => spec.account)
}

fn two_user_accounts() -> Core {
    let mut core = core_with(vec![account(1, "alpha", "A"), account(2, "beta", "B")]);
    core.set_user_services(
        vec![
            user_def("alpha", "https://a.example/"),
            user_def("beta", "https://b.example/"),
        ],
        vec![],
    );
    let effects = core.handle(STARTED);
    assert_eq!(created(&effects).len(), 2);
    core
}
