use super::*;

#[test]
fn snapshot_carries_settings_app_info_and_account_settings() {
    let mut a = acc(1, "mastodon", "Fedi");
    a.instance = Some("fosstodon.org".into());
    a.show_badge = false;
    let mut c = core_with(vec![a, acc(2, "slack", "Slack")]);
    let info = AppInfo {
        version: "1.2.3".into(),
        config_dir: "/cfg".into(),
        services_dir: "/cfg/services".into(),
        log_file: "/log/m.log".into(),
    };
    c.set_app_info(info.clone());
    let s = c.snapshot();
    assert_eq!(s.app, info);
    assert_eq!(s.settings, AppSettings::default());
    assert_eq!(s.accounts[0].instance_label.as_deref(), Some("Instance"));
    assert_eq!(s.accounts[1].instance_label, None);
    let expected = AccountSettings {
        label: "Fedi".into(),
        instance: Some("fosstodon.org".into()),
        muted: false,
        show_badge: false,
        in_dock_badge: true,
        user_agent: None,
        enabled: true,
        link_target: None,
        custom_icon: None,
        simple_icon: None,
        zoom: 1.0,
        hibernate_after_min: None,
        custom_css: None,
        allow_media: None,
        app_hosts: vec![],
    };
    assert_eq!(s.accounts[0].settings, expected);
}

#[test]
fn set_muted_and_dnd_map_onto_settings() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
    let fx = c.handle(Input::Ui(UiCommand::SetMuted {
        account: id(1),
        muted: true,
    }));
    assert!(snap(&fx).accounts[0].settings.muted);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].muted))
    );
    let fx = c.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
    assert!(snap(&fx).settings.dnd);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.settings.app.dnd))
    );
}

#[test]
fn update_settings_persists_applies_and_emits() {
    let (mut c, _) = started(vec![acc(1, "discord", "Discord")]);
    c.handle(Input::TitleChanged {
        account: id(1),
        title: "• Discord".into(),
    });
    let settings = AppSettings {
        launch_at_login: true,
        dock_badge: DockBadgeMode::CountOnly,
        browser: Some("  Firefox ".into()),
        ..AppSettings::default()
    };
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: settings.clone(),
    }));
    let stored = AppSettings {
        browser: Some("Firefox".into()),
        ..settings
    };
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.settings.app == stored))
    );
    assert!(fx.contains(&Effect::ApplySystemSettings(stored.clone())));
    assert_eq!(badges(&fx), vec![DockBadge::None]);
    assert_eq!(snap(&fx).settings, stored);
    let fx = c.handle(Input::ExternalLink {
        account: id(1),
        url: "https://x.org".into(),
    });
    assert_eq!(
        fx,
        vec![Effect::OpenExternal {
            url: "https://x.org".into(),
            browser: Some("Firefox".into())
        }]
    );
    let blank = AppSettings {
        browser: Some(" ".into()),
        ..AppSettings::default()
    };
    c.handle(Input::Ui(UiCommand::UpdateSettings { settings: blank }));
    assert_eq!(c.snapshot().settings.browser, None);
}

#[test]
fn update_account_simple_fields_no_recreate() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
    let fx = update_account(&mut c, 1, |s| {
        s.label = "  Work ".into();
        s.muted = true;
    });
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(fx.iter().any(
        |e| matches!(e, Effect::Persist(p) if p.accounts[0].label == "Work" && p.accounts[0].muted)
    ));
    let s = snap(&fx);
    assert_eq!(
        (
            s.accounts[0].settings.label.as_str(),
            s.accounts[0].initials.as_str()
        ),
        ("Work", "Wo")
    );
    let fx = update_account(&mut c, 1, |s| s.label = " ".into());
    assert_eq!(snap(&fx).accounts[0].settings.label, "Slack");
    let fx = c.handle(Input::Ui(UiCommand::UpdateAccount {
        account: id(9),
        settings: c.account(id(1)).unwrap().settings(),
    }));
    assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
}

#[test]
fn update_account_instance_recreates_view() {
    let mut a = acc(1, "mastodon", "M");
    a.instance = Some("mastodon.social".into());
    let (mut c, _) = started(vec![acc(2, "slack", "S"), a]);
    // inactive account -> recreated hidden
    let fx = update_account(&mut c, 1, |s| {
        s.instance = Some(" https://fosstodon.org/ ".into());
    });
    assert_eq!(fx[0], Effect::Destroy(id(1)));
    let v = views(&fx);
    assert_eq!(v.len(), 1);
    assert_eq!(
        (v[0].0.url.as_str(), v[0].0.store_id, v[0].1),
        ("https://fosstodon.org/", *id(1).as_bytes(), false)
    );
    assert_eq!(v[0].0.internal_hosts, vec!["fosstodon.org".to_string()]);
    assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[1].instance.as_deref() == Some("fosstodon.org"))));
    // same instance again -> nothing recreated
    let fx = update_account(&mut c, 1, |s| s.instance = Some("fosstodon.org/".into()));
    assert!(views(&fx).is_empty());
    // active account -> visible, unless the dialog is open
    c.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    let fx = update_account(&mut c, 1, |s| s.instance = Some("a.social".into()));
    assert!(views(&fx)[0].1);
    c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let fx = update_account(&mut c, 1, |s| s.instance = Some("b.social".into()));
    assert!(!views(&fx)[0].1);
}

#[test]
fn update_account_invalid_instance_changes_nothing() {
    let mut a = acc(1, "mastodon", "M");
    a.instance = Some("mastodon.social".into());
    let (mut c, _) = started(vec![a]);
    let before = c.snapshot();
    let fx = update_account(&mut c, 1, |s| {
        s.label = "New".into();
        s.instance = Some("bad host/x y".into());
    });
    assert!(
        matches!(fx.as_slice(), [Effect::Log(_), Effect::EmitSnapshot(_)]),
        "{fx:?}"
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn user_agent_override() {
    let (mut c, _) = started(vec![acc(1, "slack", "S")]);
    let service_ua = c.view_spec(c.account(id(1)).unwrap()).unwrap().user_agent;
    let ua = |c: &mut Core, value: Option<&str>| {
        let fx = update_account(c, 1, |s| s.user_agent = value.map(String::from));
        assert_eq!(fx[0], Effect::Destroy(id(1)));
        views(&fx)[0].0.user_agent.clone()
    };
    assert_eq!(
        ua(&mut c, Some("chrome")).as_deref(),
        Some(crate::catalog::UA_CHROME)
    );
    assert_eq!(ua(&mut c, Some("default")), None);
    assert_eq!(ua(&mut c, Some(" Foo/1 ")).as_deref(), Some("Foo/1"));
    assert_eq!(ua(&mut c, Some("  ")), service_ua);
    assert_eq!(c.snapshot().accounts[0].settings.user_agent, None);
}

#[test]
fn update_account_before_start_does_not_recreate() {
    let mut c = core_with(vec![acc(1, "slack", "S")]);
    let fx = update_account(&mut c, 1, |s| s.user_agent = Some("chrome".into()));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    let fx = c.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert_eq!(
        views(&fx)[0].0.user_agent.as_deref(),
        Some(crate::catalog::UA_CHROME)
    );
}

#[test]
fn custom_css_injected_and_recreates() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = update_account(&mut c, 1, |s| {
        s.custom_css = Some("body{content:\"</style>\\n\"}".into());
    });
    assert_eq!(destroys(&fx), vec![id(1)]);
    let script = &views(&fx)[0].0.init_script;
    assert!(script.starts_with(crate::NOTIFY_SHIM));
    assert!(
        script.contains(r#"const css="body{content:\"</style>\\n\"}";"#),
        "{script}"
    );
    let fx = update_account(&mut c, 1, |s| s.label = "B".into());
    assert!(destroys(&fx).is_empty(), "same CSS -> no recreate");
    let fx = update_account(&mut c, 1, |s| s.custom_css = Some("  ".into()));
    assert_eq!(views(&fx)[0].0.init_script, crate::NOTIFY_SHIM);
    assert_eq!(c.account(id(1)).unwrap().custom_css, None);
}

#[test]
fn settings_of_hibernated_account_apply_on_next_load() {
    let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
    c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let fx = update_account(&mut c, 2, |s| {
        s.user_agent = Some("chrome".into());
        s.zoom = 1.5;
        s.hibernate_after_min = Some(0);
    });
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::CreateView { .. } | Effect::SetZoom { .. } | Effect::Destroy(_)
    )));
    assert_eq!(c.account(id(2)).unwrap().hibernate_after_min, Some(0));
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        views(&fx)[0].0.user_agent.as_deref(),
        Some(crate::catalog::UA_CHROME)
    );
    assert!(has(
        &fx,
        &Effect::SetZoom {
            account: id(2),
            zoom: 1.5
        }
    ));
}
