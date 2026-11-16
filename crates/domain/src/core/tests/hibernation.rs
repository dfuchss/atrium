use super::*;

#[test]
fn hibernate_after_idle_time() {
    let mut b = account(2, "slack", "B");
    b.hibernate_after_min = Some(1);
    let mut a = account(1, "slack", "A");
    a.hibernate_after_min = Some(1);
    let (mut core, _) = started(vec![a, b]);
    let effects = tick_range(&mut core, 0, 50_000);
    assert!(destroys(&effects).is_empty());
    let effects = core.handle(Input::Tick { now_ms: 60_000 });
    // the active account (1) is never hibernated
    assert_eq!(destroys(&effects), vec![id(2)]);
    assert_eq!(polls(&effects), vec![id(1)]);
    let snapshot = last_snapshot(&effects);
    assert!(!snapshot.accounts[0].hibernated && snapshot.accounts[1].hibernated);
    // a hibernated account is not polled and not unloaded twice
    let effects = tick_range(&mut core, 70_000, 200_000);
    assert!(destroys(&effects).is_empty());
    assert!(!polls(&effects).contains(&id(2)));

    // selecting it loads it again (visible); the account left becomes idle from now on
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(effects[0], Effect::Hide(id(1)));
    assert_eq!(
        views(&effects)
            .iter()
            .map(|(snapshot, v)| (snapshot.account, *v))
            .collect::<Vec<_>>(),
        vec![(id(2), true)]
    );
    assert!(!last_snapshot(&effects).accounts[1].hibernated);
    // left at the last tick (200 s) -> unloaded one minute later
    let effects = tick_range(&mut core, 210_000, 250_000);
    assert!(destroys(&effects).is_empty());
    let effects = core.handle(Input::Tick { now_ms: 260_000 });
    assert_eq!(destroys(&effects), vec![id(1)]);
}

#[test]
fn global_hibernate_applies_to_accounts_without_override() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    tick_range(&mut core, 0, 50_000);
    // changing the setting unloads nothing by itself
    set_global_hibernate(&mut core, Some(1));
    assert!(destroys(&core.handle(Input::Tick { now_ms: 55_000 })).is_empty());
    // measured from the last activity of the account
    let effects = core.handle(Input::Tick { now_ms: 60_000 });
    assert_eq!(destroys(&effects), vec![id(2)]);
}

#[test]
fn account_override_beats_global_hibernate() {
    let mut b = account(2, "slack", "B");
    b.hibernate_after_min = Some(2);
    let (mut core, _) = started(vec![account(1, "slack", "A"), b]);
    set_global_hibernate(&mut core, Some(1));
    tick_range(&mut core, 0, 60_000);
    assert!(destroys(&core.handle(Input::Tick { now_ms: 60_000 })).is_empty());
    let effects = core.handle(Input::Tick { now_ms: 120_000 });
    assert_eq!(destroys(&effects), vec![id(2)]);
}

#[test]
fn zero_means_never_even_with_global_hibernate() {
    let mut b = account(2, "slack", "B");
    b.hibernate_after_min = Some(0);
    let (mut core, _) = started(vec![account(1, "slack", "A"), b]);
    set_global_hibernate(&mut core, Some(1));
    let effects = tick_range(&mut core, 0, 600_000);
    assert!(destroys(&effects).is_empty());
}

#[test]
fn active_account_is_never_hibernated_by_global() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    set_global_hibernate(&mut core, Some(1));
    let effects = tick_range(&mut core, 0, 600_000);
    assert!(destroys(&effects).is_empty());
}

#[test]
fn old_persisted_account_json_loads() {
    let mut v = serde_json::to_value(account(1, "slack", "A")).unwrap();
    v.as_object_mut().unwrap().remove("hibernate_after_min");
    let a: Account = serde_json::from_value(v).unwrap();
    assert_eq!(a.hibernate_after_min, None);
    let old: AppSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.hibernate_after_min, None);
}

#[test]
fn hibernate_command() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    let effects = core.handle(Input::Ui(UiCommand::Hibernate { account: id(1) }));
    assert!(
        matches!(effects.as_slice(), [Effect::Log(_)]),
        "active account: only logged"
    );
    let effects = core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert_eq!(effects[0], Effect::Destroy(id(2)));
    assert!(last_snapshot(&effects).accounts[1].hibernated);
    let effects = core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    assert!(destroys(&effects).is_empty());
    assert_eq!(polls(&core.handle(Input::Tick { now_ms: 1 })), vec![id(1)]);
    // commands on an unloaded view are logged, not sent to the host
    let effects = core.handle(Input::Ui(UiCommand::Reload { account: id(2) }));
    assert!(matches!(effects.as_slice(), [Effect::Log(_)]));
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(views(&effects).len(), 1);
    assert!(views(&effects)[0].1);
    assert!(matches!(
        core.handle(Input::Ui(UiCommand::Hibernate { account: id(9) }))
            .as_slice(),
        [Effect::Log(_)]
    ));
}

/// Accounts 1 and 2 loaded, 3 hibernated, ticked up to 31 s.
fn three_accounts_one_hibernated() -> Core {
    let (mut core, _) = started(vec![
        account(1, "slack", "A"),
        account(2, "slack", "B"),
        account(3, "slack", "C"),
    ]);
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(3) }));
    tick_range(&mut core, 1_000, 31_000);
    core
}

fn reloads(effects: &[Effect]) -> Vec<Effect> {
    effects
        .iter()
        .filter(|effect| matches!(effect, Effect::Reload(_)))
        .cloned()
        .collect()
}

const AFTER_SLEEP_MS: u64 = 31_000 + SLEEP_GAP_MS + 1;

#[test]
fn sleep_gap_alone_does_not_reload() {
    let mut core = three_accounts_one_hibernated();
    let effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS,
    });
    assert!(logs(&effects)[0].contains("sleep detected"));
    assert!(reloads(&effects).is_empty());
    // exactly 30 s is no sleep
    let effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS + SLEEP_GAP_MS,
    });
    assert!(logs(&effects).is_empty());
}

#[test]
fn screens_waking_after_sleep_reloads_loaded_views_once() {
    let mut core = three_accounts_one_hibernated();
    // dark wakes (Power Nap) only leave gaps between ticks
    core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS,
    });
    core.handle(Input::Tick {
        now_ms: 2 * AFTER_SLEEP_MS,
    });
    let effects = core.handle(Input::ScreensWoke {
        now_ms: 2 * AFTER_SLEEP_MS + 500,
    });
    assert_eq!(
        reloads(&effects),
        vec![Effect::Reload(id(1)), Effect::Reload(id(2))]
    );
    let effects = core.handle(Input::ScreensWoke {
        now_ms: 2 * AFTER_SLEEP_MS + 1_000,
    });
    assert!(reloads(&effects).is_empty());
}

#[test]
fn screens_waking_before_the_first_tick_after_sleep_reloads() {
    let mut core = three_accounts_one_hibernated();
    let effects = core.handle(Input::ScreensWoke {
        now_ms: AFTER_SLEEP_MS,
    });
    assert_eq!(reloads(&effects).len(), 2);
    // the gap was counted once
    let effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS + 1_000,
    });
    assert!(logs(&effects).is_empty());
    let effects = core.handle(Input::ScreensWoke {
        now_ms: AFTER_SLEEP_MS + 2_000,
    });
    assert!(reloads(&effects).is_empty());
}

#[test]
fn screens_waking_without_sleep_does_not_reload() {
    let mut core = three_accounts_one_hibernated();
    let effects = core.handle(Input::ScreensWoke { now_ms: 32_000 });
    assert!(effects.is_empty());
}

#[test]
fn no_reload_after_wake_when_disabled() {
    let mut core = three_accounts_one_hibernated();
    let settings = AppSettings {
        reload_after_wake: false,
        ..AppSettings::default()
    };
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS,
    });
    let effects = core.handle(Input::ScreensWoke {
        now_ms: AFTER_SLEEP_MS + 500,
    });
    assert!(logs(&effects)[0].contains("screens woke after sleep"));
    assert!(reloads(&effects).is_empty());
}
