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
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].app_hosts == want))
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
    b.link_target = Some(LinkTarget::SystemBrowser);
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
    assert!(has(
        &effects,
        &Effect::SetZoom {
            account: id(1),
            zoom: 1.0
        }
    ));

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
    assert_eq!(core.account(id(2)).unwrap().link_target, None);
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
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[1].allow_media == Some(true)))
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
