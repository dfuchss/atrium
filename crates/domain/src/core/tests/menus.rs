use super::*;

#[test]
fn zoom_menu_and_settings() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let zoom = |effects: &[Effect]| {
        effects.iter().find_map(|effect| {
            if let Effect::SetZoom { zoom, .. } = effect {
                Some(*zoom)
            } else {
                None
            }
        })
    };
    assert_eq!(
        zoom(&core.handle(Input::Menu(MenuAction::ZoomIn))),
        Some(1.1)
    );
    assert_eq!(
        zoom(&core.handle(Input::Menu(MenuAction::ZoomOut))),
        Some(1.0)
    );
    let effects = core.handle(Input::Menu(MenuAction::ZoomOut));
    assert_eq!(zoom(&effects), Some(0.9));
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].zoom == 0.9)
    ));
    assert_eq!(
        zoom(&core.handle(Input::Menu(MenuAction::ZoomReset))),
        Some(1.0)
    );
    assert!(core.handle(Input::Menu(MenuAction::ZoomReset)).is_empty());
    let effects = update_account(&mut core, 1, |settings| settings.zoom = 1.3);
    assert_eq!(zoom(&effects), Some(1.3));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::CreateView { .. }))
    );
    assert_eq!(
        zoom(&core.handle(Input::Menu(MenuAction::ZoomIn))),
        Some(1.5)
    );
    let effects = update_account(&mut core, 1, |settings| settings.zoom = 2.0);
    assert_eq!(zoom(&effects), Some(2.0));
    assert!(core.handle(Input::Menu(MenuAction::ZoomIn)).is_empty());
    assert_eq!(
        zoom(&update_account(&mut core, 1, |settings| settings.zoom = f64::NAN)),
        Some(1.0)
    );
    // a new view gets the persisted zoom
    update_account(&mut core, 1, |settings| settings.zoom = 0.8);
    let effects = update_account(&mut core, 1, |settings| {
        settings.user_agent = Some("chrome".into());
    });
    assert_eq!(zoom(&effects), Some(0.8));
}

#[test]
fn menu_actions_on_active_account() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    core.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        core.handle(Input::Menu(MenuAction::Reload)),
        vec![Effect::Reload(id(2))]
    );
    assert_eq!(
        core.handle(Input::Menu(MenuAction::Back)),
        vec![Effect::Eval {
            account: id(2),
            script: "history.back()".into()
        }]
    );
    assert_eq!(
        core.handle(Input::Ui(UiCommand::GoForward { account: id(1) })),
        vec![Effect::Eval {
            account: id(1),
            script: "history.forward()".into()
        }]
    );
    assert_eq!(
        core.handle(Input::Menu(MenuAction::OpenInBrowser)),
        vec![Effect::OpenInBrowser {
            account: id(2),
            browser: None
        }]
    );
    let effects = core.handle(Input::Menu(MenuAction::ToggleDnd));
    assert!(last_snapshot(&effects).settings.dnd);
    assert!(menus(&effects)[0].dnd);
    let effects = core.handle(Input::Menu(MenuAction::ToggleDnd));
    assert!(!last_snapshot(&effects).settings.dnd);
    let (mut empty, _) = started(vec![]);
    assert!(matches!(
        empty.handle(Input::Menu(MenuAction::Reload)).as_slice(),
        [Effect::Log(_)]
    ));
}

#[test]
fn open_in_browser_and_clear_data() {
    let (mut core, _) = started(vec![account(1, "slack", "A"), account(2, "slack", "B")]);
    let effects = core.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
    assert_eq!(effects.last(), Some(&Effect::ClearData(id(2))));
    core.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let effects = core.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
    assert_eq!(
        effects.last(),
        Some(&Effect::DeleteProfile {
            store_id: *id(2).as_bytes()
        })
    );
    let effects = core.handle(Input::Ui(UiCommand::OpenInBrowser { account: id(2) }));
    assert_eq!(
        effects,
        vec![Effect::OpenExternal {
            url: "https://app.slack.com/client".into(),
            browser: None
        }]
    );
}

#[test]
fn menus_follow_accounts_unread_and_settings() {
    let (mut core, effects) = started(vec![account(1, "discord", "A"), account(2, "discord", "B")]);
    let m = menus(&effects)[0].clone();
    assert_eq!(
        m.accounts
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
    assert!(m.show_tray && !m.dnd && m.tray_title.is_empty());
    assert!(
        menus(&core.handle(Input::Tick { now_ms: 1 })).is_empty(),
        "unchanged -> not re-emitted"
    );
    let effects = core.handle(Input::TitleChanged {
        account: id(2),
        title: "(3) B".into(),
    });
    let m = menus(&effects)[0];
    assert_eq!((m.accounts[1].unread, m.tray_title.as_str()), (3, "3"));
    let effects = core.handle(Input::TitleChanged {
        account: id(2),
        title: "• B".into(),
    });
    assert_eq!(menus(&effects)[0].tray_title, "•");
    let settings = AppSettings {
        show_tray: false,
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    assert!(!menus(&effects)[0].show_tray);
    let effects = update_account(&mut core, 1, |settings| settings.label = "Work".into());
    assert_eq!(menus(&effects)[0].accounts[0].label, "Work");
    assert!(
        menus(&core_with(vec![]).handle(Input::Tick { now_ms: 1 })).is_empty(),
        "nothing before start"
    );
}
