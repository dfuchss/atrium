use super::*;

/// Started core with the custom service "My Chat" (`custom-my-chat`).
fn core_with_custom_chat() -> (Core, Vec<Effect>) {
    let (mut core, _) = started(vec![]);
    let effects = core.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom(" My Chat ", "https://chat.example.org/app"),
    }));
    (core, effects)
}

/// `core_with_custom_chat` plus an account of it (id 101).
fn core_with_custom_chat_account() -> (Core, Vec<Effect>) {
    let (mut core, _) = core_with_custom_chat();
    let effects = core.handle(Input::Ui(UiCommand::AddAccount {
        service: "custom-my-chat".into(),
        label: String::new(),
        instance: None,
    }));
    (core, effects)
}

fn is_log_and_snapshot(effects: &[Effect]) -> bool {
    matches!(effects, [Effect::Log(_), Effect::EmitSnapshot(_)])
}

#[test]
fn custom_service_add_normalises_and_numbers_ids() {
    let (mut core, effects) = core_with_custom_chat();
    let snapshot = last_snapshot(&effects);
    assert_eq!(snapshot.custom_services.len(), 1);
    assert_eq!(
        (
            snapshot.custom_services[0].id.as_str(),
            snapshot.custom_services[0].name.as_str()
        ),
        ("custom-my-chat", "My Chat")
    );
    let info = snapshot
        .services
        .iter()
        .find(|x| x.id == "custom-my-chat")
        .unwrap();
    assert!(info.custom && info.name == "My Chat" && info.color == "#123456");
    assert!(
        !snapshot
            .services
            .iter()
            .find(|x| x.id == "slack")
            .unwrap()
            .custom
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.custom_services.len() == 1))
    );
    let effects = core.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom("My chat", "https://b.org"),
    }));
    assert_eq!(
        last_snapshot(&effects).custom_services[1].id,
        "custom-my-chat-2"
    );
}

#[test]
fn custom_service_invalid_input_is_logged_and_not_stored() {
    let (mut core, _) = core_with_custom_chat();
    let mut bad_regex = custom("X", "https://a.org");
    bad_regex.title_regex = Some("(".into());
    for bad in [
        custom(" ", "https://a.org"),
        custom("X", "mailto:a@b.c"),
        bad_regex,
    ] {
        let effects = core.handle(Input::Ui(UiCommand::AddCustomService { service: bad }));
        assert!(is_log_and_snapshot(&effects), "{effects:?}");
    }
    assert_eq!(core.snapshot().custom_services.len(), 1);
}

#[test]
fn custom_service_account_uses_its_definition() {
    let (mut core, effects) = core_with_custom_chat_account();
    let (spec, _) = views(&effects)[0];
    assert_eq!(spec.url.as_str(), "https://chat.example.org/app");
    assert_eq!(spec.internal_hosts, vec!["chat.example.org"]);
    assert_eq!(spec.user_agent.as_deref(), Some(crate::catalog::UA_WEBKIT));
    // title unread with the default regex
    let effects = core.handle(Input::TitleChanged {
        account: id(101),
        title: "(4) Chat".into(),
    });
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 4);
    assert_eq!(
        last_snapshot(&effects).accounts[0].settings.label,
        "My Chat"
    );
}

#[test]
fn custom_service_update_recreates_the_views_of_its_accounts() {
    let (mut core, _) = core_with_custom_chat_account();
    let mut changed = core.snapshot().custom_services[0].clone();
    changed.url = "https://new.example.org/".into();
    let effects = core.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed.clone(),
    }));
    assert_eq!(destroys(&effects), vec![id(101)]);
    assert_eq!(
        views(&effects)[0].0.url.as_str(),
        "https://new.example.org/"
    );
    assert!(views(&effects)[0].1);
    changed.url = "nope".into();
    let effects = core.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed.clone(),
    }));
    assert!(is_log_and_snapshot(&effects));
    changed.id = "custom-unknown".into();
    let effects = core.handle(Input::Ui(UiCommand::UpdateCustomService {
        service: changed,
    }));
    assert!(is_log_and_snapshot(&effects));
}

#[test]
fn custom_service_removal_is_refused_while_used() {
    let (mut core, _) = core_with_custom_chat_account();
    core.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom("My chat", "https://b.org"),
    }));
    let effects = core.handle(Input::Ui(UiCommand::RemoveCustomService {
        id: "custom-my-chat".into(),
    }));
    assert!(logs(&effects)[0].contains("still used"));
    assert_eq!(last_snapshot(&effects).custom_services.len(), 2);
    core.handle(Input::Ui(UiCommand::RemoveAccount { account: id(101) }));
    let effects = core.handle(Input::Ui(UiCommand::RemoveCustomService {
        id: "custom-my-chat".into(),
    }));
    let snapshot = last_snapshot(&effects);
    assert_eq!(
        snapshot
            .custom_services
            .iter()
            .map(|x| x.id.as_str())
            .collect::<Vec<_>>(),
        vec!["custom-my-chat-2"]
    );
    assert!(snapshot.services.iter().all(|x| x.id != "custom-my-chat"));
}

#[test]
fn custom_services_loaded_from_persisted() {
    let mut bad = custom("Bad", "nope");
    bad.id = "custom-bad".into();
    let mut good = custom("Good", "https://{instance}.good.io/");
    good.id = "custom-good".into();
    let mut a = account(1, "custom-good", "G");
    a.instance = Some("kit".into());
    let persisted = Persisted {
        accounts: vec![a],
        custom_services: vec![bad, good],
        ..Default::default()
    };
    let mut core = Core::new(Catalog::builtin(), persisted, Box::new(|| id(5)));
    let effects = core.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert!(logs(&effects).iter().any(|l| l.contains("Bad")));
    assert_eq!(views(&effects)[0].0.url.as_str(), "https://kit.good.io/");
    assert_eq!(
        last_snapshot(&effects).accounts[0]
            .instance_label
            .as_deref(),
        Some("Instance")
    );
}

#[test]
fn open_and_reload_commands_emit_effects() {
    let (mut core, _) = started(vec![]);
    assert_eq!(
        core.handle(Input::Ui(UiCommand::OpenServicesFolder))[0],
        Effect::OpenServicesFolder
    );
    assert_eq!(
        core.handle(Input::Ui(UiCommand::ReloadServices))[0],
        Effect::LoadServices
    );
}

#[test]
fn reload_replaces_definition_and_recreates_only_affected_account() {
    let mut core = two_user_accounts();
    let effects = core.handle(Input::ServicesLoaded {
        defs: vec![
            user_def("alpha", "https://a2.example/"),
            user_def("beta", "https://b.example/"),
        ],
        errors: vec![],
    });
    assert_eq!(created(&effects), vec![id(1)]);
    assert!(effects.contains(&Effect::Destroy(id(1))));
    assert!(!effects.contains(&Effect::Destroy(id(2))));
    // the unaffected account only gets its policy again
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::SetViewPolicy { account, .. } if *account == id(2))
    ));
    let initial = last_snapshot(&effects);
    let alpha = initial
        .services
        .iter()
        .find(|settings| settings.id == "alpha")
        .unwrap();
    assert_eq!(alpha.url, "https://a2.example/");
    assert!(initial.service_errors.is_empty());
}

#[test]
fn reload_error_is_reported_and_keeps_previous_definition() {
    let mut core = two_user_accounts();
    let effects = core.handle(Input::ServicesLoaded {
        defs: vec![user_def("beta", "https://b.example/")],
        errors: vec!["invalid service definition: broken.toml".into()],
    });
    assert!(created(&effects).is_empty());
    assert!(effects.contains(&Effect::Log(
        "invalid service definition: broken.toml".into()
    )));
    let initial = last_snapshot(&effects);
    assert_eq!(initial.service_errors.len(), 1);
    let alpha = initial
        .services
        .iter()
        .find(|settings| settings.id == "alpha")
        .unwrap();
    assert_eq!(alpha.url, "https://a.example/");
    // without errors, a missing file removes its definition
    let effects = core.handle(Input::ServicesLoaded {
        defs: vec![user_def("beta", "https://b.example/")],
        errors: vec![],
    });
    assert!(
        last_snapshot(&effects)
            .services
            .iter()
            .all(|settings| settings.id != "alpha")
    );
    assert!(last_snapshot(&effects).service_errors.is_empty());
}

#[test]
fn user_file_overrides_builtin_and_custom_services_win() {
    let (mut core, _) = started(vec![]);
    let effects = core.handle(Input::ServicesLoaded {
        defs: vec![user_def("slack", "https://my.slack.example/")],
        errors: vec![],
    });
    let slack = last_snapshot(&effects)
        .services
        .into_iter()
        .find(|settings| settings.id == "slack")
        .unwrap();
    assert_eq!(slack.url, "https://my.slack.example/");
    assert_eq!(slack.name, "X slack");
    assert_eq!(
        last_snapshot(&effects)
            .services
            .iter()
            .filter(|settings| settings.id == "slack")
            .count(),
        1
    );
}

#[test]
fn startup_user_services_errors_reach_snapshot() {
    let mut core = core_with(vec![]);
    core.set_user_services(vec![], vec!["bad file".into()]);
    assert_eq!(core.snapshot().service_errors, vec!["bad file".to_string()]);
}
