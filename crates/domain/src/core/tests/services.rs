use super::*;

#[test]
fn custom_service_crud() {
    let (mut c, _) = started(vec![]);
    let fx = c.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom(" My Chat ", "https://chat.example.org/app"),
    }));
    let s = snap(&fx);
    assert_eq!(s.custom_services.len(), 1);
    assert_eq!(
        (
            s.custom_services[0].id.as_str(),
            s.custom_services[0].name.as_str()
        ),
        ("custom-my-chat", "My Chat")
    );
    let info = s
        .services
        .iter()
        .find(|x| x.id == "custom-my-chat")
        .unwrap();
    assert!(info.custom && info.name == "My Chat" && info.color == "#123456");
    assert!(!s.services.iter().find(|x| x.id == "slack").unwrap().custom);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.custom_services.len() == 1))
    );
    let fx = c.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom("My chat", "https://b.org"),
    }));
    assert_eq!(snap(&fx).custom_services[1].id, "custom-my-chat-2");

    // invalid input: logged, nothing stored
    for bad in [custom(" ", "https://a.org"), custom("X", "mailto:a@b.c"), {
        let mut x = custom("X", "https://a.org");
        x.title_regex = Some("(".into());
        x
    }] {
        let fx = c.handle(Input::Ui(UiCommand::AddCustomService { service: bad }));
        assert!(
            matches!(fx.as_slice(), [Effect::Log(_), Effect::EmitSnapshot(_)]),
            "{fx:?}"
        );
    }
    assert_eq!(c.snapshot().custom_services.len(), 2);

    // an account of the custom service: title unread with the default regex
    let fx = c.handle(Input::Ui(UiCommand::AddAccount {
        service: "custom-my-chat".into(),
        label: "".into(),
        instance: None,
    }));
    let (spec, _) = views(&fx)[0];
    assert_eq!(spec.url.as_str(), "https://chat.example.org/app");
    assert_eq!(spec.internal_hosts, vec!["chat.example.org"]);
    assert_eq!(spec.user_agent.as_deref(), Some(crate::catalog::UA_SAFARI));
    let fx = c.handle(Input::TitleChanged {
        account: id(101),
        title: "(4) Chat".into(),
    });
    assert_eq!(snap(&fx).accounts[0].unread, 4);
    assert_eq!(snap(&fx).accounts[0].settings.label, "My Chat");

    // update recreates the views of its accounts
    let mut changed = c.snapshot().custom_services[0].clone();
    changed.url = "https://new.example.org/".into();
    let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed.clone(),
    }));
    assert_eq!(destroys(&fx), vec![id(101)]);
    assert_eq!(views(&fx)[0].0.url.as_str(), "https://new.example.org/");
    assert!(views(&fx)[0].1);
    changed.url = "nope".into();
    let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed.clone(),
    }));
    assert!(matches!(
        fx.as_slice(),
        [Effect::Log(_), Effect::EmitSnapshot(_)]
    ));
    changed.id = "custom-unknown".into();
    let fx = c.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed,
    }));
    assert!(matches!(
        fx.as_slice(),
        [Effect::Log(_), Effect::EmitSnapshot(_)]
    ));

    // remove: refused while used
    let fx = c.handle(Input::Ui(UiCommand::RemoveCustomService {
        id: "custom-my-chat".into(),
    }));
    assert!(logs(&fx)[0].contains("still used"));
    assert_eq!(snap(&fx).custom_services.len(), 2);
    c.handle(Input::Ui(UiCommand::RemoveAccount { account: id(101) }));
    let fx = c.handle(Input::Ui(UiCommand::RemoveCustomService {
        id: "custom-my-chat".into(),
    }));
    let s = snap(&fx);
    assert_eq!(
        s.custom_services
            .iter()
            .map(|x| x.id.as_str())
            .collect::<Vec<_>>(),
        vec!["custom-my-chat-2"]
    );
    assert!(s.services.iter().all(|x| x.id != "custom-my-chat"));
}

#[test]
fn custom_services_loaded_from_persisted() {
    let mut bad = custom("Bad", "nope");
    bad.id = "custom-bad".into();
    let mut good = custom("Good", "https://{instance}.good.io/");
    good.id = "custom-good".into();
    let mut a = acc(1, "custom-good", "G");
    a.instance = Some("kit".into());
    let persisted = Persisted {
        accounts: vec![a],
        custom_services: vec![bad, good],
        ..Default::default()
    };
    let mut c = Core::new(Catalog::builtin(), persisted, Box::new(|| id(5)));
    let fx = c.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert!(logs(&fx).iter().any(|l| l.contains("Bad")));
    assert_eq!(views(&fx)[0].0.url.as_str(), "https://kit.good.io/");
    assert_eq!(
        snap(&fx).accounts[0].instance_label.as_deref(),
        Some("Instance")
    );
}

#[test]
fn open_and_reload_commands_emit_effects() {
    let (mut c, _) = started(vec![]);
    assert_eq!(
        c.handle(Input::Ui(UiCommand::OpenServicesFolder))[0],
        Effect::OpenServicesFolder
    );
    assert_eq!(
        c.handle(Input::Ui(UiCommand::ReloadServices))[0],
        Effect::LoadServices
    );
}

#[test]
fn reload_replaces_definition_and_recreates_only_affected_account() {
    let mut c = two_user_accounts();
    let fx = c.handle(Input::ServicesLoaded {
        defs: vec![
            user_def("alpha", "https://a2.example/"),
            user_def("beta", "https://b.example/"),
        ],
        errors: vec![],
    });
    assert_eq!(created(&fx), vec![id(1)]);
    assert!(fx.contains(&Effect::Destroy(id(1))));
    assert!(!fx.contains(&Effect::Destroy(id(2))));
    // the unaffected account only gets its policy again
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::SetViewPolicy { account, .. } if *account == id(2)))
    );
    let s0 = snap(&fx);
    let alpha = s0.services.iter().find(|s| s.id == "alpha").unwrap();
    assert_eq!(alpha.url, "https://a2.example/");
    assert!(s0.service_errors.is_empty());
}

#[test]
fn reload_error_is_reported_and_keeps_previous_definition() {
    let mut c = two_user_accounts();
    let fx = c.handle(Input::ServicesLoaded {
        defs: vec![user_def("beta", "https://b.example/")],
        errors: vec!["invalid service definition: broken.toml".into()],
    });
    assert!(created(&fx).is_empty());
    assert!(fx.contains(&Effect::Log(
        "invalid service definition: broken.toml".into()
    )));
    let s0 = snap(&fx);
    assert_eq!(s0.service_errors.len(), 1);
    let alpha = s0.services.iter().find(|s| s.id == "alpha").unwrap();
    assert_eq!(alpha.url, "https://a.example/");
    // without errors, a missing file removes its definition
    let fx = c.handle(Input::ServicesLoaded {
        defs: vec![user_def("beta", "https://b.example/")],
        errors: vec![],
    });
    assert!(snap(&fx).services.iter().all(|s| s.id != "alpha"));
    assert!(snap(&fx).service_errors.is_empty());
}

#[test]
fn user_file_overrides_builtin_and_custom_services_win() {
    let (mut c, _) = started(vec![]);
    let fx = c.handle(Input::ServicesLoaded {
        defs: vec![user_def("slack", "https://my.slack.example/")],
        errors: vec![],
    });
    let slack = snap(&fx)
        .services
        .into_iter()
        .find(|s| s.id == "slack")
        .unwrap();
    assert_eq!(slack.url, "https://my.slack.example/");
    assert_eq!(slack.name, "X slack");
    assert_eq!(
        snap(&fx)
            .services
            .iter()
            .filter(|s| s.id == "slack")
            .count(),
        1
    );
}

#[test]
fn startup_user_services_errors_reach_snapshot() {
    let mut c = core_with(vec![]);
    c.set_user_services(vec![], vec!["bad file".into()]);
    assert_eq!(c.snapshot().service_errors, vec!["bad file".to_string()]);
}
