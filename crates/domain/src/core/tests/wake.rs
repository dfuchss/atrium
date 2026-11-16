use super::super::wake::WAKE_GRACE_MS;
use super::*;
use crate::model::Settings;

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

fn with_global_after_wake(mut core: Core, after_wake: AfterWake) -> Core {
    let settings = AppSettings {
        after_wake,
        ..AppSettings::default()
    };
    core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    core
}

fn reloads(effects: &[Effect]) -> Vec<Uuid> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Reload(account) = effect {
                Some(*account)
            } else {
                None
            }
        })
        .collect()
}

const AFTER_SLEEP_MS: u64 = 31_000 + SLEEP_GAP_MS + 1;

/// Sleeps, wakes the screens at [`AFTER_SLEEP_MS`] and returns the effects of the wake.
fn wake(core: &mut Core) -> Vec<Effect> {
    core.handle(Input::ScreensWoke {
        now_ms: AFTER_SLEEP_MS,
    })
}

/// Ticks every second from just after the wake to the end of the grace period.
fn tick_through_grace(core: &mut Core) -> Vec<Effect> {
    (1..=WAKE_GRACE_MS / 1_000)
        .flat_map(|second| {
            core.handle(Input::Tick {
                now_ms: AFTER_SLEEP_MS + second * 1_000,
            })
        })
        .collect()
}

fn poll_answer(core: &mut Core, n: u128, raw: &str) -> Vec<Effect> {
    core.handle(Input::PollResult {
        account: id(n),
        raw: raw.into(),
    })
}

const POLL_ANSWER: &str = r#"{"n":[],"u":null}"#;

#[test]
fn sleep_gap_alone_does_not_reload() {
    let mut core = with_global_after_wake(three_accounts_one_hibernated(), AfterWake::Reload);
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
fn check_is_the_default() {
    assert_eq!(AppSettings::default().after_wake, AfterWake::Check);
}

#[test]
fn check_reloads_only_the_silent_account_once() {
    let mut core = three_accounts_one_hibernated();
    let effects = wake(&mut core);
    assert!(logs(&effects).contains(&"screens woke after sleep"));
    assert!(reloads(&effects).is_empty(), "a check does not reload");
    let mut effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS + 1_000,
    });
    effects.extend(poll_answer(&mut core, 1, POLL_ANSWER));
    assert!(logs(&effects).contains(&&*format!("[{}] alive after wake (1000 ms)", id(1))));
    let effects = tick_through_grace(&mut core);
    // the hibernated account 3 is not watched
    assert_eq!(reloads(&effects), vec![id(2)]);
    assert!(logs(&effects).contains(&&*format!(
        "[{}] no answer 15 s after wake -> reload",
        id(2)
    )));
    // a late answer or later ticks change nothing
    let mut effects = poll_answer(&mut core, 2, POLL_ANSWER);
    effects.extend(tick_range(
        &mut core,
        AFTER_SLEEP_MS + 20_000,
        AFTER_SLEEP_MS + 60_000,
    ));
    assert!(reloads(&effects).is_empty());
    assert!(
        !logs(&effects)
            .iter()
            .any(|line| line.contains("after wake"))
    );
}

#[test]
fn an_undecodable_answer_counts_but_an_empty_one_does_not() {
    let mut core = three_accounts_one_hibernated();
    wake(&mut core);
    poll_answer(&mut core, 1, "not json");
    poll_answer(&mut core, 2, "");
    assert_eq!(reloads(&tick_through_grace(&mut core)), vec![id(2)]);
}

#[test]
fn every_silent_account_is_reloaded_after_the_grace_period_only() {
    let mut core = three_accounts_one_hibernated();
    wake(&mut core);
    let effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS + WAKE_GRACE_MS - 1,
    });
    assert!(reloads(&effects).is_empty());
    let effects = core.handle(Input::Tick {
        now_ms: AFTER_SLEEP_MS + WAKE_GRACE_MS,
    });
    assert_eq!(reloads(&effects), vec![id(1), id(2)]);
}

#[test]
fn accounts_unloaded_or_reloaded_while_watched_are_not_reloaded() {
    let mut core = three_accounts_one_hibernated();
    wake(&mut core);
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let effects = core.handle(Input::Ui(UiCommand::Reload { account: id(1) }));
    assert_eq!(reloads(&effects), vec![id(1)]);
    assert!(reloads(&tick_through_grace(&mut core)).is_empty());
}

#[test]
fn a_new_sleep_during_the_check_waits_for_the_next_screen_wake() {
    let mut core = three_accounts_one_hibernated();
    wake(&mut core);
    // a dark wake after the next sleep must not reload
    let dark_wake_ms = AFTER_SLEEP_MS + SLEEP_GAP_MS + 1;
    let effects = core.handle(Input::Tick {
        now_ms: dark_wake_ms,
    });
    assert!(reloads(&effects).is_empty());
    let effects = core.handle(Input::Tick {
        now_ms: dark_wake_ms + WAKE_GRACE_MS,
    });
    assert!(reloads(&effects).is_empty());
}

#[test]
fn reload_mode_reloads_loaded_views_once() {
    let mut core = with_global_after_wake(three_accounts_one_hibernated(), AfterWake::Reload);
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
    assert_eq!(reloads(&effects), vec![id(1), id(2)]);
    let effects = core.handle(Input::ScreensWoke {
        now_ms: 2 * AFTER_SLEEP_MS + 1_000,
    });
    assert!(reloads(&effects).is_empty());
}

#[test]
fn reload_mode_reloads_when_the_screens_wake_before_the_first_tick() {
    let mut core = with_global_after_wake(three_accounts_one_hibernated(), AfterWake::Reload);
    assert_eq!(reloads(&wake(&mut core)).len(), 2);
    // the gap was counted once, and nothing is watched
    let effects = tick_through_grace(&mut core);
    assert!(logs(&effects).is_empty());
    assert!(reloads(&effects).is_empty());
    let effects = core.handle(Input::ScreensWoke {
        now_ms: AFTER_SLEEP_MS + 16_000,
    });
    assert!(reloads(&effects).is_empty());
}

#[test]
fn screens_waking_without_sleep_does_nothing() {
    let mut core = three_accounts_one_hibernated();
    let effects = core.handle(Input::ScreensWoke { now_ms: 32_000 });
    assert!(effects.is_empty());
    assert!(reloads(&tick_through_grace(&mut core)).is_empty());
}

#[test]
fn never_mode_does_nothing() {
    let mut core = with_global_after_wake(three_accounts_one_hibernated(), AfterWake::Never);
    let effects = wake(&mut core);
    assert!(logs(&effects).contains(&"screens woke after sleep"));
    assert!(reloads(&effects).is_empty());
    assert!(reloads(&tick_through_grace(&mut core)).is_empty());
}

#[test]
fn account_override_beats_the_global_mode() {
    let mut core = with_global_after_wake(three_accounts_one_hibernated(), AfterWake::Reload);
    update_account(&mut core, 1, |settings| {
        settings.after_wake = Some(AfterWake::Never);
    });
    update_account(&mut core, 2, |settings| {
        settings.after_wake = Some(AfterWake::Check);
    });
    assert!(reloads(&wake(&mut core)).is_empty());
    assert_eq!(reloads(&tick_through_grace(&mut core)), vec![id(2)]);
    assert_eq!(
        core.account(id(2)).unwrap().after_wake,
        Some(AfterWake::Check)
    );
}

#[test]
fn a_terminated_web_process_reloads_its_view() {
    let mut core = three_accounts_one_hibernated();
    wake(&mut core);
    let effects = core.handle(Input::WebProcessTerminated { account: id(2) });
    assert_eq!(reloads(&effects), vec![id(2)]);
    assert_eq!(
        logs(&effects),
        vec![format!("[{}] web process terminated -> reload", id(2))]
    );
    // the reload ends the check of account 2
    assert_eq!(reloads(&tick_through_grace(&mut core)), vec![id(1)]);
    // a view that is gone is not reloaded
    let effects = core.handle(Input::WebProcessTerminated { account: id(3) });
    assert!(effects.is_empty());
}

#[test]
fn old_reload_after_wake_setting_is_migrated() {
    let load = |reload: bool| -> Settings {
        serde_json::from_value(serde_json::json!({ "reload_after_wake": reload, "dnd": true }))
            .unwrap()
    };
    assert_eq!(load(true).app.after_wake, AfterWake::Check);
    assert_eq!(load(false).app.after_wake, AfterWake::Never);
    assert!(load(false).app.dnd);
    let saved = serde_json::to_value(load(false)).unwrap();
    assert_eq!(saved["after_wake"], "never");
    assert!(saved.get("reload_after_wake").is_none());
    let reloaded: Settings = serde_json::from_value(saved).unwrap();
    assert_eq!(reloaded.app.after_wake, AfterWake::Never);
}

#[test]
fn settings_without_after_wake_load_with_defaults() {
    let settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.app.after_wake, AfterWake::Check);
    let mut v = serde_json::to_value(account(1, "slack", "A")).unwrap();
    v.as_object_mut().unwrap().remove("after_wake");
    let account: Account = serde_json::from_value(v).unwrap();
    assert_eq!(account.after_wake, None);
}
