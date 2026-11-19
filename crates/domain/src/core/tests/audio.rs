use super::*;

fn audio_mutes(effects: &[Effect]) -> Vec<(Uuid, bool)> {
    effects_of!(effects, Effect::SetAudioMuted { account, muted } => (*account, *muted))
}

fn capture(core: &mut Core, n: u128, active: bool) -> Vec<Effect> {
    core.handle(Input::CaptureChanged {
        account: id(n),
        active,
    })
}

fn set_dnd(core: &mut Core, enabled: bool) -> Vec<Effect> {
    core.handle(Input::Ui(UiCommand::SetDnd { enabled }))
}

#[test]
fn created_views_are_muted_while_dnd_is_on() {
    let (_, effects) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    assert_eq!(audio_mutes(&effects), vec![(id(1), false), (id(2), false)]);
    let mut persisted = Persisted {
        accounts: vec![account(1, "slack", "A")],
        ..Persisted::default()
    };
    persisted.settings.app.dnd = true;
    let mut core = Core::new(
        Catalog::builtin(),
        persisted,
        Box::new(Uuid::nil),
        UA_PLATFORM.into(),
    );
    let effects = core.handle(STARTED);
    let create = effects
        .iter()
        .position(|effect| matches!(effect, Effect::CreateView { .. }))
        .expect("view created");
    assert_eq!(audio_mutes(&effects[create..]), vec![(id(1), true)]);
}

#[test]
fn dnd_mutes_and_unmutes_the_loaded_views() {
    let mut disabled = account(3, "slack", "C");
    disabled.settings.enabled = false;
    let (mut core, _) = started(vec![
        account(1, "slack", "A"),
        account(2, "slack", "B"),
        disabled,
    ]);
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let effects = set_dnd(&mut core, true);
    assert_eq!(audio_mutes(&effects), vec![(id(1), true)]);
    let effects = set_dnd(&mut core, true);
    assert!(audio_mutes(&effects).is_empty(), "unchanged -> nothing");
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(audio_mutes(&effects), vec![(id(2), true)], "woken muted");
    let effects = core.handle(Input::Menu(MenuAction::ToggleDnd));
    assert_eq!(audio_mutes(&effects), vec![(id(1), false), (id(2), false)]);
}

#[test]
fn dnd_changed_in_the_settings_mutes_the_views() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let update = |core: &mut Core, settings: AppSettings| {
        audio_mutes(&core.handle(Input::Ui(UiCommand::UpdateSettings { settings })))
    };
    let dnd = AppSettings {
        dnd: true,
        ..AppSettings::default()
    };
    assert_eq!(update(&mut core, dnd.clone()), vec![(id(1), true)]);
    let unchanged = AppSettings {
        browser: Some("Firefox".into()),
        ..dnd
    };
    assert!(update(&mut core, unchanged).is_empty());
    assert_eq!(
        update(&mut core, AppSettings::default()),
        vec![(id(1), false)]
    );
}

#[test]
fn a_capturing_view_keeps_its_audio_in_dnd() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    set_dnd(&mut core, true);
    let effects = capture(&mut core, 1, true);
    assert_eq!(audio_mutes(&effects), vec![(id(1), false)]);
    assert_eq!(logs(&effects).len(), 1);
    assert!(
        capture(&mut core, 1, true).is_empty(),
        "unchanged -> nothing"
    );
    let effects = set_dnd(&mut core, false);
    assert_eq!(audio_mutes(&effects), vec![(id(2), false)], "1 is unmuted");
    let effects = set_dnd(&mut core, true);
    assert_eq!(audio_mutes(&effects), vec![(id(2), true)]);
    let effects = capture(&mut core, 1, false);
    assert_eq!(audio_mutes(&effects), vec![(id(1), true)], "call over");
}

#[test]
fn capture_without_dnd_mutes_nothing() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = capture(&mut core, 1, true);
    assert!(audio_mutes(&effects).is_empty());
    let effects = set_dnd(&mut core, true);
    assert!(audio_mutes(&effects).is_empty(), "the call keeps its audio");
    let effects = capture(&mut core, 1, false);
    assert_eq!(audio_mutes(&effects), vec![(id(1), true)]);
}

#[test]
fn capture_is_forgotten_with_the_view() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    set_dnd(&mut core, true);
    capture(&mut core, 2, true);
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let effects = capture(&mut core, 2, true);
    assert!(effects.is_empty(), "no view -> ignored: {effects:?}");
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(audio_mutes(&effects), vec![(id(2), true)], "new view muted");
    capture(&mut core, 2, true);
    let effects = update_account(&mut core, 2, |settings| {
        settings.user_agent = Some("chrome".into());
    });
    assert_eq!(destroys(&effects), vec![id(2)]);
    assert_eq!(
        audio_mutes(&effects),
        vec![(id(2), true)],
        "recreated view starts without capture"
    );
}

fn poll(core: &mut Core, n: u128, raw: &str) -> Vec<Effect> {
    core.handle(Input::PollResult {
        account: id(n),
        raw: serde_json::Value::from(raw).to_string(),
    })
}

fn probing(accounts: Vec<Account>) -> (Core, Vec<Effect>) {
    let persisted = Persisted {
        accounts,
        ..Persisted::default()
    };
    let mut core = Core::new(
        Catalog::builtin(),
        persisted,
        Box::new(Uuid::nil),
        UA_PLATFORM.into(),
    );
    core.set_page_capture_detection(true);
    let effects = core.handle(STARTED);
    (core, effects)
}

#[test]
fn without_page_detection_there_is_no_probe_and_the_field_is_ignored() {
    let (mut core, effects) = started(vec![account(1, "slack", "A")]);
    assert!(
        !views(&effects)[0]
            .0
            .init_script
            .contains("__atriumCapturing")
    );
    set_dnd(&mut core, true);
    let effects = poll(&mut core, 1, r#"{"n":[],"u":null,"c":true}"#);
    assert!(audio_mutes(&effects).is_empty());
    assert!(core.audio_muted(id(1)));
}

#[test]
fn page_detection_injects_the_probe_and_follows_the_polled_value() {
    let (mut core, effects) = probing(vec![account(1, "slack", "A")]);
    assert!(
        views(&effects)[0]
            .0
            .init_script
            .contains(crate::CAPTURE_PROBE)
    );
    set_dnd(&mut core, true);
    let effects = poll(&mut core, 1, r#"{"n":[],"u":null,"c":true}"#);
    assert_eq!(audio_mutes(&effects), vec![(id(1), false)]);
    let effects = poll(&mut core, 1, r#"{"n":[],"u":null,"c":true}"#);
    assert!(audio_mutes(&effects).is_empty(), "unchanged -> nothing");
    let effects = poll(&mut core, 1, r#"{"n":[],"u":null,"c":false}"#);
    assert_eq!(audio_mutes(&effects), vec![(id(1), true)]);
}

#[test]
fn a_missing_or_malformed_polled_value_is_not_capturing() {
    let (mut core, _) = probing(vec![account(1, "slack", "A")]);
    set_dnd(&mut core, true);
    poll(&mut core, 1, r#"{"n":[],"u":null,"c":true}"#);
    for raw in [
        r#"{"n":[],"u":null}"#,
        r#"{"c":"yes"}"#,
        r#"{"c":null}"#,
        r#"{"c":1}"#,
    ] {
        let effects = poll(&mut core, 1, raw);
        assert_eq!(audio_mutes(&effects), vec![(id(1), true)], "{raw}");
        poll(&mut core, 1, r#"{"c":true}"#);
    }
}
