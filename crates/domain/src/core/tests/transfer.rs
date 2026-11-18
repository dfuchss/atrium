use super::*;

fn add_custom(core: &mut Core, name: &str, url: &str) {
    core.handle(Input::Ui(UiCommand::AddCustomService {
        service: custom(name, url),
    }));
}

#[test]
fn settings_import_updates_known_custom_services_and_adds_the_others() {
    let mut saved = Persisted::default();
    saved.settings.app.dnd = true;
    saved.custom_services = vec![
        CustomService {
            id: "custom-chat".into(),
            ..custom("Chat", "https://new.example.org/")
        },
        CustomService {
            id: "slack".into(),
            ..custom("Slack", "https://other.example.org/")
        },
    ];
    let (mut core, _) = started(vec![account(7, "discord", "D")]);
    add_custom(&mut core, "Chat", "https://chat.example.org/");
    add_custom(&mut core, "Mine", "https://mine.example.org/");

    let effects = core.handle(Input::ImportSettings(saved));

    let snapshot = last_snapshot(&effects);
    assert!(snapshot.settings.dnd);
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(snapshot.active, Some(id(7)));
    let custom: Vec<_> = snapshot
        .custom_services
        .iter()
        .map(|service| (service.id.as_str(), service.url.as_str()))
        .collect();
    assert_eq!(
        custom,
        [
            ("custom-chat", "https://new.example.org/"),
            ("custom-mine", "https://mine.example.org/"),
            ("custom-slack", "https://other.example.org/"),
        ]
    );
    assert_eq!(
        core.registry.get("slack").unwrap().url,
        Catalog::builtin().get("slack").unwrap().url
    );
}

#[test]
fn settings_import_saves_and_emits_once_and_skips_invalid_custom_services() {
    let saved = Persisted {
        custom_services: vec![
            custom("One", "https://one.example.org/"),
            custom("Two", "https://two.example.org/"),
            custom("Broken", "not a url"),
        ],
        ..Persisted::default()
    };
    let (mut core, _) = started(vec![]);

    let effects = core.handle(Input::ImportSettings(saved));

    let count = |matches: fn(&Effect) -> bool| effects.iter().filter(|e| matches(e)).count();
    assert_eq!(count(|effect| matches!(effect, Effect::Persist(_))), 1);
    assert_eq!(count(|effect| matches!(effect, Effect::EmitSnapshot(_))), 1);
    assert_eq!(last_snapshot(&effects).custom_services.len(), 2);
    assert!(logs(&effects).iter().any(|line| line.contains("Broken")));
}

#[test]
fn full_import_replaces_the_saved_state() {
    let mut saved = Persisted {
        accounts: vec![account(7, "discord", "D")],
        ..Persisted::default()
    };
    saved.settings.active = Some(id(7));
    let (mut core, _) = started(vec![]);

    let imported = core.handle(Input::ImportEverything(saved.clone()));
    let persisted = imported
        .iter()
        .position(|effect| matches!(effect, Effect::Persist(_)));
    let restarted = imported
        .iter()
        .position(|effect| *effect == Effect::Restart);
    assert!(persisted < restarted, "{imported:?}");
    assert!(restarted.is_some());
    let effects = core.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));

    saved.settings.app.dnd = true;
    assert!(effects.contains(&Effect::Persist(saved)));
}
