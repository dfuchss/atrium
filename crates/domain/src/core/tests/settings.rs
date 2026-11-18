use super::*;

#[test]
fn snapshot_carries_settings_app_info_and_account_settings() {
    let mut a = account(1, "mastodon", "Fedi");
    a.settings.instance = Some("fosstodon.org".into());
    a.settings.show_badge = false;
    let mut core = core_with(vec![a, account(2, "slack", "Slack")]);
    let info = AppInfo {
        os: OperatingSystem::Linux,
        version: "1.2.3".into(),
        config_dir: "/cfg".into(),
        services_dir: "/cfg/services".into(),
        log_file: "/log/m.log".into(),
    };
    core.set_app_info(info.clone());
    let snapshot = core.snapshot();
    assert_eq!(snapshot.app, info);
    assert_eq!(snapshot.settings, AppSettings::default());
    assert_eq!(
        snapshot.accounts[0].instance_label.as_deref(),
        Some("Instance")
    );
    assert_eq!(snapshot.accounts[1].instance_label, None);
    let expected = AccountSettings {
        label: "Fedi".into(),
        instance: Some("fosstodon.org".into()),
        muted: false,
        show_badge: false,
        badge_style: None,
        dock_badge: None,
        user_agent: None,
        enabled: true,
        link_target: None,
        custom_icon: None,
        simple_icon: None,
        zoom: 1.0,
        hibernate_after_min: None,
        start_asleep: false,
        after_wake: None,
        custom_css: None,
        allow_media: None,
        app_hosts: vec![],
    };
    assert_eq!(snapshot.accounts[0].settings, expected);
}

#[test]
fn set_muted_and_dnd_map_onto_settings() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let effects = core.handle(Input::Ui(UiCommand::SetMuted {
        account: id(1),
        muted: true,
    }));
    assert!(last_snapshot(&effects).accounts[0].settings.muted);
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].settings.muted)
        )
    );
    let effects = core.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
    assert!(last_snapshot(&effects).settings.dnd);
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::Persist(persisted) if persisted.settings.app.dnd)
        )
    );
}

#[test]
fn update_settings_persists_applies_and_emits() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    core.handle(Input::TitleChanged {
        account: id(1),
        title: "• Discord".into(),
    });
    let settings = AppSettings {
        launch_at_login: true,
        dock_badge: DockBadgeMode::CountOnly,
        browser: Some("  Firefox ".into()),
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: settings.clone(),
    }));
    let stored = AppSettings {
        browser: Some("Firefox".into()),
        ..settings
    };
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Persist(persisted) if persisted.settings.app == stored)
    ));
    assert!(effects.contains(&Effect::ApplySystemSettings(stored.clone())));
    assert_eq!(badges(&effects), vec![DockBadge::None]);
    assert_eq!(last_snapshot(&effects).settings, stored);
    let effects = core.handle(Input::ExternalLink {
        account: id(1),
        url: "https://x.org".into(),
    });
    assert_eq!(
        effects,
        vec![Effect::OpenExternal {
            url: "https://x.org".into(),
            browser: Some("Firefox".into())
        }]
    );
    let blank = AppSettings {
        browser: Some(" ".into()),
        ..AppSettings::default()
    };
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings: blank }));
    assert_eq!(core.snapshot().settings.browser, None);
}

#[test]
fn update_account_simple_fields_no_recreate() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.label = "  Work ".into();
        settings.muted = true;
    });
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].settings.label == "Work" && persisted.accounts[0].settings.muted)
    ));
    let snapshot = last_snapshot(&effects);
    assert_eq!(
        (
            snapshot.accounts[0].settings.label.as_str(),
            snapshot.accounts[0].initials.as_str()
        ),
        ("Work", "Wo")
    );
    let effects = update_account(&mut core, 1, |settings| settings.label = " ".into());
    assert_eq!(last_snapshot(&effects).accounts[0].settings.label, "Slack");
    let effects = core.handle(Input::Ui(UiCommand::UpdateAccount {
        account: id(9),
        settings: core.account(id(1)).unwrap().settings.clone(),
    }));
    assert!(matches!(effects.as_slice(), [Effect::Log(_)]));
}

#[test]
fn update_account_instance_recreates_view() {
    let mut a = account(1, "mastodon", "M");
    a.settings.instance = Some("mastodon.social".into());
    let (mut core, _) = started(vec![account(2, "slack", "S"), a]);
    // inactive account -> recreated hidden
    let effects = update_account(&mut core, 1, |settings| {
        settings.instance = Some(" https://fosstodon.org/ ".into());
    });
    assert_eq!(effects[0], Effect::Destroy(id(1)));
    let v = views(&effects);
    assert_eq!(v.len(), 1);
    assert_eq!(
        (v[0].0.url.as_str(), v[0].0.store_id, v[0].1),
        ("https://fosstodon.org/", *id(1).as_bytes(), false)
    );
    assert_eq!(v[0].0.internal_hosts, vec!["fosstodon.org".to_string()]);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[1].settings.instance.as_deref() == Some("fosstodon.org"))));
    // same instance again -> nothing recreated
    let effects = update_account(&mut core, 1, |settings| {
        settings.instance = Some("fosstodon.org/".into());
    });
    assert!(views(&effects).is_empty());
    // active account -> visible, unless the dialog is open
    core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    let effects = update_account(&mut core, 1, |settings| {
        settings.instance = Some("a.social".into());
    });
    assert!(views(&effects)[0].1);
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let effects = update_account(&mut core, 1, |settings| {
        settings.instance = Some("b.social".into());
    });
    assert!(!views(&effects)[0].1);
}

#[test]
fn update_account_invalid_instance_changes_nothing() {
    let mut a = account(1, "mastodon", "M");
    a.settings.instance = Some("mastodon.social".into());
    let (mut core, _) = started(vec![a]);
    let before = core.snapshot();
    let effects = update_account(&mut core, 1, |settings| {
        settings.label = "New".into();
        settings.instance = Some("bad host/x y".into());
    });
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::Log(_), Effect::EmitSnapshot(_)]
        ),
        "{effects:?}"
    );
    assert_eq!(core.snapshot(), before);
}

#[test]
fn user_agent_override() {
    let (mut core, _) = started(vec![account(1, "slack", "S")]);
    let service_ua = core
        .view_spec(core.account(id(1)).unwrap())
        .unwrap()
        .user_agent;
    let ua = |core: &mut Core, value: Option<&str>| {
        let effects = update_account(core, 1, |settings| {
            settings.user_agent = value.map(String::from);
        });
        assert_eq!(effects[0], Effect::Destroy(id(1)));
        views(&effects)[0].0.user_agent.clone()
    };
    assert_eq!(
        ua(&mut core, Some("chrome")),
        crate::catalog::resolve_user_agent_preset(Some("chrome"), UA_PLATFORM)
    );
    assert_eq!(ua(&mut core, Some("default")), None);
    assert_eq!(ua(&mut core, Some(" Foo/1 ")).as_deref(), Some("Foo/1"));
    assert_eq!(ua(&mut core, Some("  ")), service_ua);
    assert_eq!(core.snapshot().accounts[0].settings.user_agent, None);
}

#[test]
fn update_account_before_start_does_not_recreate() {
    let mut core = core_with(vec![account(1, "slack", "S")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.user_agent = Some("chrome".into());
    });
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    let effects = core.handle(STARTED);
    assert_eq!(
        views(&effects)[0].0.user_agent,
        crate::catalog::resolve_user_agent_preset(Some("chrome"), UA_PLATFORM)
    );
}

#[test]
fn custom_css_injected_and_recreates() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.custom_css = Some("body{content:\"</style>\\n\"}".into());
    });
    assert_eq!(destroys(&effects), vec![id(1)]);
    let script = &views(&effects)[0].0.init_script;
    assert!(script.starts_with(crate::NOTIFY_SHIM));
    assert!(
        script.contains(r#"const css="body{content:\"</style>\\n\"}";"#),
        "{script}"
    );
    let effects = update_account(&mut core, 1, |settings| settings.label = "B".into());
    assert!(destroys(&effects).is_empty(), "same CSS -> no recreate");
    let effects = update_account(&mut core, 1, |settings| {
        settings.custom_css = Some("  ".into());
    });
    assert_eq!(views(&effects)[0].0.init_script, crate::NOTIFY_SHIM);
    assert_eq!(core.account(id(1)).unwrap().settings.custom_css, None);
}

#[test]
fn settings_of_hibernated_account_apply_on_next_load() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let effects = update_account(&mut core, 2, |settings| {
        settings.user_agent = Some("chrome".into());
        settings.zoom = 1.5;
        settings.hibernate_after_min = Some(0);
    });
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        Effect::CreateView { .. } | Effect::SetZoom { .. } | Effect::Destroy(_)
    )));
    assert_eq!(
        core.account(id(2)).unwrap().settings.hibernate_after_min,
        Some(0)
    );
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        views(&effects)[0].0.user_agent,
        crate::catalog::resolve_user_agent_preset(Some("chrome"), UA_PLATFORM)
    );
    assert!(effects.contains(&Effect::SetZoom {
        account: id(2),
        zoom: 1.5
    }));
}

fn theme_effects(effects: &[Effect]) -> Vec<Theme> {
    effects_of!(effects, Effect::ApplyTheme(theme) => *theme)
}

#[test]
fn theme_defaults_to_auto_and_is_applied_at_start() {
    assert_eq!(AppSettings::default().theme, Theme::Auto);
    let (_, effects) = started(vec![]);
    assert_eq!(theme_effects(&effects), vec![Theme::Auto]);
}

#[test]
fn persisted_theme_is_applied_at_start() {
    let mut persisted = Persisted::default();
    persisted.settings.app.theme = Theme::Dark;
    let mut core = Core::new(
        Catalog::builtin(),
        persisted,
        Box::new(Uuid::nil),
        UA_PLATFORM.into(),
    );
    let effects = core.handle(STARTED);
    assert_eq!(theme_effects(&effects), vec![Theme::Dark]);
}

#[test]
fn changing_the_theme_applies_it_once_and_other_updates_do_not() {
    let (mut core, _) = started(vec![]);
    let dark = AppSettings {
        theme: Theme::Dark,
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: dark.clone(),
    }));
    assert_eq!(theme_effects(&effects), vec![Theme::Dark]);
    assert_eq!(last_snapshot(&effects).settings.theme, Theme::Dark);
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: AppSettings { dnd: true, ..dark },
    }));
    assert!(theme_effects(&effects).is_empty());
}
