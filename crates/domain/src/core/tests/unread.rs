use super::super::notifications::HIDDEN_PREVIEW_BODY;
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
fn slack_title_counts_new_items() {
    let (mut core, _) = started(vec![account(1, "slack", "Slack")]);
    let mut unread = |title: &str| {
        let effects = core.handle(Input::TitleChanged {
            account: id(1),
            title: title.into(),
        });
        last_snapshot(&effects).accounts[0].unread
    };
    assert_eq!(unread("dev (Channel) - Team - 1 new item - Slack"), 1);
    assert_eq!(unread("dev (Channel) - Team - 12 new items - Slack"), 12);
    assert_eq!(unread("dev (Channel) - Team - Slack"), 0);
}

#[test]
fn muted_not_in_badge() {
    let mut a = account(1, "discord", "Discord");
    a.settings.muted = true;
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
    a.settings.muted = true;
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
    let mut core = Core::new(
        cat,
        persisted,
        Box::new(|| Uuid::from_u128(999)),
        UA_PLATFORM.into(),
    );
    core.handle(STARTED);
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
fn bad_poll_json_is_logged_once_until_it_recovers() {
    let mut core = core_with_polled_service();
    let effects = polled(&mut core, "not json");
    assert!(matches!(effects.as_slice(), [Effect::Log(line)] if line.contains("bad JSON")));
    assert!(polled(&mut core, "not json either").is_empty());
    polled(&mut core, r#"{"n":[],"u":1}"#);
    assert!(matches!(
        polled(&mut core, "not json").as_slice(),
        [Effect::Log(_)]
    ));
}

#[test]
fn the_tick_evaluates_the_expression_apart_from_the_drain() {
    let mut core = core_with_polled_service();
    let effects = core.handle(Input::Tick { now_ms: 1000 });
    match effects.as_slice() {
        [
            Effect::Eval { script: unread, .. },
            Effect::Poll { script: drain, .. },
            ..,
        ] => {
            assert!(unread.contains("m.unread=(\n1+1\n)"), "{unread}");
            assert_eq!(drain, scripts::POLL_SCRIPT);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_broken_expression_stays_out_of_the_drain_script() {
    let broken = "1})();document.title='x';(()=>{(";
    let script = scripts::unread_script(broken);
    assert!(
        script.starts_with(
            "(()=>{const m=window.__atrium;if(!m)return;m.unread=null;try{m.unread=(\n"
        )
    );
    assert!(script.ends_with("\n)}catch(e){}})()"));
    assert!(!scripts::POLL_SCRIPT.contains(broken));
    // A trailing line comment cannot swallow the closing parenthesis.
    assert!(scripts::unread_script("1 // count").contains("1 // count\n)"));
}

#[test]
fn tick_script_exact_and_unknown_account() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let effects = core.handle(Input::Tick { now_ms: 1000 });
    assert_eq!(
        effects,
        vec![Effect::Poll {
            account: id(1),
            script: "(()=>{const m=window.__atrium;if(!m)return JSON.stringify({n:[],u:null});let u=null;try{u=JSON.parse(JSON.stringify(m.unread))}catch(e){}m.unread=null;return JSON.stringify({n:m.drain(),u:u});})()".into()
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
fn dock_badge_account_mode_overrides_global() {
    let mut a = account(1, "discord", "A");
    a.settings.dock_badge = Some(DockBadgeMode::CountOnly);
    let (mut core, _) = started(vec![a, account(2, "discord", "B")]);
    let effects = core.handle(Input::TitleChanged {
        account: id(1),
        title: "• A".into(),
    });
    assert!(badges(&effects).is_empty());
    let effects = core.handle(Input::TitleChanged {
        account: id(2),
        title: "• B".into(),
    });
    assert_eq!(badges(&effects), vec![DockBadge::Dot]);
    let effects = update_account(&mut core, 2, |settings| {
        settings.dock_badge = Some(DockBadgeMode::Off);
    });
    assert_eq!(badges(&effects), vec![DockBadge::None]);
    let effects = update_account(&mut core, 2, |settings| settings.dock_badge = None);
    assert_eq!(badges(&effects), vec![DockBadge::Dot]);
}

#[test]
fn legacy_in_dock_badge_false_reads_as_off() {
    let mut a = account(1, "discord", "A");
    a.settings.in_dock_badge = false;
    let (mut core, _) = started(vec![a, account(2, "discord", "B")]);
    let effects = core.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) A".into(),
    });
    assert!(badges(&effects).is_empty());
    assert_eq!(last_snapshot(&effects).accounts[0].unread, 3);
    let effects = update_account(&mut core, 1, |settings| {
        settings.in_dock_badge = true;
        settings.dock_badge = Some(DockBadgeMode::CountOnly);
    });
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

#[test]
fn finished_download_of_muted_account_is_silent() {
    let mut a = account(1, "slack", "KIT");
    a.settings.muted = true;
    let (mut core, _) = started(vec![a]);
    let effects = core.handle(Input::DownloadFinished {
        account: id(1),
        file_name: "report.pdf".into(),
    });
    assert!(notifies(&effects).is_empty());
}

/// Poll result with `count` notifications titled `title` and bodied `body`.
fn poll_with(count: usize, title: &str, body: &str) -> String {
    let queue: Vec<serde_json::Value> = (1..=count)
        .map(|shim_id| serde_json::json!({"id": shim_id, "title": title, "body": body}))
        .collect();
    serde_json::json!({"n": queue, "u": null}).to_string()
}

#[test]
fn notifications_are_capped_per_poll_and_minute() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let mut shown = 0;
    for second in 0..10u64 {
        core.handle(Input::Tick {
            now_ms: 1_000 + second * 1_000,
        });
        let effects = core.handle(Input::PollResult {
            account: id(1),
            raw: poll_with(8, "Alice", "hi"),
        });
        let in_this_poll = notifies(&effects).len();
        assert!(in_this_poll <= 5, "{in_this_poll}");
        shown += in_this_poll;
        if in_this_poll < 8 {
            assert!(logs(&effects).iter().any(|line| line.contains("dropped")));
        }
    }
    assert_eq!(shown, 20);
    core.handle(Input::Tick { now_ms: 61_000 });
    let effects = core.handle(Input::PollResult {
        account: id(1),
        raw: poll_with(1, "Alice", "hi"),
    });
    assert_eq!(
        notifies(&effects).len(),
        1,
        "a minute later the budget is back"
    );
}

#[test]
fn notification_text_is_truncated() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let effects = core.handle(Input::PollResult {
        account: id(1),
        raw: poll_with(1, &"t".repeat(500), &"b".repeat(5_000)),
    });
    let notification = notifies(&effects)[0];
    assert_eq!(notification.title, format!("Discord: {}", "t".repeat(200)));
    assert_eq!(notification.body, "b".repeat(1_000));
}

#[test]
fn oversized_poll_results_are_dropped_and_logged_once() {
    let (mut core, _) = started(vec![account(1, "discord", "Discord")]);
    let oversized = poll_with(1, "Alice", &"x".repeat(300 * 1024));
    let poll = |core: &mut Core, raw: &str| {
        core.handle(Input::PollResult {
            account: id(1),
            raw: raw.into(),
        })
    };
    let first = poll(&mut core, &oversized);
    assert!(notifies(&first).is_empty());
    assert_eq!(logs(&first).len(), 1);
    assert!(poll(&mut core, &oversized).is_empty(), "logged once");
    assert_eq!(
        notifies(&poll(&mut core, &poll_with(1, "Alice", "hi"))).len(),
        1
    );
    assert_eq!(
        logs(&poll(&mut core, &oversized)).len(),
        1,
        "logged again after recovery"
    );
}
