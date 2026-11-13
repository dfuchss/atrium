use super::*;

#[test]
fn app_hosts_normalised_and_policy_updated_live() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = update_account(&mut c, 1, |s| {
        s.app_hosts = vec![
            " HTTPS://IdP.Example.org:443/login ".into(),
            "idp.example.org".into(),
            "*.microsoftonline.com".into(),
            "no good".into(),
        ]
    });
    let want = vec![
        "idp.example.org".to_string(),
        "*.microsoftonline.com".into(),
    ];
    let got = policies(&fx);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.app_hosts, want);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Log(m) if m.contains("no good")))
    );
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].app_hosts == want))
    );
    assert_eq!(snap(&fx).accounts[0].settings.app_hosts, want);
    // same list again -> no policy effect
    let fx = update_account(&mut c, 1, |s| s.app_hosts = want.clone());
    assert!(policies(&fx).is_empty());
}

#[test]
fn view_policy_app_host_matching() {
    let p = ViewPolicy {
        app_hosts: vec!["idp.example.org".into(), "*.ms.com".into()],
        ..pol(LinkTarget::SystemBrowser, false)
    };
    assert!(p.is_app_host("idp.example.org"));
    assert!(p.is_app_host("IDP.example.org"));
    assert!(!p.is_app_host("x.idp.example.org"));
    assert!(p.is_app_host("ms.com"));
    assert!(p.is_app_host("login.ms.com"));
    assert!(!p.is_app_host("notms.com"));
    assert!(!pol(LinkTarget::AppWindow, false).is_app_host("ms.com"));
}

#[test]
fn view_policy_emitted_on_create_and_change() {
    let mut b = acc(2, "slack", "B");
    b.link_target = Some(LinkTarget::SystemBrowser);
    let (mut c, fx) = started(vec![acc(1, "slack", "A"), b]);
    let pos = |fx: &[Effect], e: &Effect| fx.iter().position(|x| x == e);
    let set1 = Effect::SetViewPolicy {
        account: id(1),
        policy: pol(LinkTarget::SystemBrowser, true),
    };
    let created = fx
        .iter()
        .position(|e| matches!(e, Effect::CreateView { spec, .. } if spec.account == id(1)));
    assert!(
        pos(&fx, &set1).unwrap() < created.unwrap(),
        "policy is set before the view exists"
    );
    assert!(has(
        &fx,
        &Effect::SetZoom {
            account: id(1),
            zoom: 1.0
        }
    ));

    let settings = AppSettings {
        link_target: LinkTarget::AppWindow,
        ..AppSettings::default()
    };
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: settings.clone(),
    }));
    assert_eq!(
        policies(&fx),
        vec![(id(1), pol(LinkTarget::AppWindow, true))]
    );
    // unchanged global -> nothing
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    assert!(policies(&fx).is_empty());

    // account override equal to the effective target -> nothing; different -> effect
    let fx = update_account(&mut c, 1, |s| s.link_target = Some(LinkTarget::AppWindow));
    assert!(policies(&fx).is_empty());
    let fx = update_account(&mut c, 2, |s| s.link_target = None);
    assert_eq!(
        policies(&fx),
        vec![(id(2), pol(LinkTarget::AppWindow, true))]
    );
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert_eq!(c.account(id(2)).unwrap().link_target, None);
}

#[test]
fn allow_media_from_service_and_override() {
    let (mut c, fx) = started(vec![acc(1, "slack", "S"), acc(2, "linkedin", "L")]);
    let p = policies(&fx);
    assert_eq!(
        p,
        vec![
            (id(1), pol(LinkTarget::SystemBrowser, true)),
            (id(2), pol(LinkTarget::SystemBrowser, false))
        ]
    );
    let s = c.snapshot();
    let info = |id: &str| s.services.iter().find(|x| x.id == id).unwrap().allow_media;
    assert!(info("slack") && info("rainbow") && !info("linkedin"));
    // override without recreating the view; persisted
    let fx = update_account(&mut c, 2, |s| s.allow_media = Some(true));
    assert_eq!(
        policies(&fx),
        vec![(id(2), pol(LinkTarget::SystemBrowser, true))]
    );
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Destroy(_) | Effect::CreateView { .. }))
    );
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.accounts[1].allow_media == Some(true)))
    );
    assert_eq!(snap(&fx).accounts[1].settings.allow_media, Some(true));
    let fx = update_account(&mut c, 1, |s| s.allow_media = Some(false));
    assert_eq!(
        policies(&fx),
        vec![(id(1), pol(LinkTarget::SystemBrowser, false))]
    );
    // explicit value equal to the service default -> no change
    let fx = update_account(&mut c, 2, |s| s.allow_media = None);
    assert_eq!(
        policies(&fx),
        vec![(id(2), pol(LinkTarget::SystemBrowser, false))]
    );
    let fx = update_account(&mut c, 2, |s| s.allow_media = Some(false));
    assert!(policies(&fx).is_empty());
    // hibernated: applied on the next create
    c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert!(policies(&update_account(&mut c, 2, |s| s.allow_media = Some(true))).is_empty());
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        policies(&fx),
        vec![(id(2), pol(LinkTarget::SystemBrowser, true))]
    );
    // custom services default to no media
    c.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom("Site", "https://site.org"),
    }));
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "custom-site".into(),
        label: "".into(),
        instance: None,
    }));
    assert_eq!(
        policies(&fx),
        vec![(id(101), pol(LinkTarget::SystemBrowser, false))]
    );
    assert!(
        !snap(&fx)
            .services
            .iter()
            .find(|x| x.id == "custom-site")
            .unwrap()
            .allow_media
    );
}

#[test]
fn open_project_page_opens_the_repository() {
    let (mut c, _) = started(vec![]);
    assert_eq!(
        c.handle(Input::Ui(UiCommand::OpenProjectPage)),
        vec![Effect::OpenExternal {
            url: protocol::PROJECT_URL.into(),
            browser: None
        }]
    );
}

#[test]
fn external_links_only_web_and_mail() {
    let (mut c, _) = started(vec![acc(1, "slack", "S")]);
    let open = |c: &mut Core, url: &str| {
        c.handle(Input::ExternalLink {
            account: id(1),
            url: url.into(),
        })
    };
    assert_eq!(
        open(&mut c, "mailto:a@b.org"),
        vec![Effect::OpenExternal {
            url: "mailto:a@b.org".into(),
            browser: None
        }]
    );
    assert!(matches!(
        open(&mut c, "https://x.org").as_slice(),
        [Effect::OpenExternal { .. }]
    ));
    for bad in [
        "file:///etc/hosts",
        "x-apple.systempreferences:com.apple.preference.security",
        "slack://channel",
        "javascript:1",
    ] {
        assert!(
            matches!(open(&mut c, bad).as_slice(), [Effect::Log(_)]),
            "{bad}"
        );
    }
}
