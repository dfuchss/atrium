use super::*;

#[test]
fn started_creates_views() {
    let (core, effects) = started(vec![
        account(1, "slack", "Slack"),
        account(2, "discord", "D"),
    ]);
    let content = Bounds {
        x: SIDEBAR,
        y: 0.0,
        width: 1000.0 - SIDEBAR,
        height: 600.0,
    };
    assert_eq!(
        effects[0],
        Effect::ApplySystemSettings(AppSettings::default())
    );
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::ApplySystemSettings(_)))
            .count(),
        1
    );
    assert!(
        matches!(effects[1], Effect::SetUiBounds(b) if b.width == SIDEBAR && b.height == 600.0)
    );
    let views: Vec<_> = effects
        .iter()
        .filter_map(|effect| {
            if let Effect::CreateView {
                spec,
                bounds,
                visible,
            } = effect
            {
                Some((spec.account, *bounds, *visible))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(views, vec![(id(1), content, true), (id(2), content, false)]);
    assert_eq!(badges(&effects), vec![DockBadge::None]);
    assert_eq!(core.snapshot().active, Some(id(1)));
    assert!(matches!(
        effects[effects.len() - 2],
        Effect::EmitSnapshot(_)
    ));
    assert!(matches!(effects.last(), Some(Effect::UpdateMenus(_))));
}

#[test]
fn missing_service_gets_no_view() {
    let (_, effects) = started(vec![account(1, "nope", "x")]);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Log(_)))
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::CreateView { .. }))
    );
}

#[test]
fn add_then_select() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "mastodon".into(),
        label: "  ".into(),
        instance: Some(" https://fosstodon.org/ ".into()),
    }));
    assert_eq!(effects[0], Effect::Hide(id(1)));
    let (spec, visible) = views(&effects)[0];
    assert!(visible);
    assert_eq!(spec.account, id(101));
    assert_eq!(spec.url.as_str(), "https://fosstodon.org/");
    let snapshot = last_snapshot(&effects);
    assert_eq!(snapshot.active, Some(id(101)));
    assert_eq!(snapshot.accounts[1].settings.label, "Mastodon");
    assert_eq!(
        snapshot.accounts[1].instance_label.as_deref(),
        Some("Instance")
    );
    assert_eq!(
        snapshot.accounts[1].settings.instance.as_deref(),
        Some("fosstodon.org")
    );
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert_eq!(effects[0], Effect::Hide(id(101)));
    assert!(effects.contains(&Effect::Show(id(1))));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.settings.active == Some(id(1))))
    );
}

#[test]
fn add_unknown_service_logs() {
    let (mut core, _) = started(vec![]);
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "zzz".into(),
        label: "a".into(),
        instance: None,
    }));
    assert!(matches!(effects[0], Effect::Log(_)));
    assert!(matches!(effects[1], Effect::EmitSnapshot(_)));
    assert_eq!(effects.len(), 2);
}

#[test]
fn notification_click() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack"), account(2, "slack", "B")]);
    let effects = core.handle(Input::NotificationClicked {
        account: id(2),
        shim_id: Some(5),
    });
    let sel = core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert!(effects.contains(&Effect::Hide(id(1))) && effects.contains(&Effect::Show(id(2))));
    assert!(!sel.is_empty());
    let n = effects.len();
    assert_eq!(effects[n - 2], Effect::FocusWindow);
    assert_eq!(
        effects[n - 1],
        Effect::Eval {
            account: id(2),
            script: "window.__msgr&&window.__msgr.click(5)".into()
        }
    );
}

#[test]
fn remove_account_picks_next() {
    let (mut core, _) = started(vec![
        account(1, "slack", "A"),
        account(2, "slack", "B"),
        account(3, "slack", "C"),
    ]);
    core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    let effects = core.handle(Input::Ui(UiCommand::RemoveAccount { account: id(2) }));
    assert_eq!(effects[0], Effect::Destroy(id(2)));
    assert_eq!(
        effects[1],
        Effect::DeleteProfile {
            store_id: *id(2).as_bytes()
        }
    );
    assert!(effects.contains(&Effect::Show(id(3))));
    assert_eq!(last_snapshot(&effects).active, Some(id(3)));
    let effects = core.handle(Input::Ui(UiCommand::RemoveAccount { account: id(3) }));
    assert_eq!(last_snapshot(&effects).active, Some(id(1)));
    let effects = core.handle(Input::Ui(UiCommand::RemoveAccount { account: id(1) }));
    assert_eq!(last_snapshot(&effects).active, None);
}

#[test]
fn move_clamps() {
    let (mut core, _) = started(vec![
        account(1, "slack", "A"),
        account(2, "slack", "B"),
        account(3, "slack", "C"),
    ]);
    let effects = core.handle(Input::Ui(UiCommand::Move {
        account: id(1),
        delta: 10,
    }));
    let order: Vec<_> = last_snapshot(&effects)
        .accounts
        .iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(order, vec![id(2), id(3), id(1)]);
    let effects = core.handle(Input::Ui(UiCommand::Move {
        account: id(1),
        delta: -10,
    }));
    assert_eq!(last_snapshot(&effects).accounts[0].id, id(1));
}

#[test]
fn add_from_open_dialog_keeps_dialog_and_active() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "discord".into(),
        label: "D".into(),
        instance: None,
    }));
    assert_eq!(views(&effects).len(), 1);
    assert!(
        !views(&effects)[0].1,
        "new view stays hidden while the dialog is open"
    );
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        Effect::SetUiBounds(_) | Effect::Show(_) | Effect::Hide(_)
    )));
    let snapshot = last_snapshot(&effects);
    assert!(snapshot.dialog_open);
    assert_eq!(snapshot.active, Some(id(1)));
    assert_eq!(snapshot.accounts.len(), 2);
}

#[test]
fn add_first_account_from_dialog_becomes_active_on_close() {
    let (mut core, _) = started(vec![]);
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "discord".into(),
        label: String::new(),
        instance: None,
    }));
    let new = last_snapshot(&effects).accounts[0].id;
    assert_eq!(last_snapshot(&effects).active, Some(new));
    let effects = core.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(effects.contains(&Effect::Show(new)));
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
    let (_, effects) = started(vec![account(1, "element", "E")]);
    let script = &views(&effects)[0].0.init_script;
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
    let parse = |text: &str| url::Url::parse(text).unwrap();
    assert!(spec.is_internal(&parse("about:blank")));
    assert!(spec.is_internal(&parse("blob:https://evil.com/1")));
    assert!(spec.is_internal(&parse("https://app.slack.com/x")));
    assert!(spec.is_internal(&parse("https://SLACK.com/")));
    assert!(!spec.is_internal(&parse("https://evilslack.com/")));
    assert!(!spec.is_internal(&parse("https://sub.x.org/")));
}

#[test]
fn disable_and_enable_account() {
    let (mut core, _) = started(vec![account(1, "discord", "A"), account(2, "discord", "B")]);
    core.handle(Input::TitleChanged {
        account: id(1),
        title: "(2) A".into(),
    });
    let effects = update_account(&mut core, 1, |settings| settings.enabled = false);
    assert_eq!(effects[0], Effect::Destroy(id(1)));
    assert!(has(&effects, &Effect::Show(id(2))));
    assert_eq!(badges(&effects), vec![DockBadge::None]);
    let snapshot = last_snapshot(&effects);
    assert_eq!(snapshot.active, Some(id(2)));
    assert!(
        snapshot.accounts[0].hibernated
            && !snapshot.accounts[0].settings.enabled
            && snapshot.accounts[0].unread == 0
    );
    assert_eq!(
        menus(&effects)[0]
            .accounts
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        vec![id(2)]
    );
    assert_eq!(polls(&core.handle(Input::Tick { now_ms: 1 })), vec![id(2)]);
    // selecting a disabled account is refused
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    assert!(
        !logs(&effects).is_empty()
            && !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Show(_) | Effect::CreateView { .. }))
    );
    assert_eq!(core.snapshot().active, Some(id(2)));

    let effects = update_account(&mut core, 1, |settings| settings.enabled = true);
    assert_eq!(
        views(&effects)
            .iter()
            .map(|(snapshot, v)| (snapshot.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(1), false)]
    );
    assert!(!last_snapshot(&effects).accounts[0].hibernated);
    assert_eq!(
        polls(&core.handle(Input::Tick { now_ms: 2 })),
        vec![id(1), id(2)]
    );
}

#[test]
fn disabled_accounts_at_start() {
    let mut a = account(1, "slack", "A");
    a.enabled = false;
    let mut core = core_with(vec![a, account(2, "slack", "B")]);
    core.persisted.settings.active = Some(id(1));
    core.active = Some(id(1));
    let effects = core.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert_eq!(
        views(&effects)
            .iter()
            .map(|(snapshot, v)| (snapshot.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(2), true)]
    );
    assert_eq!(last_snapshot(&effects).active, Some(id(2)));
    assert!(last_snapshot(&effects).accounts[0].hibernated);
}
