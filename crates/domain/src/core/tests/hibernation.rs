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

fn start_asleep_account(n: u128) -> Account {
    let mut account = account(n, "slack", "Asleep");
    account.start_asleep = true;
    account
}

fn startup_views(effects: &[Effect]) -> Vec<Uuid> {
    views(effects)
        .iter()
        .map(|(spec, _)| spec.account)
        .collect()
}

#[test]
fn start_asleep_account_gets_no_view_at_startup() {
    let (mut core, effects) = started(vec![
        account(1, "slack", "A"),
        start_asleep_account(2),
        account(3, "slack", "C"),
    ]);
    assert_eq!(startup_views(&effects), vec![id(1), id(3)]);
    assert!(has(
        &effects,
        &Effect::Log(format!("[{}] starts asleep", id(2)))
    ));
    assert!(last_snapshot(&effects).accounts[1].hibernated);
    // like a hibernated account: no polls
    let effects = tick_range(&mut core, 0, 5_000);
    assert!(!polls(&effects).contains(&id(2)));
}

#[test]
fn selecting_a_start_asleep_account_loads_it() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), start_asleep_account(2)]);
    let effects = core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        views(&effects)
            .iter()
            .map(|(spec, visible)| (spec.account, *visible))
            .collect::<Vec<_>>(),
        vec![(id(2), true)]
    );
    assert!(!last_snapshot(&effects).accounts[1].hibernated);
}

#[test]
fn active_start_asleep_account_is_loaded_anyway() {
    let mut core = core_with(vec![start_asleep_account(1), start_asleep_account(2)]);
    core.active = Some(id(2));
    let effects = core.handle(Input::Started {
        width: 1000.0,
        height: 600.0,
    });
    assert_eq!(startup_views(&effects), vec![id(2)]);
    assert!(!has(
        &effects,
        &Effect::Log(format!("[{}] starts asleep", id(2)))
    ));
    assert!(has(
        &effects,
        &Effect::Log(format!("[{}] starts asleep", id(1)))
    ));
}

#[test]
fn toggling_start_asleep_at_runtime_changes_nothing_now() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    let effects = update_account(&mut core, 2, |settings| settings.start_asleep = true);
    assert!(destroys(&effects).is_empty() && views(&effects).is_empty());
    assert!(!last_snapshot(&effects).accounts[1].hibernated);
    let effects = update_account(&mut core, 2, |settings| settings.start_asleep = false);
    assert!(destroys(&effects).is_empty() && views(&effects).is_empty());
}

#[test]
fn start_asleep_does_not_change_the_hibernation_timeout() {
    let mut b = start_asleep_account(2);
    b.hibernate_after_min = Some(1);
    let (mut core, _) = started(vec![account(1, "slack", "A"), b]);
    core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    core.handle(Input::Ui(UiCommand::Select { account: id(1) }));
    tick_range(&mut core, 0, 50_000);
    assert_eq!(
        destroys(&core.handle(Input::Tick { now_ms: 60_000 })),
        vec![id(2)]
    );
}
