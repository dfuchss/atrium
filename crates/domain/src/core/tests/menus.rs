use super::*;

#[test]
fn zoom_menu_and_settings() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let zoom = |fx: &[Effect]| {
        fx.iter().find_map(|e| {
            if let Effect::SetZoom { zoom, .. } = e {
                Some(*zoom)
            } else {
                None
            }
        })
    };
    assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomIn))), Some(1.1));
    assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomOut))), Some(1.0));
    let fx = c.handle(Input::Menu(MenuAction::ZoomOut));
    assert_eq!(zoom(&fx), Some(0.9));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].zoom == 0.9))
    );
    assert_eq!(
        zoom(&c.handle(Input::Menu(MenuAction::ZoomReset))),
        Some(1.0)
    );
    assert!(c.handle(Input::Menu(MenuAction::ZoomReset)).is_empty());
    let fx = update_account(&mut c, 1, |s| s.zoom = 1.3);
    assert_eq!(zoom(&fx), Some(1.3));
    assert!(!fx.iter().any(|e| matches!(e, Effect::CreateView { .. })));
    assert_eq!(zoom(&c.handle(Input::Menu(MenuAction::ZoomIn))), Some(1.5));
    let fx = update_account(&mut c, 1, |s| s.zoom = 2.0);
    assert_eq!(zoom(&fx), Some(2.0));
    assert!(c.handle(Input::Menu(MenuAction::ZoomIn)).is_empty());
    assert_eq!(
        zoom(&update_account(&mut c, 1, |s| s.zoom = f64::NAN)),
        Some(1.0)
    );
    // a new view gets the persisted zoom
    update_account(&mut c, 1, |s| s.zoom = 0.8);
    let fx = update_account(&mut c, 1, |s| s.user_agent = Some("chrome".into()));
    assert_eq!(zoom(&fx), Some(0.8));
}

#[test]
fn menu_actions_on_active_account() {
    let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
    c.handle(Input::Ui(UiCommand::Select { account: id(2) }));
    assert_eq!(
        c.handle(Input::Menu(MenuAction::Reload)),
        vec![Effect::Reload(id(2))]
    );
    assert_eq!(
        c.handle(Input::Menu(MenuAction::Back)),
        vec![Effect::Eval {
            account: id(2),
            script: "history.back()".into()
        }]
    );
    assert_eq!(
        c.handle(Input::Ui(UiCommand::GoForward { account: id(1) })),
        vec![Effect::Eval {
            account: id(1),
            script: "history.forward()".into()
        }]
    );
    assert_eq!(
        c.handle(Input::Menu(MenuAction::OpenInBrowser)),
        vec![Effect::OpenInBrowser {
            account: id(2),
            browser: None
        }]
    );
    let fx = c.handle(Input::Menu(MenuAction::ToggleDnd));
    assert!(snap(&fx).settings.dnd);
    assert!(menus(&fx)[0].dnd);
    let fx = c.handle(Input::Menu(MenuAction::ToggleDnd));
    assert!(!snap(&fx).settings.dnd);
    let (mut empty, _) = started(vec![]);
    assert!(matches!(
        empty.handle(Input::Menu(MenuAction::Reload)).as_slice(),
        [Effect::Log(_)]
    ));
}

#[test]
fn open_in_browser_and_clear_data() {
    let (mut c, _) = started(vec![acc(1, "slack", "A"), acc(2, "slack", "B")]);
    let fx = c.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
    assert_eq!(fx.last(), Some(&Effect::ClearData(id(2))));
    c.handle(Input::Ui(UiCommand::Hibernate { account: id(2) }));
    let fx = c.handle(Input::Ui(UiCommand::ClearData { account: id(2) }));
    assert_eq!(
        fx.last(),
        Some(&Effect::DeleteProfile {
            store_id: *id(2).as_bytes()
        })
    );
    let fx = c.handle(Input::Ui(UiCommand::OpenInBrowser { account: id(2) }));
    assert_eq!(
        fx,
        vec![Effect::OpenExternal {
            url: "https://app.slack.com/client".into(),
            browser: None
        }]
    );
}

#[test]
fn menus_follow_accounts_unread_and_settings() {
    let (mut c, fx) = started(vec![acc(1, "discord", "A"), acc(2, "discord", "B")]);
    let m = menus(&fx)[0].clone();
    assert_eq!(
        m.accounts
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
    assert!(m.show_tray && !m.dnd && m.tray_title.is_empty());
    assert!(
        menus(&c.handle(Input::Tick { now_ms: 1 })).is_empty(),
        "unchanged -> not re-emitted"
    );
    let fx = c.handle(Input::TitleChanged {
        account: id(2),
        title: "(3) B".into(),
    });
    let m = menus(&fx)[0];
    assert_eq!((m.accounts[1].unread, m.tray_title.as_str()), (3, "3"));
    let fx = c.handle(Input::TitleChanged {
        account: id(2),
        title: "• B".into(),
    });
    assert_eq!(menus(&fx)[0].tray_title, "•");
    let settings = AppSettings {
        show_tray: false,
        ..AppSettings::default()
    };
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings }));
    assert!(!menus(&fx)[0].show_tray);
    let fx = update_account(&mut c, 1, |s| s.label = "Work".into());
    assert_eq!(menus(&fx)[0].accounts[0].label, "Work");
    assert!(
        menus(&core_with(vec![]).handle(Input::Tick { now_ms: 1 })).is_empty(),
        "nothing before start"
    );
}
