use super::*;

#[test]
fn dialog_bounds() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    assert!(matches!(effects[0], Effect::SetUiBounds(b) if b.width == WINDOW_WIDTH));
    assert_eq!(effects[1], Effect::Hide(id(1)));
    assert!(last_snapshot(&effects).dialog_open);
    let effects = core.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(matches!(effects[0], Effect::SetUiBounds(b) if b.width == SIDEBAR));
    assert!(matches!(effects[1], Effect::SetBounds { .. }));
    assert_eq!(effects[2], Effect::Show(id(1)));
    let effects = core.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(matches!(effects.as_slice(), [Effect::EmitSnapshot(_)]));
}

#[test]
fn icon_size_sets_sidebar_width() {
    assert_eq!(IconSize::default().sidebar_width(), SIDEBAR);
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let large = AppSettings {
        icon_size: IconSize::Large,
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: large.clone(),
    }));
    let content = Bounds {
        x: 72.0,
        y: 0.0,
        width: WINDOW_WIDTH - 72.0,
        height: WINDOW_HEIGHT,
    };
    assert!(effects.contains(&Effect::SetUiBounds(Bounds {
        x: 0.0,
        y: 0.0,
        width: 72.0,
        height: WINDOW_HEIGHT
    })));
    assert!(effects.contains(&Effect::SetBounds {
        account: id(1),
        bounds: content
    }));
    // Unchanged icon size: no layout effects.
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings { settings: large }));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetUiBounds(_) | Effect::SetBounds { .. }))
    );
    // Resizes and newly shown views use the new width.
    let effects = core.handle(Input::WindowResized {
        width: 800.0,
        height: 500.0,
    });
    assert!(effects.contains(&Effect::SetBounds {
        account: id(1),
        bounds: Bounds {
            x: 72.0,
            y: 0.0,
            width: 728.0,
            height: 500.0
        }
    }));
}

#[test]
fn icon_size_change_with_open_dialog_keeps_views_covered() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    core.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let medium = AppSettings {
        icon_size: IconSize::Medium,
        ..AppSettings::default()
    };
    let effects = core.handle(Input::Ui(UiCommand::UpdateSettings { settings: medium }));
    assert!(effects.contains(&Effect::SetUiBounds(Bounds {
        x: 0.0,
        y: 0.0,
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT
    })));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetBounds { .. } | Effect::Show(_)))
    );
    let effects = core.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert_eq!(
        effects[0],
        Effect::SetUiBounds(Bounds {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: WINDOW_HEIGHT
        })
    );
    assert_eq!(
        effects[1],
        Effect::SetBounds {
            account: id(1),
            bounds: Bounds {
                x: 64.0,
                y: 0.0,
                width: 936.0,
                height: WINDOW_HEIGHT
            }
        }
    );
}

#[test]
fn resize_and_misc() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = core.handle(Input::WindowResized {
        width: 50.0,
        height: 300.0,
    });
    assert!(
        matches!(effects[1], Effect::SetBounds { bounds, .. } if bounds.width == 0.0 && bounds.height == 300.0)
    );
    assert!(
        core.handle(Input::PageLoaded {
            account: id(99),
            url: "x".into()
        })
        .is_empty()
    );
    assert_eq!(
        core.handle(Input::ExternalLink {
            account: id(1),
            url: "https://u.org/".into()
        }),
        vec![Effect::OpenExternal {
            url: "https://u.org/".into(),
            browser: None
        }]
    );
}
