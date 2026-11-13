use super::*;

#[test]
fn hibernate_after_idle_time() {
    let mut b = acc(2, "slack", "B");
    b.hibernate_after_min = Some(1);
    let mut a = acc(1, "slack", "A");
    a.hibernate_after_min = Some(1);
    let (mut c, _) = started(vec![a, b]);
    let fx = tick_range(&mut c, 0, 50_000);
    assert!(destroys(&fx).is_empty());
    let fx = c.handle(Input::Tick { now_ms: 60_000 });
    // the active account (1) is never hibernated
    assert_eq!(destroys(&fx), vec![id(2)]);
    assert_eq!(polls(&fx), vec![id(1)]);
    let s = snap(&fx);
    assert!(!s.accounts[0].hibernated && s.accounts[1].hibernated);
    // a hibernated account is not polled and not unloaded twice
    let fx = tick_range(&mut c, 70_000, 200_000);
    assert!(destroys(&fx).is_empty());
    assert!(!polls(&fx).contains(&id(2)));

    // selecting it loads it again (visible); the account left becomes idle from now on
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(fx[0], Effect::Hide(id(1)));
    assert_eq!(
        views(&fx)
            .iter()
            .map(|(s, v)| (s.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(2), true)]
    );
    assert!(!snap(&fx).accounts[1].hibernated);
    // left at the last tick (200 s) -> unloaded one minute later
    let fx = tick_range(&mut c, 210_000, 250_000);
    assert!(destroys(&fx).is_empty());
    let fx = c.handle(Input::Tick { now_ms: 260_000 });
    assert_eq!(destroys(&fx), vec![id(1)]);
}

#[test]
fn global_hibernate_applies_to_accounts_without_override() {
    let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
    tick_range(&mut c, 0, 50_000);
    // changing the setting unloads nothing by itself
    set_global_hibernate(&mut c, Some(1));
    assert!(destroys(&c.handle(Input::Tick { now_ms: 55_000 })).is_empty());
    // measured from the last activity of the account
    let fx = c.handle(Input::Tick { now_ms: 60_000 });
    assert_eq!(destroys(&fx), vec![id(2)]);
}

#[test]
fn account_override_beats_global_hibernate() {
    let mut b = acc(2, "slack", "B");
    b.hibernate_after_min = Some(2);
    let (mut c, _) = started(vec![acc(1, "slack", "A"), b]);
    set_global_hibernate(&mut c, Some(1));
    tick_range(&mut c, 0, 60_000);
    assert!(destroys(&c.handle(Input::Tick { now_ms: 60_000 })).is_empty());
    let fx = c.handle(Input::Tick { now_ms: 120_000 });
    assert_eq!(destroys(&fx), vec![id(2)]);
}

#[test]
fn zero_means_never_even_with_global_hibernate() {
    let mut b = acc(2, "slack", "B");
    b.hibernate_after_min = Some(0);
    let (mut c, _) = started(vec![acc(1, "slack", "A"), b]);
    set_global_hibernate(&mut c, Some(1));
    let fx = tick_range(&mut c, 0, 600_000);
    assert!(destroys(&fx).is_empty());
}

#[test]
fn active_account_is_never_hibernated_by_global() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    set_global_hibernate(&mut c, Some(1));
    let fx = tick_range(&mut c, 0, 600_000);
    assert!(destroys(&fx).is_empty());
}

#[test]
fn old_persisted_account_json_loads() {
    let mut v = serde_json::to_value(acc(1, "slack", "A")).unwrap();
    v.as_object_mut().unwrap().remove("hibernate_after_min");
    let a: Account = serde_json::from_value(v).unwrap();
    assert_eq!(a.hibernate_after_min, None);
    let old: AppSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.hibernate_after_min, None);
}

#[test]
fn hibernate_command() {
    let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
    let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(1) }));
    assert!(
        matches!(fx.as_slice(), [Effect::Log(_)]),
        "active account: only logged"
    );
    let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert_eq!(fx[0], Effect::Destroy(id(2)));
    assert!(snap(&fx).accounts[1].hibernated);
    let fx = c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert!(destroys(&fx).is_empty());
    assert_eq!(polls(&c.handle(Input::Tick { now_ms: 1 })), vec![id(1)]);
    // commands on an unloaded view are logged, not sent to the host
    let fx = c.handle(Input::Ui(UiCommand::Reload { account: id(2) }));
    assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
    let fx = c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(views(&fx).len(), 1);
    assert!(views(&fx)[0].1);
    assert!(matches!(
        c.handle(Input::Ui(UiCommand::Hibernate { account: id(9) }))
            .as_slice(),
        [Effect::Log(_)]
    ));
}

#[test]
fn wake_detection_reloads_loaded_views() {
    let (mut c, _) = started(vec![
        acc(1, "slack", "A"),
        acc(2, "slack", "B"),
        acc(3, "slack", "C"),
    ]);
    c.handle(Input::Ui(UiCommand::Hibernate { account: id(3) }));
    let fx = tick_range(&mut c, 1_000, 31_000);
    assert!(logs(&fx).is_empty() && !fx.iter().any(|e| matches!(e, Effect::Reload(_))));
    let fx = c.handle(Input::Tick {
        now_ms: 31_000 + WAKE_GAP_MS + 1,
    });
    assert!(logs(&fx)[0].contains("wake detected"));
    let reloads: Vec<_> = fx
        .iter()
        .filter(|e| matches!(e, Effect::Reload(_)))
        .cloned()
        .collect();
    assert_eq!(reloads, vec![Effect::Reload(id(1)), Effect::Reload(id(2))]);
    // exactly 30 s is not a wake
    let fx = c.handle(Input::Tick {
        now_ms: 31_000 + WAKE_GAP_MS + 1 + WAKE_GAP_MS,
    });
    assert!(logs(&fx).is_empty());

    let settings = AppSettings {
        reload_after_wake: false,
        ..AppSettings::default()
    };
    c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    let fx = c.handle(Input::Tick { now_ms: 10_000_000 });
    assert!(logs(&fx)[0].contains("wake detected"));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Reload(_))));
}
