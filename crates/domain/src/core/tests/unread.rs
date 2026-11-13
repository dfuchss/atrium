use super::*;

#[test]
fn title_unread_and_dock_badge() {
    let (mut c, _) = started(vec![acc(1, "discord", "Discord")]);
    let t = |c: &mut Core, title: &str| {
        c.handle(Input::TitleChanged {
            account: id(1),
            title: title.into(),
        })
    };
    let fx = t(&mut c, "(3) Discord | x");
    assert_eq!(snap(&fx).accounts[0].unread, 3);
    assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
    assert!(t(&mut c, "(3) Discord | x").is_empty());
    let fx = t(&mut c, "• Discord | x");
    assert_eq!(badges(&fx), vec![DockBadge::Dot]);
    assert!(snap(&fx).accounts[0].indirect);
    let fx = t(&mut c, "Discord");
    assert_eq!(badges(&fx), vec![DockBadge::None]);
}

#[test]
fn muted_not_in_badge() {
    let mut a = acc(1, "discord", "Discord");
    a.muted = true;
    let (mut c, _) = started(vec![a]);
    let fx = c.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) D".into(),
    });
    assert!(badges(&fx).is_empty());
    assert_eq!(snap(&fx).accounts[0].unread, 3);
}

#[test]
fn poll_notifies_with_prefix() {
    let (mut c, _) = started(vec![acc(1, "slack", "KIT"), acc(2, "slack", "Slack")]);
    let inner = r#"{"n":[{"id":7,"title":"Alice","body":"hi"}],"u":null}"#;
    let raw = serde_json::to_string(inner).unwrap();
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw,
    });
    let n = notifies(&fx);
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0].shim_id, n[0].title.as_str(), n[0].body.as_str()),
        (7, "Slack (KIT): Alice", "hi")
    );
    let fx = c.handle(Input::PollResult {
        account: id(2),
        raw: inner.into(),
    });
    assert_eq!(notifies(&fx)[0].title, "Slack: Alice");
}

#[test]
fn poll_muted_or_dnd_silent() {
    let mut a = acc(1, "slack", "Slack");
    a.muted = true;
    let (mut c, _) = started(vec![a, acc(2, "slack", "Slack")]);
    let raw = r#"{"n":[{"id":1,"title":"x"}],"u":null}"#.to_string();
    assert!(
        notifies(&c.handle(Input::PollResult {
            account: id(1),
            raw: raw.clone()
        }))
        .is_empty()
    );
    assert_eq!(
        notifies(&c.handle(Input::PollResult {
            account: id(2),
            raw: raw.clone()
        }))
        .len(),
        1
    );
    c.handle(Input::Ui(UiCommand::SetDnd { enabled: true }));
    assert!(
        notifies(&c.handle(Input::PollResult {
            account: id(2),
            raw
        }))
        .is_empty()
    );
}

#[test]
fn poll_unread_and_bad_json() {
    let mut def = Catalog::parse("p", "id='p'\nname='P'\ncolor='#fff'\nurl='https://p.io'\n[unread]\npoll='1+1'\ntitle_regex='^\\((\\d+)\\)'").unwrap();
    def.id = "p".into();
    let mut cat = Catalog::builtin();
    cat.insert(def);
    let persisted = Persisted {
        accounts: vec![acc(1, "p", "P")],
        ..Default::default()
    };
    let mut c = Core::new(cat, persisted, Box::new(|| Uuid::from_u128(999)));
    c.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":null,"u":4}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(4)]);
    // poll wins over titles
    assert!(
        c.handle(Input::TitleChanged {
            account: id(1),
            title: "(9) P".into()
        })
        .is_empty()
    );
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":{"direct":0,"indirect":true}}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Dot]);
    // no poll value: the last title value (9) takes over
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":null}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(9)]);
    // title changes apply while the poll yields nothing
    let fx = c.handle(Input::TitleChanged {
        account: id(1),
        title: "(2) P".into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(2)]);
    c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":null}"#.into(),
    });
    // a poll value wins again, and later title changes are stored but ignored
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":5}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(5)]);
    assert!(
        c.handle(Input::TitleChanged {
            account: id(1),
            title: "(7) P".into()
        })
        .is_empty()
    );
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":null}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(7)]);
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: r#"{"n":[],"u":{"direct":0,"indirect":true}}"#.into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Dot]);
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: "not json".into(),
    });
    assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
    let fx = c.handle(Input::Tick { now_ms: 1000 });
    match &fx[0] {
        Effect::Poll { script, .. } => assert!(script.contains("u=(1+1)")),
        e => panic!("{e:?}"),
    }
}

#[test]
fn tick_script_exact_and_unknown_account() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
    let fx = c.handle(Input::Tick { now_ms: 1000 });
    assert_eq!(
        fx,
        vec![Effect::Poll {
            account: id(1),
            script: "(()=>{const m=window.__msgr;let u=null;try{u=(null)}catch(e){}return JSON.stringify({n:m?m.drain():[],u:u});})()".into()
        }]
    );
    assert!(matches!(
        c.handle(Input::TitleChanged {
            account: id(99),
            title: "x".into()
        })
        .as_slice(),
        [Effect::Log(_)]
    ));
    assert!(
        core_with(vec![acc(1, "slack", "S")])
            .handle(Input::Tick { now_ms: 1000 })
            .is_empty()
    );
}

#[test]
fn dock_badge_modes() {
    let (mut c, _) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
    let mode = |c: &mut Core, dock_badge| {
        let settings = AppSettings {
            dock_badge,
            ..AppSettings::default()
        };
        badges(&c.handle(Input::Ui(UiCommand::UpdateSettings { settings })))
    };
    c.handle(Input::TitleChanged {
        account: id(1),
        title: "• A".into(),
    });
    assert_eq!(
        mode(&mut c, DockBadgeMode::CountOnly),
        vec![DockBadge::None]
    );
    assert_eq!(mode(&mut c, DockBadgeMode::All), vec![DockBadge::Dot]);
    let fx = c.handle(Input::TitleChanged {
        account: id(2),
        title: "(2) B".into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(2)]);
    assert_eq!(mode(&mut c, DockBadgeMode::Off), vec![DockBadge::None]);
    let fx = c.handle(Input::TitleChanged {
        account: id(2),
        title: "(5) B".into(),
    });
    assert!(badges(&fx).is_empty());
    assert_eq!(snap(&fx).accounts[1].unread, 5);
    assert_eq!(
        mode(&mut c, DockBadgeMode::CountOnly),
        vec![DockBadge::Count(5)]
    );
}

#[test]
fn dock_badge_respects_in_dock_badge() {
    let mut a = acc(1, "discord", "A");
    a.in_dock_badge = false;
    let (mut c, _) = started(vec![a, acc(2, "discord", "B")]);
    let fx = c.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) A".into(),
    });
    assert!(badges(&fx).is_empty());
    assert_eq!(snap(&fx).accounts[0].unread, 3);
    let fx = update_account(&mut c, 1, |s| s.in_dock_badge = true);
    assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
    let fx = update_account(&mut c, 1, |s| s.muted = true);
    assert_eq!(badges(&fx), vec![DockBadge::None]);
}

#[test]
fn show_badge_is_ui_only() {
    let (mut c, _) = started(vec![acc(1, "discord", "A")]);
    let fx = update_account(&mut c, 1, |s| s.show_badge = false);
    assert!(!snap(&fx).accounts[0].settings.show_badge);
    let fx = c.handle(Input::TitleChanged {
        account: id(1),
        title: "(3) A".into(),
    });
    assert_eq!(badges(&fx), vec![DockBadge::Count(3)]);
    assert_eq!(snap(&fx).accounts[0].unread, 3);
}

#[test]
fn notification_preview_and_sound() {
    let (mut c, _) = started(vec![acc(1, "slack", "Slack")]);
    let raw = raw_notification("secret");
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw: raw.clone(),
    });
    assert_eq!(
        (notifies(&fx)[0].body.as_str(), notifies(&fx)[0].sound),
        ("secret", true)
    );
    let settings = AppSettings {
        notification_preview: false,
        notification_sound: false,
        ..AppSettings::default()
    };
    c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    let fx = c.handle(Input::PollResult {
        account: id(1),
        raw,
    });
    let n = notifies(&fx);
    assert_eq!(
        (n[0].title.as_str(), n[0].body.as_str(), n[0].sound),
        ("Slack: Alice", HIDDEN_PREVIEW_BODY, false)
    );
}
