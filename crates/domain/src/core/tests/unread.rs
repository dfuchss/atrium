use super::*;

#[test]
fn title_unread_and_dock_badge() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let t = |core: &mut Core, title: &str| {
        core.handle(Input::TitleChanged {
            account: id(1),
            title: title.into(),
        })
    };
    let effects = t(&mut core, "(3) Discord | x");
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 3);
    assert_eq!(badges(&effects), vec![DockBadge::Count(3)]);
    assert!(t(&mut core, "(3) Discord | x").is_empty());
    let effects = t(&mut core, "• Discord | x");
    assert_eq!(badges(&effects), vec![DockBadge::Dot]);
    assert!(last_snapshot(&effects).accounts[0].indirect);
    let effects = t(&mut core, "Discord");
    assert_eq!(badges(&effects), vec![DockBadge::None]);
}

#[test]
fn muted_not_in_badge() {
    let mut a = account(1, "discord", "Discord");
    a.muted = true;
    let (mut core, _) = started(vec![a]);
    let effects = core.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) D".into(),
    });
    assert!(badges(&effects).is_empty());
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 3);
}

#[test]
fn poll_notifies_with_prefix() {
    let (mut core, _) = started(vec![
        account(1, "slack", "KIT"),
        account(2, "slack", "Slack"),
    ]);
    let inner = r#"{"n":[{"id":7,"title":"Alice","body":"hi"}],"u":null}"#;
    let raw = serde_json::to_string(inner).unwrap();
    let effects = core.handle(Input::PollResult {
        account: id(1),
        raw,
    });
    let n = notifies(&effects);
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0].shim_id, n[0].title.as_str(), n[0].body.as_str()),
        (Some(7), "Slack (KIT): Alice", "hi")
    );
    let effects = core.handle(Input::PollResult {
        account: id(2),
        raw: inner.into(),
    });
    assert_eq!(notifies(&effects)[0].title, "Slack: Alice");
}

#[test]
fn poll_muted_or_dnd_silent() {
    let mut a = account(1, "slack", "Slack");
    a.muted = true;
    let (mut core, _) = started(vec![a, account(2, "slack", "Slack")]);
    let raw = r#"{"n":[{"id":1,"title":"x"}],"u":null}"#.to_string();
    assert!(
        notifies(&core.handle(Input::PollResult {
            account: id(1),
            raw: raw.clone()
        }))
        .is_empty()
    );
    assert_eq!(
        notifies(&core.handle(Input::PollResult {
            account: id(2),
            raw: raw.clone()
        }))
        .len(),
        1
    );
    core.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
    assert!(
        notifies(&core.handle(Input::PollResult {
            account: id(2),
            raw
        }))
        .is_empty()
    );
}

/// Started core with one account (id 1) of a service with poll `1+1` and title pattern `(n)`.
fn core_with_polled_service() -> Core {
    let mut def = Catalog::parse("p", "id='p'\nname='P'\ncolor='#fff'\nurl='https://p.io'\n[unread]\npoll='1+1'\ntitle_regex='^\\((\\d+)\\)'").unwrap();
    def.id = "p".into();
    let mut cat = Catalog::builtin();
    cat.insert(def);
    let persisted = Persisted {
        accounts: vec![account(1, "p", "P")],
        ..Default::default()
    };
    let mut core = Core::new(cat, persisted, Box::new(|| Uuid::from_u128(999)));
    core.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    core
}

fn polled(core: &mut Core, raw: &str) -> Vec<Effect> {
    core.handle(Input::PollResult {
        account: id(1),
        raw: raw.into(),
    })
}

fn titled(core: &mut Core, title: &str) -> Vec<Effect> {
    core.handle(Input::TitleChanged {
        account: id(1),
        title: title.into(),
    })
}

#[test]
fn poll_value_wins_over_the_title() {
    let mut core = core_with_polled_service();
    let effects = polled(&mut core, r#"{"n":null,"u":4}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Count(4)]);
    assert!(titled(&mut core, "(9) P").is_empty());
    let effects = polled(&mut core, r#"{"n":[],"u":{"direct":0,"indirect":true}}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Dot]);
}

#[test]
fn title_applies_while_the_poll_yields_nothing() {
    let mut core = core_with_polled_service();
    polled(&mut core, r#"{"n":null,"u":4}"#);
    titled(&mut core, "(9) P");
    // no poll value: the last title value (9) takes over
    let effects = polled(&mut core, r#"{"n":[],"u":null}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Count(9)]);
    let effects = titled(&mut core, "(2) P");
    assert_eq!(badges(&effects), vec![DockBadge::Count(2)]);
    polled(&mut core, r#"{"n":[],"u":null}"#);
    // a poll value wins again, and later title changes are stored but ignored
    let effects = polled(&mut core, r#"{"n":[],"u":5}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Count(5)]);
    assert!(titled(&mut core, "(7) P").is_empty());
    let effects = polled(&mut core, r#"{"n":[],"u":null}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Count(7)]);
    let effects = polled(&mut core, r#"{"n":[],"u":{"direct":0,"indirect":true}}"#);
    assert_eq!(badges(&effects), vec![DockBadge::Dot]);
}

#[test]
fn bad_poll_json_is_logged_and_the_tick_polls_the_expression() {
    let mut core = core_with_polled_service();
    let effects = polled(&mut core, "not json");
    assert!(matches!(effects.as_slice(), [Effect::Log(_)]));
    let effects = core.handle(Input::Tick { now_ms: 1000 });
    match &effects[0] {
        Effect::Poll { script, .. } => assert!(script.contains("u=(1+1)")),
        effect => panic!("{effect:?}"),
    }
}

#[test]
fn tick_script_exact_and_unknown_account() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let effects = core.handle(Input::Tick { now_ms: 1000 });
    assert_eq!(
        effects,
        vec![Effect::Poll {
            account: id(1),
            script: "(()=>{const m=window.__msgr;let u=null;try{u=(null)}catch(e){}return JSON.stringify({n:m?m.drain():[],u:u});})()".into()
        }]
    );
    assert!(matches!(
        core.handle(Input::TitleChanged {
            account: id(99),
            title: "x".into()
        })
        .as_slice(),
        [Effect::Log(_)]
    ));
    assert!(
        core_with(vec![account(1, "slack", "S")])
            .handle(Input::Tick { now_ms: 1000 })
            .is_empty()
    );
}

#[test]
fn dock_badge_modes() {
    let (mut core, _) = started(vec![account(1, "discord", "A"), account(2, "discord", "B")]);
    let mode = |core: &mut Core, dock_badge| {
        let settings = AppSettings {
            dock_badge,
            ..AppSettings::default()
        };
        badges(&core.handle(Input::Ui(UiCommand::UpdateSettings { settings })))
    };
    core.handle(Input::TitleChanged {
        account: id(1),
        title: "• A".into(),
    });
    assert_eq!(
        mode(&mut core, DockBadgeMode::CountOnly),
        vec![DockBadge::None]
    );
    assert_eq!(mode(&mut core, DockBadgeMode::All), vec![DockBadge::Dot]);
    let effects = core.handle(Input::TitleChanged {
        account: id(2),
        title: "(2) B".into(),
    });
    assert_eq!(badges(&effects), vec![DockBadge::Count(2)]);
    assert_eq!(mode(&mut core, DockBadgeMode::Off), vec![DockBadge::None]);
    let effects = core.handle(Input::TitleChanged {
        account: id(2),
        title: "(5) B".into(),
    });
    assert!(badges(&effects).is_empty());
    assert_eq!(last_snapshot(&effects).accounts[1].unread, 5);
    assert_eq!(
        mode(&mut core, DockBadgeMode::CountOnly),
        vec![DockBadge::Count(5)]
    );
}

#[test]
fn dock_badge_respects_in_dock_badge() {
    let mut a = account(1, "discord", "A");
    a.in_dock_badge = false;
    let (mut core, _) = started(vec![a, account(2, "discord", "B")]);
    let effects = core.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) A".into(),
    });
    assert!(badges(&effects).is_empty());
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 3);
    let effects = update_account(&mut core, 1, |settings| settings.in_dock_badge = true);
    assert_eq!(badges(&effects), vec![DockBadge::Count(3)]);
    let effects = update_account(&mut core, 1, |settings| settings.muted = true);
    assert_eq!(badges(&effects), vec![DockBadge::None]);
}

#[test]
fn show_badge_is_ui_only() {
    let (mut core, _) = started(vec![account(1, "discord", "A")]);
    let effects = update_account(&mut core, 1, |settings| settings.show_badge = false);
    assert!(!last_snapshot(&effects).accounts[0].settings.show_badge);
    let effects = core.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) A".into(),
    });
    assert_eq!(badges(&effects), vec![DockBadge::Count(3)]);
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 3);
}

#[test]
fn notification_preview_and_sound() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let raw = raw_notification("secret");
    let effects = core.handle(Input::PollResult {
        account: id(1),
        raw: raw.clone(),
    });
    assert_eq!(
        (
            notifies(&effects)[0].body.as_str(),
            notifies(&effects)[0].sound
        ),
        ("secret", true)
    );
    let settings = AppSettings {
        notification_preview: false,
        notification_sound: false,
        ..AppSettings::default()
    };
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    let effects = core.handle(Input::PollResult {
        account: id(1),
        raw,
    });
    let n = notifies(&effects);
    assert_eq!(
        (n[0].title.as_str(), n[0].body.as_str(), n[0].sound),
        ("Slack: Alice", HIDDEN_PREVIEW_BODY, false)
    );
}

#[test]
fn page_titles_are_logged_only_when_enabled() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let title_changed = |core: &mut Core, title: &str| {
        core.handle(Input::TitleChanged {
            account: id(1),
            title: title.into(),
        })
    };
    let effects = title_changed(&mut core, "(3) Alice | Secret chat");
    let lines = logs(&effects);
    assert!(
        lines.iter().any(|line| line.contains("unread 3")),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.contains("Secret")),
        "{lines:?}"
    );

    core.set_log_page_titles(true);
    let effects = title_changed(&mut core, "(4) Alice | Secret chat");
    assert!(
        logs(&effects)
            .iter()
            .any(|line| line.contains("Secret chat"))
    );
}

#[test]
fn finished_download_notifies_without_page_click() {
    let (mut core, _) = started(vec![account(1, "slack", "KIT")]);
    let downloaded = |core: &mut Core| {
        core.handle(Input::DownloadFinished {
            account: id(1),
            file_name: "report.pdf".into(),
        })
    };
    let effects = downloaded(&mut core);
    let notifications = notifies(&effects);
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        (
            notifications[0].shim_id,
            notifications[0].title.as_str(),
            notifications[0].body.as_str()
        ),
        (None, "Downloaded report.pdf", "Slack (KIT)")
    );

    let clicked = core.handle(Input::NotificationClicked {
        account: id(1),
        shim_id: None,
    });
    assert!(clicked.contains(&Effect::FocusWindow));
    assert!(
        !clicked
            .iter()
            .any(|effect| matches!(effect, Effect::Eval { .. }))
    );

    core.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
    assert!(notifies(&downloaded(&mut core)).is_empty());
}
