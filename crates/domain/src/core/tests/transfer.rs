use super::*;
use protocol::BadgeStyle;

fn exported(core: &mut Core) -> String {
    let effects = core.handle(Input::Ui(UiCommand::ExportSettings));
    effects_of!(&effects, Effect::ExportSettings { json } => json.clone())
        .pop()
        .expect("export")
}

fn source_core() -> Core {
    let (mut core, _) = started(vec![
        account(1, "slack", "Work"),
        account(2, "discord", "D"),
    ]);
    core.handle(Input::Ui(UiCommand::AddCustomService {
        service: CustomService {
            id: String::new(),
            name: "My chat".into(),
            url: "https://chat.example.org/".into(),
            color: "#123456".into(),
            instance_label: None,
            title_regex: None,
            user_agent: None,
        },
    }));
    core.handle(Input::Ui(UiCommand::AddAccount {
        service: "custom-my-chat".into(),
        label: "Chat".into(),
        instance: None,
    }));
    let mut settings = core.settings().clone();
    settings.dnd = true;
    settings.badge_style = BadgeStyle::Dot;
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    update_account(&mut core, 1, |settings| {
        settings.badge_style = Some(BadgeStyle::UrgentOnly);
        settings.muted = true;
    });
    core
}

#[test]
fn export_then_import_restores_settings_and_accounts() {
    let json = exported(&mut source_core());
    let (mut target, _) = started(vec![account(7, "mastodon", "Old")]);
    let effects = target.handle(Input::Ui(UiCommand::ImportSettings { json }));
    let snapshot = last_snapshot(&effects);
    assert!(snapshot.settings.dnd);
    assert_eq!(snapshot.settings.badge_style, BadgeStyle::Dot);
    let labels: Vec<_> = snapshot
        .accounts
        .iter()
        .map(|account| (account.service.as_str(), account.settings.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        [
            ("slack", "Work"),
            ("discord", "D"),
            ("custom-my-chat", "Chat")
        ]
    );
    assert_eq!(
        snapshot.accounts[0].settings.badge_style,
        Some(BadgeStyle::UrgentOnly)
    );
    assert!(snapshot.accounts[0].settings.muted);
    assert!(snapshot.accounts.iter().all(|account| account.id != id(7)));
    assert!(effects.contains(&Effect::DeleteProfile {
        store_id: *id(7).as_bytes()
    }));
    assert_eq!(snapshot.custom_services.len(), 1);
}

#[test]
fn unreadable_file_changes_nothing() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    for json in [
        "not json",
        r#"{"format":"other","version":1}"#,
        r#"{"format":"atrium-settings","version":99}"#,
    ] {
        let effects = core.handle(Input::Ui(UiCommand::ImportSettings { json: json.into() }));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DeleteProfile { .. } | Effect::Persist(_))),
            "{json}"
        );
        assert_eq!(last_snapshot(&effects).accounts.len(), 1);
    }
}

#[test]
fn accounts_of_unknown_services_are_skipped() {
    let json = r#"{"format":"atrium-settings","version":1,"accounts":[
        {"service":"no-such-service","label":"X"},
        {"service":"slack","label":"S","instance":"kit"}]}"#;
    let (mut core, _) = started(vec![]);
    let effects = core.handle(Input::Ui(UiCommand::ImportSettings { json: json.into() }));
    let snapshot = last_snapshot(&effects);
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(snapshot.accounts[0].settings.label, "S");
}

fn assert_unchanged(effects: &[Effect], core: &Core, json: &str) {
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::DeleteProfile { .. } | Effect::Persist(_))),
        "{json}"
    );
    assert_eq!(
        last_snapshot(effects).accounts.len(),
        core.persisted.accounts.len()
    );
}

#[test]
fn invalid_custom_service_or_account_changes_nothing() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    for json in [
        r##"{"format":"atrium-settings","version":1,"custom_services":[
            {"id":"","name":"Bad","url":"not a url","color":"#123456"}],
            "accounts":[{"service":"slack","label":"S"}]}"##,
        r##"{"format":"atrium-settings","version":1,"custom_services":[
            {"id":"","name":"Bad","url":"https://x.example.org/","color":"#123456","title_regex":"("}]}"##,
        r#"{"format":"atrium-settings","version":1,
            "accounts":[{"service":"slack","label":"S"},{"service":"mastodon","label":"M","instance":"a b"}]}"#,
    ] {
        let effects = core.handle(Input::Ui(UiCommand::ImportSettings { json: json.into() }));
        assert_unchanged(&effects, &core, json);
    }
    assert_eq!(core.persisted.custom_services.len(), 0);
}

#[test]
fn import_persists_and_emits_once_and_creates_disabled_accounts_without_view() {
    let json = r#"{"format":"atrium-settings","version":1,"accounts":[
        {"service":"slack","label":"A","enabled":true},
        {"service":"slack","label":"B","enabled":false},
        {"service":"discord","label":"C"}]}"#;
    let (mut core, _) = started(vec![account(7, "mastodon", "Old")]);
    let effects = core.handle(Input::Ui(UiCommand::ImportSettings { json: json.into() }));
    let count = |f: fn(&Effect) -> bool| effects.iter().filter(|e| f(e)).count();
    assert_eq!(count(|e| matches!(e, Effect::Persist(_))), 1);
    assert_eq!(count(|e| matches!(e, Effect::EmitSnapshot(_))), 1);
    assert_eq!(count(|e| matches!(e, Effect::CreateView { .. })), 2);
}
