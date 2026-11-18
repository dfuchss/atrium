use super::*;

#[test]
fn app_hosts_normalised_and_policy_updated_live() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.app_hosts = vec![
            " HTTPS://IdP.Example.org:443/login ".into(),
            "idp.example.org".into(),
            "*.microsoftonline.com".into(),
            "no good".into(),
        ];
    });
    let want = vec![
        "idp.example.org".to_string(),
        "*.microsoftonline.com".into(),
    ];
    let got = policies(&effects);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.app_hosts, want);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Log(m) if m.contains("no good")))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].settings.app_hosts == want))
    );
    assert_eq!(last_snapshot(&effects).accounts[0].settings.app_hosts, want);
    // same list again -> no policy effect
    let effects = update_account(&mut core, 1, |settings| settings.app_hosts = want.clone());
    assert!(policies(&effects).is_empty());
}

#[test]
fn view_policy_app_host_matching() {
    let policy = ViewPolicy {
        app_hosts: vec!["idp.example.org".into(), "*.ms.com".into()],
        ..policy_with(LinkTarget::SystemBrowser, false)
    };
    assert!(policy.is_app_host("idp.example.org"));
    assert!(policy.is_app_host("IDP.example.org"));
    assert!(!policy.is_app_host("x.idp.example.org"));
    assert!(policy.is_app_host("ms.com"));
    assert!(policy.is_app_host("login.ms.com"));
    assert!(!policy.is_app_host("notms.com"));
    assert!(!policy_with(LinkTarget::AppWindow, false).is_app_host("ms.com"));
}

#[test]
fn view_policy_emitted_on_create_and_change() {
    let mut b = account(2, "slack", "B");
    b.settings.link_target = Some(LinkTarget::SystemBrowser);
    let (mut core, effects) = started(vec![account(1, "slack", "A"), b]);
    let pos = |effects: &[Effect], effect: &Effect| effects.iter().position(|x| x == effect);
    let set1 = Effect::SetViewPolicy {
        account: id(1),
        policy: policy_with(LinkTarget::SystemBrowser, true),
    };
    let created = effects.iter().position(
        |effect| matches!(effect, Effect::CreateView { spec, .. } if spec.account == id(1)),
    );
    assert!(
        pos(&effects, &set1).unwrap() < created.unwrap(),
        "policy is set before the view exists"
    );
    assert!(effects.contains(&Effect::SetZoom {
        account: id(1),
        zoom: 1.0
    }));

    let settings = AppSettings {
        link_target: LinkTarget::AppWindow,
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: settings.clone(),
    }));
    assert_eq!(
        policies(&effects),
        vec![(id(1), policy_with(LinkTarget::AppWindow, true))]
    );
    // unchanged global -> nothing
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    assert!(policies(&effects).is_empty());

    // account override equal to the effective target -> nothing; different -> effect
    let effects = update_account(&mut core, 1, |settings| {
        settings.link_target = Some(LinkTarget::AppWindow);
    });
    assert!(policies(&effects).is_empty());
    let effects = update_account(&mut core, 2, |settings| settings.link_target = None);
    assert_eq!(
        policies(&effects),
        vec![(id(2), policy_with(LinkTarget::AppWindow, true))]
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert_eq!(core.account(id(2)).unwrap().settings.link_target, None);
}

#[test]
fn allow_media_from_service_and_override() {
    let (mut core, effects) = started(vec![account(1, "slack", "S"), account(2, "linkedin", "L")]);
    let initial = policies(&effects);
    assert_eq!(
        initial,
        vec![
            (id(1), policy_with(LinkTarget::SystemBrowser, true)),
            (id(2), policy_with(LinkTarget::SystemBrowser, false))
        ]
    );
    let snapshot = core.snapshot();
    let info = |id: &str| {
        snapshot
            .services
            .iter()
            .find(|x| x.id == id)
            .unwrap()
            .allow_media
    };
    assert!(info("slack") && info("rainbow") && !info("linkedin"));
    // override without recreating the view; persisted
    let effects = update_account(&mut core, 2, |settings| settings.allow_media = Some(true));
    assert_eq!(
        policies(&effects),
        vec![(id(2), policy_with(LinkTarget::SystemBrowser, true))]
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[1].settings.allow_media == Some(true)))
    );
    assert_eq!(
        last_snapshot(&effects).accounts[1].settings.allow_media,
        Some(true)
    );
    let effects = update_account(&mut core, 1, |settings| settings.allow_media = Some(false));
    assert_eq!(
        policies(&effects),
        vec![(id(1), policy_with(LinkTarget::SystemBrowser, false))]
    );
}

#[test]
fn allow_media_equal_to_default_and_while_hibernated() {
    let (mut core, _) = started(vec![account(1, "slack", "S"), account(2, "linkedin", "L")]);
    update_account(&mut core, 2, |settings| settings.allow_media = Some(true));
    // explicit value equal to the service default -> no change
    let effects = update_account(&mut core, 2, |settings| settings.allow_media = None);
    assert_eq!(
        policies(&effects),
        vec![(id(2), policy_with(LinkTarget::SystemBrowser, false))]
    );
    let effects = update_account(&mut core, 2, |settings| settings.allow_media = Some(false));
    assert!(policies(&effects).is_empty());
    // hibernated: applied on the next create
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert!(
        policies(&update_account(&mut core, 2, |settings| settings
            .allow_media =
            Some(true)))
        .is_empty()
    );
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        policies(&effects),
        vec![(id(2), policy_with(LinkTarget::SystemBrowser, true))]
    );
}

#[test]
fn custom_services_default_to_no_media() {
    let (mut core, _) = started(vec![]);
    core.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom("Site", "https://site.org"),
    }));
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "custom-site".into(),
        label: String::new(),
        instance: None,
    }));
    assert_eq!(
        policies(&effects),
        vec![(id(101), policy_with(LinkTarget::SystemBrowser, false))]
    );
    assert!(
        !last_snapshot(&effects)
            .services
            .iter()
            .find(|x| x.id == "custom-site")
            .unwrap()
            .allow_media
    );
}

#[test]
fn open_project_page_opens_the_repository() {
    let (mut core, _) = started(vec![]);
    assert_eq!(
        core.handle(Input::Ui(UiCommand::OpenProjectPage)),
        vec![Effect::OpenExternal {
            url: protocol::PROJECT_URL.into(),
            browser: None
        }]
    );
}

#[test]
fn external_links_only_web_and_mail() {
    let (mut core, _) = started(vec![account(1, "slack", "S")]);
    let open = |core: &mut Core, url: &str| {
        core.handle(Input::ExternalLink {
            account: id(1),
            url: url.into(),
        })
    };
    assert_eq!(
        open(&mut core, "mailto:a@b.org"),
        vec![Effect::OpenExternal {
            url: "mailto:a@b.org".into(),
            browser: None
        }]
    );
    core.handle(Input::Tick { now_ms: 10_000 });
    assert!(matches!(
        open(&mut core, "https://x.org").as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    for bad in [
        "file:///etc/hosts",
        "x-apple.systempreferences:com.apple.preference.security",
        "slack://channel",
        "javascript:1",
    ] {
        assert!(
            matches!(open(&mut core, bad).as_slice(), [Effect::Log(_)]),
            "{bad}"
        );
    }
}

#[test]
fn media_is_allowed_only_on_internal_hosts() {
    let spec = ViewSpec {
        account: id(1),
        url: url::Url::parse("https://app.slack.com/").unwrap(),
        store_id: [0; 16],
        user_agent: None,
        init_script: String::new(),
        internal_hosts: vec!["*.slack.com".into(), "kit.example".into()],
    };
    let page = |raw: &str| url::Url::parse(raw).unwrap();
    for internal in [
        "https://app.slack.com/client",
        "https://kit.example/a",
        "http://slack.com/",
    ] {
        assert!(
            spec.media_allowed_at(true, Some(&page(internal))),
            "{internal}"
        );
        assert!(
            !spec.media_allowed_at(false, Some(&page(internal))),
            "{internal}"
        );
    }
    for foreign in [
        "https://evil.example/",
        "https://slack.com.evil.example/",
        "about:blank",
        "data:text/html,x",
        "blob:https://app.slack.com/1",
    ] {
        assert!(
            !spec.media_allowed_at(true, Some(&page(foreign))),
            "{foreign}"
        );
    }
    assert!(!spec.media_allowed_at(true, None));
}

#[test]
fn external_links_are_throttled_per_account() {
    let (mut core, _) = started(vec![account(1, "slack", "S"), account(2, "slack", "T")]);
    let open = |core: &mut Core, n: u128| {
        core.handle(Input::ExternalLink {
            account: id(n),
            url: "https://x.org/a?secret=1".into(),
        })
    };
    core.handle(Input::Tick { now_ms: 10_000 });
    assert!(matches!(
        open(&mut core, 1).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    let dropped = open(&mut core, 1);
    assert!(
        matches!(dropped.as_slice(), [Effect::Log(line)] if line.contains("https://x.org/a:") && !line.contains("secret")),
        "{dropped:?}"
    );
    core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert!(matches!(
        open(&mut core, 2).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    core.handle(Input::Tick { now_ms: 12_000 });
    assert!(matches!(
        open(&mut core, 1).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
}

#[test]
fn web_links_open_only_for_the_visible_account() {
    let (mut core, _) = started(vec![account(1, "slack", "S"), account(2, "slack", "T")]);
    let link = |core: &mut Core, n: u128| {
        core.handle(Input::ExternalLink {
            account: id(n),
            url: "https://x.org/a?secret=1".into(),
        })
    };
    let refused = link(&mut core, 2);
    assert!(
        matches!(refused.as_slice(), [Effect::Log(message)] if message.contains("background") && !message.contains("secret")),
        "{refused:?}"
    );
    assert!(matches!(
        link(&mut core, 1).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    core.handle(Input::Tick { now_ms: 10_000 });
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    assert!(matches!(link(&mut core, 1).as_slice(), [Effect::Log(_)]));
    core.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert!(matches!(
        link(&mut core, 2).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
}

#[test]
fn mail_links_open_only_for_the_visible_account() {
    let (mut core, _) = started(vec![account(1, "slack", "S"), account(2, "slack", "T")]);
    let mail = |core: &mut Core, n: u128| {
        core.handle(Input::ExternalLink {
            account: id(n),
            url: "mailto:a@b.org".into(),
        })
    };
    assert!(
        matches!(mail(&mut core, 2).as_slice(), [Effect::Log(line)] if line.contains("mailto:") && !line.contains("a@b.org"))
    );
    assert!(matches!(
        mail(&mut core, 1).as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    core.handle(Input::Tick { now_ms: 10_000 });
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    assert!(matches!(mail(&mut core, 1).as_slice(), [Effect::Log(_)]));
}
