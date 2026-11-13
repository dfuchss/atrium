use super::*;

#[test]
fn dialog_bounds() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    assert!(matches!(fx[0], Effect::SetUiBounds(b) if b.width == 1000.0));
    assert_eq!(fx[1], Effect::Hide(id(1)));
    assert!(snap(&fx).dialog_open);
    let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(matches!(fx[0], Effect::SetUiBounds(b) if b.width == SIDEBAR));
    assert!(matches!(fx[1], Effect::SetBounds { .. }));
    assert_eq!(fx[2], Effect::Show(id(1)));
    let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert!(matches!(fx.as_slice(), [Effect::EmitSnapshot(_)]));
}

#[test]
fn icon_size_sets_sidebar_width() {
    assert_eq!(IconSize::default().sidebar_width(), SIDEBAR);
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let large = AppSettings {
        icon_size: IconSize::Large,
        ..AppSettings::default()
    };
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings {
        settings: large.clone(),
    }));
    let content = Bounds {
        x: 72.0,
        y: 0.0,
        width: 1000.0 - 72.0,
        height: 600.0,
    };
    assert!(fx.contains(&Effect::SetUiBounds(Bounds {
        x: 0.0,
        y: 0.0,
        width: 72.0,
        height: 600.0
    })));
    assert!(fx.contains(&Effect::SetBounds {
        account: id(1),
        bounds: content
    }));
    // Unchanged icon size: no layout effects.
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings: large }));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::SetUiBounds(_) | Effect::SetBounds { .. }))
    );
    // Resizes and newly shown views use the new width.
    let fx = c.handle(Input::WindowResized {
        width: 800.0,
        height: 500.0,
    });
    assert!(fx.contains(&Effect::SetBounds {
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
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    c.handle(Input::Ui(UiCommand::SetDialog { open: true }));
    let medium = AppSettings {
        icon_size: IconSize::Medium,
        ..AppSettings::default()
    };
    let fx = c.handle(Input::Ui(UiCommand::UpdateSettings { settings: medium }));
    assert!(fx.contains(&Effect::SetUiBounds(Bounds {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 600.0
    })));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::SetBounds { .. } | Effect::Show(_)))
    );
    let fx = c.handle(Input::Ui(UiCommand::SetDialog { open: false }));
    assert_eq!(
        fx[0],
        Effect::SetUiBounds(Bounds {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 600.0
        })
    );
    assert_eq!(
        fx[1],
        Effect::SetBounds {
            account: id(1),
            bounds: Bounds {
                x: 64.0,
                y: 0.0,
                width: 936.0,
                height: 600.0
            }
        }
    );
}

#[test]
fn resize_and_misc() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = c.handle(Input::WindowResized {
        width: 50.0,
        height: 300.0,
    });
    assert!(
        matches!(fx[1], Effect::SetBounds { bounds, .. } if bounds.width == 0.0 && bounds.height == 300.0)
    );
    assert!(
        c.handle(Input::PageLoaded {
            account: id(99),
            url: "x".into()
        })
        .is_empty()
    );
    assert_eq!(
        c.handle(Input::ExternalLink {
            account: id(1),
            url: "https://u.org/".into()
        }),
        vec![Effect::OpenExternal {
            url: "https://u.org/".into(),
            browser: None
        }]
    );
}
