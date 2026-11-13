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

fn acc(n: u128, service: &str, label: &str) -> Account {
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
    let mut c = core_with(accounts);
    let fx = c.handle(Input::Started {
        width: 1000.0,
        height: 600.0,
    });
    (c, fx)
}

fn badges(fx: &[Effect]) -> Vec<DockBadge> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::SetDockBadge(b) = e {
                Some(*b)
            } else {
                None
            }
        })
        .collect()
}

fn notifies(fx: &[Effect]) -> Vec<&NotificationReq> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::Notify(n) = e {
                Some(n)
            } else {
                None
            }
        })
        .collect()
}

fn snap(fx: &[Effect]) -> Snapshot {
    fx.iter()
        .rev()
        .find_map(|e| {
            if let Effect::EmitSnapshot(s) = e {
                Some(s.clone())
            } else {
                None
            }
        })
        .expect("snapshot")
}

fn raw_notification(body: &str) -> String {
    format!(r#"{{"n":[{{"id":1,"title":"Alice","body":"{body}"}}],"u":null}}"#)
}

fn views(fx: &[Effect]) -> Vec<(&ViewSpec, bool)> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::CreateView { spec, visible, .. } = e {
                Some((spec, *visible))
            } else {
                None
            }
        })
        .collect()
}

fn update_account(c: &mut Core, n: u128, f: impl FnOnce(&mut AccountSettings)) -> Vec<Effect> {
    let mut settings = c.account(id(n)).unwrap().settings();
    f(&mut settings);
    c.handle(Input::Ui(UiCommand::UpdateAccount {
        account: id(n),
        settings,
    }))
}

// ---- feature batch 2 ----
fn has(fx: &[Effect], e: &Effect) -> bool {
    fx.contains(e)
}

fn destroys(fx: &[Effect]) -> Vec<Uuid> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::Destroy(a) = e {
                Some(*a)
            } else {
                None
            }
        })
        .collect()
}

fn polls(fx: &[Effect]) -> Vec<Uuid> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::Poll { account, .. } = e {
                Some(*account)
            } else {
                None
            }
        })
        .collect()
}

fn logs(fx: &[Effect]) -> Vec<&str> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::Log(m) = e {
                Some(m.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn menus(fx: &[Effect]) -> Vec<&MenuModel> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::UpdateMenus(m) = e {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}

/// Ticks every 10 s from `from` to `to` (inclusive), collecting all effects.
fn tick_range(c: &mut Core, from: u64, to: u64) -> Vec<Effect> {
    let mut fx = vec![];
    let mut t = from;
    while t <= to {
        fx.extend(c.handle(Input::Tick { now_ms: t }));
        t += 10_000;
    }
    fx
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

fn pol(link_target: LinkTarget, allow_media: bool) -> ViewPolicy {
    ViewPolicy {
        link_target,
        allow_media,
        app_hosts: vec![],
    }
}

fn policies(fx: &[Effect]) -> Vec<(Uuid, ViewPolicy)> {
    fx.iter()
        .filter_map(|e| {
            if let Effect::SetViewPolicy { account, policy } = e {
                Some((*account, policy.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn set_global_hibernate(c: &mut Core, m: Option<u32>) {
    let mut settings = c.settings().clone();
    settings.hibernate_after_min = m;
    c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
}

fn svg(tag: &str) -> String {
    format!("data:image/svg+xml;base64,{tag}")
}

fn fetched(c: &mut Core, n: u128, slug: &str, r: SimpleIconResult) -> Vec<Effect> {
    c.handle(Input::SimpleIconFetched {
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

fn created(fx: &[Effect]) -> Vec<Uuid> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::CreateView { spec, .. } => Some(spec.account),
            _ => None,
        })
        .collect()
}

fn two_user_accounts() -> Core {
    let mut c = core_with(vec![acc(1, "alpha", "A"), acc(2, "beta", "B")]);
    c.set_user_services(
        vec![
            user_def("alpha", "https://a.example/"),
            user_def("beta", "https://b.example/"),
        ],
        vec![],
    );
    let fx = c.handle(Input::Started {
        width: 1000.0,
        height: 600.0,
    });
    assert_eq!(created(&fx).len(), 2);
    c
}
