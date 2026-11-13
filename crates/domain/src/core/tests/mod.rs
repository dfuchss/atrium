//! Tests of the reducer, grouped by feature; shared fixtures live here.

use super::snapshot::initials;
use super::*;
use protocol::{AccountSettings, CustomService, DockBadgeMode, IconSize, LinkTarget, Snapshot};

mod accounts;
mod hibernation;
mod icons;
mod layout;
mod menus;
mod services;
mod settings;
mod unread;
mod view_policy;

/// Sidebar width at the default icon size.
const SIDEBAR: f64 = 56.0;

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
    Core::new(Catalog::builtin(), persisted, gen_id)
}

fn started(accounts: Vec<Account>) -> (Core, Vec<Effect>) {
    let mut core = core_with(accounts);
    let effects = core.handle(Input::Started {
        width: 1000.0,
        height: 600.0,
    });
    (core, effects)
}

fn badges(effects: &[Effect]) -> Vec<DockBadge> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::SetDockBadge(b) = effect {
                Some(*b)
            } else {
                None
            }
        })
        .collect()
}

fn notifies(effects: &[Effect]) -> Vec<&NotificationReq> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Notify(n) = effect {
                Some(n)
            } else {
                None
            }
        })
        .collect()
}

fn last_snapshot(effects: &[Effect]) -> Snapshot {
    effects
        .iter()
        .rev()
        .find_map(|effect| {
            if let Effect::EmitSnapshot(snapshot) = effect {
                Some(snapshot.clone())
            } else {
                None
            }
        })
        .expect("snapshot")
}

fn raw_notification(body: &str) -> String {
    format!(r#"{{"n":[{{"id":1,"title":"Alice","body":"{body}"}}],"u":null}}"#)
}

fn views(effects: &[Effect]) -> Vec<(&ViewSpec, bool)> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::CreateView { spec, visible, .. } = effect {
                Some((spec, *visible))
            } else {
                None
            }
        })
        .collect()
}

fn update_account(core: &mut Core, n: u128, f: impl FnOnce(&mut AccountSettings)) -> Vec<Effect> {
    let mut settings = core.account(id(n)).unwrap().settings();
    f(&mut settings);
    core.handle(Input::Ui(UiCommand::UpdateAccount {
        account: id(n),
        settings,
    }))
}

// ---- feature batch 2 ----
fn has(effects: &[Effect], effect: &Effect) -> bool {
    effects.contains(effect)
}

fn destroys(effects: &[Effect]) -> Vec<Uuid> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Destroy(a) = effect {
                Some(*a)
            } else {
                None
            }
        })
        .collect()
}

fn polls(effects: &[Effect]) -> Vec<Uuid> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Poll { account, .. } = effect {
                Some(*account)
            } else {
                None
            }
        })
        .collect()
}

fn logs(effects: &[Effect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Log(m) = effect {
                Some(m.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn menus(effects: &[Effect]) -> Vec<&MenuModel> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::UpdateMenus(m) = effect {
                Some(m)
            } else {
                None
            }
        })
        .collect()
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
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::SetViewPolicy { account, policy } = effect {
                Some((*account, policy.clone()))
            } else {
                None
            }
        })
        .collect()
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

// ---- expert mode: user service files ----
fn user_def(id: &str, url: &str) -> ServiceDef {
    Catalog::parse(
        "t.toml",
        &format!("id = \"{id}\"\nname = \"X {id}\"\ncolor = \"#111111\"\nurl = \"{url}\"\n"),
    )
    .unwrap()
}

fn created(effects: &[Effect]) -> Vec<Uuid> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::CreateView { spec, .. } => Some(spec.account),
            _ => None,
        })
        .collect()
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
    let effects = core.handle(Input::Started {
        width: 1000.0,
        height: 600.0,
    });
    assert_eq!(created(&effects).len(), 2);
    core
}
