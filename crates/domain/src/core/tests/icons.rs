use super::*;

#[test]
fn favicon_flow() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let loaded = |c: &mut Core| {
        c.handle(Input::PageLoaded {
            account: id(1),
            url: "https://app.slack.com".into(),
        })
    };
    let fx = loaded(&mut c);
    assert_eq!(
        fx,
        vec![Effect::FindIcon {
            account: id(1),
            script: ICON_SCRIPT.into()
        }]
    );
    // retried on later page loads (single-page apps set their icon late), at most 3 times
    assert_eq!(loaded(&mut c).len(), 1);
    assert_eq!(loaded(&mut c).len(), 1);
    assert!(
        loaded(&mut c).is_empty(),
        "at most MAX_ICON_ATTEMPTS per run"
    );
    let raw = serde_json::to_string("https://a.slack-edge.com/icon.png").unwrap();
    let fx = c.handle(Input::IconFound {
        account: id(1),
        raw,
    });
    assert_eq!(
        fx,
        vec![Effect::FetchIcon {
            account: id(1),
            url: "https://a.slack-edge.com/icon.png".into()
        }]
    );
    let fx = c.handle(Input::IconFetched {
        account: id(1),
        data_url: None,
    });
    assert!(matches!(fx.as_slice(), [Effect::Log(_)]));
    let data = "data:image/png;base64,AAAA".to_string();
    let fx = c.handle(Input::IconFetched {
        account: id(1),
        data_url: Some(data.clone()),
    });
    assert!(fx.iter().any(
        |e| matches!(e, Effect::Persist(p) if p.accounts[0].favicon.as_deref() == Some(&*data))
    ));
    assert_eq!(snap(&fx).accounts[0].icon.as_deref(), Some(&*data));
    // a custom icon wins
    let fx = update_account(&mut c, 1, |s| {
        s.custom_icon = Some(" https://x.org/me.png ".into())
    });
    assert_eq!(
        snap(&fx).accounts[0].icon.as_deref(),
        Some("https://x.org/me.png")
    );
    // refresh looks it up again, even with a cached favicon
    let fx = c.handle(Input::Ui(UiCommand::RefreshIcon { account: id(1) }));
    assert_eq!(
        fx,
        vec![Effect::FindIcon {
            account: id(1),
            script: ICON_SCRIPT.into()
        }]
    );
    // data URLs are taken directly; bad results are logged
    let raw = serde_json::to_string("data:image/svg+xml;base64,BB").unwrap();
    let fx = c.handle(Input::IconFound {
        account: id(1),
        raw,
    });
    assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].favicon.as_deref() == Some("data:image/svg+xml;base64,BB"))));
    for raw in ["null", "\"javascript:alert(1)\"", "garbage"] {
        let fx = c.handle(Input::IconFound {
            account: id(1),
            raw: raw.into(),
        });
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]), "{raw}: {fx:?}");
    }
    // a cached favicon is not looked up again on the next run
    let mut c2 = core_with(c.persisted.accounts.clone());
    c2.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert!(
        c2.handle(Input::PageLoaded {
            account: id(1),
            url: "x".into()
        })
        .is_empty()
    );
}

#[test]
fn slug_normalisation() {
    assert_eq!(normalize_slug("  Discord "), Ok(Some("discord".into())));
    assert_eq!(normalize_slug("  "), Ok(None));
    assert_eq!(normalize_slug("x2"), Ok(Some("x2".into())));
    for bad in ["a b", "dis.cord", "../x", "é", "a/b", "a-b"] {
        assert!(normalize_slug(bad).is_err(), "{bad}");
    }
}

#[test]
fn simple_icon_fetch_on_set_and_change() {
    let (mut c, _) = started(vec![acc(1, "discord", "D")]);
    let fx = update_account(&mut c, 1, |s| s.simple_icon = Some(" Discord ".into()));
    assert!(fx.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "discord".into()
    }));
    assert_eq!(
        c.account(id(1)).unwrap().simple_icon.as_deref(),
        Some("discord")
    );
    // unchanged slug: no refetch
    let fx = update_account(&mut c, 1, |s| s.muted = true);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
    );
    let fx = fetched(&mut c, 1, "discord", SimpleIconResult::Found(svg("AA")));
    assert!(fx.iter().any(|e| matches!(e, Effect::Persist(p) if p.accounts[0].simple_icon_data.as_deref() == Some(&*svg("AA")))));
    // change: cached glyph dropped, new fetch; a stale result is ignored
    let fx = update_account(&mut c, 1, |s| s.simple_icon = Some("matrix".into()));
    assert!(fx.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "matrix".into()
    }));
    assert!(c.account(id(1)).unwrap().simple_icon_data.is_none());
    assert!(fetched(&mut c, 1, "discord", SimpleIconResult::Found(svg("BB"))).is_empty());
    assert!(c.account(id(1)).unwrap().simple_icon_data.is_none());
    // clearing the slug fetches nothing
    let fx = update_account(&mut c, 1, |s| s.simple_icon = None);
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
    );
    assert!(c.account(id(1)).unwrap().simple_icon.is_none());
}

#[test]
fn simple_icon_invalid_slug_logged_no_glyph() {
    let (mut c, _) = started(vec![acc(1, "discord", "D")]);
    let fx = update_account(&mut c, 1, |s| s.simple_icon = Some("dis cord".into()));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Log(m) if m.contains("invalid Simple Icons slug")))
    );
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
    );
    assert!(c.account(id(1)).unwrap().simple_icon.is_none());
}

#[test]
fn simple_icon_fetched_on_start_when_missing() {
    let mut a = acc(1, "discord", "D");
    a.simple_icon = Some("discord".into());
    let mut b = acc(2, "discord", "E");
    b.simple_icon = Some("matrix".into());
    b.simple_icon_data = Some(svg("CC"));
    let (_, fx) = started(vec![a, b]);
    let n = fx
        .iter()
        .filter(|e| matches!(e, Effect::FetchSimpleIcon { .. }))
        .count();
    assert_eq!(n, 1);
    assert!(fx.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "discord".into()
    }));
}

#[test]
fn simple_icon_precedence_and_404_fallback() {
    let (mut c, _) = started(vec![acc(1, "slack", "S")]);
    let fav = "data:image/png;base64,FF".to_string();
    c.handle(Input::IconFetched {
        account: id(1),
        data_url: Some(fav.clone()),
    });
    update_account(&mut c, 1, |s| s.simple_icon = Some("slack".into()));
    // 404: logged, favicon stays
    let fx = fetched(&mut c, 1, "slack", SimpleIconResult::NotFound);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Log(m) if m == "Simple Icons has no icon 'slack'"))
    );
    let s = c.snapshot();
    assert_eq!(
        (s.accounts[0].icon.as_deref(), s.accounts[0].icon_glyph),
        (Some(&*fav), false)
    );
    // glyph beats favicon
    let fx = fetched(&mut c, 1, "slack", SimpleIconResult::Found(svg("GG")));
    let v = &snap(&fx).accounts[0];
    assert_eq!((v.icon.as_deref(), v.icon_glyph), (Some(&*svg("GG")), true));
    // custom icon beats glyph
    let fx = update_account(&mut c, 1, |s| {
        s.custom_icon = Some("https://x.org/me.png".into())
    });
    let v = &snap(&fx).accounts[0];
    assert_eq!(
        (v.icon.as_deref(), v.icon_glyph),
        (Some("https://x.org/me.png"), false)
    );
    // slug without cached glyph and without favicon: initials
    let (c2, _) = started(vec![{
        let mut a = acc(1, "slack", "S");
        a.simple_icon = Some("slack".into());
        a
    }]);
    let v = &c2.snapshot().accounts[0];
    assert_eq!((v.icon.clone(), v.icon_glyph), (None, false));
}
