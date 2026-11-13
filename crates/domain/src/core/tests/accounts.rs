use super::*;

#[test]
fn started_creates_views() {
    let (c, fx) = started(vec![acc(1, "slack", "Slack"), acc(2, "discord", "D")]);
    let content = Bounds {
        x: SIDEBAR,
        y: 0.0,
        width: 1000.0 - SIDEBAR,
        height: 600.0,
    };
    assert_eq!(fx[0], Effect::ApplySystemSettings(AppSettings::default()));
    assert_eq!(
        fx.iter()
            .filter(|e| matches!(e, Effect::ApplySystemSettings(_)))
            .count(),
        1
    );
    assert!(matches!(fx[1], Effect::SetUiBounds(b) if b.width == SIDEBAR && b.height == 600.0));
    let views: Vec<_> = fx
        .iter()
        .filter_map(|e| {
            if let Effect::CreateView {
                spec,
                bounds,
                visible,
            } = e
            {
                Some((spec.account, *bounds, *visible))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(views, vec![(id(1), content, true), (id(2), content, false)]);
    assert_eq!(badges(&fx), vec![DockBadge::None]);
    assert_eq!(c.snapshot().active, Some(id(1)));
    assert!(matches!(fx[fx.len() - 2], Effect::EmitSnapshot(_)));
    assert!(matches!(fx.last(), Some(Effect::UpdateMenus(_))));
}

#[test]
fn missing_service_gets_no_view() {
    let (_, fx) = started(vec![acc(1, "nope", "x")]);
    assert!(fx.iter().any(|e| matches!(e, Effect::Log(_))));
    assert!(!fx.iter().any(|e| matches!(e, Effect::CreateView { .. })));
}

#[test]
fn add_then_select() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "mastodon".into(),
        label: "  ".into(),
        instance: Some(" https://fosstodon.org/ ".into()),
    }));
    assert_eq!(fx[0], Effect::Hide(id(1)));
    let (spec, visible) = views(&fx)[0];
    assert!(visible);
    assert_eq!(spec.account, id(101));
    assert_eq!(spec.url.as_str(), "https://fosstodon.org/");
    let s = snap(&fx);
    assert_eq!(s.active, Some(id(101)));
    assert_eq!(s.accounts[1].settings.label, "Mastodon");
    assert_eq!(s.accounts[1].instance_label.as_deref(), Some("Instance"));
    assert_eq!(
        s.accounts[1].settings.instance.as_deref(),
        Some("fosstodon.org")
    );
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert_eq!(fx[0], Effect::Hide(id(101)));
    assert!(fx.contains(&Effect::Show(id(1))));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.settings.active == Some(id(1))))
    );
}

#[test]
fn add_unknown_service_logs() {
    let (mut c, _) = started(vec![]);
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "zzz".into(),
        label: "a".into(),
        instance: None,
    }));
    assert!(matches!(fx[0], Effect::Log(_)));
    assert!(matches!(fx[1], Effect::EmitSnapshot(_)));
    assert_eq!(fx.len(), 2);
}

#[test]
fn notification_click() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack"), acc(2, "slack", "B")]);
    let fx = c.handle(Input::NotificationClicked {
        account: id(2),
        shim_id: 5,
    });
    let sel = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert!(fx.contains(&Effect::Hide(id(1))) && fx.contains(&Effect::Show(id(2))));
    assert!(!sel.is_empty());
    let n = fx.len();
    assert_eq!(fx[n - 2], Effect::FocusWindow);
    assert_eq!(
        fx[n - 1],
        Effect::Eval {
            account: id(2),
            script: "window.__msgr&&window.__msgr.click(5)".into()
        }
    );
}

#[test]
fn remove_account_picks_next() {
    let (mut c, _) = started(vec![
        acc(1, "slack", "A"),
        acc(2, "slack", "B"),
        acc(3, "slack", "C"),
    ]);
    c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(2) }));
    assert_eq!(fx[0], Effect::Destroy(id(2)));
    assert_eq!(
        fx[1],
        Effect::DeleteProfile {
            store_id: *id(2).as_bytes()
        }
    );
    assert!(fx.contains(&Effect::Show(id(3))));
    assert_eq!(snap(&fx).active, Some(id(3)));
    let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(3) }));
    assert_eq!(snap(&fx).active, Some(id(1)));
    let fx = c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(1) }));
    assert_eq!(snap(&fx).active, None);
}

#[test]
fn move_clamps() {
    let (mut c, _) = started(vec![
        acc(1, "slack", "A"),
        acc(2, "slack", "B"),
        acc(3, "slack", "C"),
    ]);
    let fx = c.handle(Input::Ui(UiCommand::Move {
        account: id(1),
        delta: 10,
    }));
    let order: Vec<_> = snap(&fx).accounts.iter().map(|a| a.id).collect();
    assert_eq!(order, vec![id(2), id(3), id(1)]);
    let fx = c.handle(Input::Ui(UiCommand::Move {
        account: id(1),
        delta: -10,
    }));
    assert_eq!(snap(&fx).accounts[0].id, id(1));
}

#[test]
fn add_from_open_dialog_keeps_dialog_and_active() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "discord".into(),
        label: "D".into(),
        instance: None,
    }));
    assert_eq!(views(&fx).len(), 1);
    assert!(
        !views(&fx)[0].1,
        "new view stays hidden while the dialog is open"
    );
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::SetUiBounds(_) | Effect::Show(_) | Effect::Hide(_)
    )));
    let s = snap(&fx);
    assert!(s.dialog_open);
    assert_eq!(s.active, Some(id(1)));
    assert_eq!(s.accounts.len(), 2);
}

#[test]
fn add_first_account_from_dialog_becomes_active_on_close() {
    let (mut c, _) = started(vec![]);
    c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "discord".into(),
        label: String::new(),
        instance: None,
    }));
    let new = snap(&fx).accounts[0].id;
    assert_eq!(snap(&fx).active, Some(new));
    let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(fx.contains(&Effect::Show(new)));
}

#[test]
fn element_clears_stale_session_lock_before_page_scripts() {
    let def = Catalog::builtin().get("element").cloned().unwrap();
    assert!(
        def.clear_local_storage
            .iter()
            .any(|k| k == "react_sdk_session_lock_ping"),
        "{def:?}"
    );
    let (_, fx) = started(vec![acc(1, "element", "E")]);
    let script = &views(&fx)[0].0.init_script;
    assert!(
        script.starts_with("(()=>{try{for(const k of [\"react_sdk_session_lock_ping\""),
        "{script:.120}"
    );
    assert!(script.contains(crate::NOTIFY_SHIM));
}

#[test]
fn initials_rules() {
    assert_eq!(initials("my work"), "MW");
    assert_eq!(initials("slack"), "Sl");
    assert_eq!(initials("a b c"), "AB");
}

#[test]
fn is_internal() {
    let spec = ViewSpec {
        account: id(1),
        url: url::Url::parse("https://a.com").unwrap(),
        store_id: [0; 16],
        user_agent: None,
        init_script: String::new(),
        internal_hosts: vec!["*.slack.com".into(), "x.org".into()],
    };
    let u = |s: &str| url::Url::parse(s).unwrap();
    assert!(spec.is_internal(&u("about:blank")));
    assert!(spec.is_internal(&u("blob:https://evil.com/1")));
    assert!(spec.is_internal(&u("https://app.slack.com/x")));
    assert!(spec.is_internal(&u("https://SLACK.com/")));
    assert!(!spec.is_internal(&u("https://evilslack.com/")));
    assert!(!spec.is_internal(&u("https://sub.x.org/")));
}

#[test]
fn disable_and_enable_account() {
    let (mut c, _) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
    c.handle(Input::TitleChanged {
        account: id(1),
        title: "(2) A".into(),
    });
    let fx = update_account(&mut c, 1, |s| s.enabled = false);
    assert_eq!(fx[0], Effect::Destroy(id(1)));
    assert!(has(&fx, &Effect::Show(id(2))));
    assert_eq!(badges(&fx), vec![DockBadge::None]);
    let s = snap(&fx);
    assert_eq!(s.active, Some(id(2)));
    assert!(
        s.accounts[0].hibernated && !s.accounts[0].settings.enabled && s.accounts[0].unread == 0
    );
    assert_eq!(
        menus(&fx)[0]
            .accounts
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        vec![id(2)]
    );
    assert_eq!(polls(&c.handle(Input::Tick { now_ms: 1 })), vec![id(2)]);
    // selecting a disabled account is refused
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert!(
        !logs(&fx).is_empty()
            && !fx
                .iter()
                .any(|e| matches!(e, Effect::Show(_) | Effect::CreateView { .. }))
    );
    assert_eq!(c.snapshot().active, Some(id(2)));

    let fx = update_account(&mut c, 1, |s| s.enabled = true);
    assert_eq!(
        views(&fx)
            .iter()
            .map(|(s, v)| (s.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(1), false)]
    );
    assert!(!snap(&fx).accounts[0].hibernated);
    assert_eq!(
        polls(&c.handle(Input::Tick { now_ms: 2 })),
        vec![id(1), id(2)]
    );
}

#[test]
fn disabled_accounts_at_start() {
    let mut a = acc(1, "slack", "A");
    a.enabled = false;
    let mut c = core_with(vec![a, acc(2, "slack", "B")]);
    c.persisted.settings.active = Some(id(1));
    c.active = Some(id(1));
    let fx = c.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert_eq!(
        views(&fx)
            .iter()
            .map(|(s, v)| (s.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(2), true)]
    );
    assert_eq!(snap(&fx).active, Some(id(2)));
    assert!(snap(&fx).accounts[0].hibernated);
}
