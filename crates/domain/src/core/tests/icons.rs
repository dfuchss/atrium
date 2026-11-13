use super::*;

fn page_loaded(c: &mut Core) -> Vec<Effect> {
    c.handle(Input::PageLoaded {
        account: id(1),
        url: "https://app.slack.com".into(),
    })
}

fn icon_found(c: &mut Core, url: &str) -> Vec<Effect> {
    c.handle(Input::IconFound {
        account: id(1),
        raw: serde_json::to_string(url).unwrap(),
    })
}

fn find_icon() -> Vec<Effect> {
    vec![Effect::FindIcon {
        account: id(1),
        script: ICON_SCRIPT.into(),
    }]
}

fn persists_favicon(fx: &[Effect], favicon: &str) -> bool {
    fx.iter().any(
        |e| matches!(e, Effect::Persist(p) if p.accounts[0].favicon.as_deref() == Some(favicon)),
    )
}

#[test]
fn favicon_lookup_is_retried_on_page_loads_at_most_three_times() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    assert_eq!(page_loaded(&mut c), find_icon());
    // single-page apps set their icon late
    assert_eq!(page_loaded(&mut c).len(), 1);
    assert_eq!(page_loaded(&mut c).len(), 1);
    assert!(
        page_loaded(&mut c).is_empty(),
        "at most MAX_FAVICON_ATTEMPTS per run"
    );
}

#[test]
fn favicon_is_downloaded_and_cached() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = icon_found(&mut c, "https://a.slack-edge.com/icon.png");
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
    assert!(persists_favicon(&fx, &data));
    assert_eq!(snap(&fx).accounts[0].icon.as_deref(), Some(&*data));
    // a cached favicon is not looked up again on the next run
    let mut next_run = core_with(c.persisted.accounts.clone());
    next_run.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert!(page_loaded(&mut next_run).is_empty());
}

#[test]
fn custom_icon_wins_and_refresh_looks_up_again() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    c.handle(Input::IconFetched {
        account: id(1),
        data_url: Some("data:image/png;base64,AAAA".into()),
    });
    let fx = update_account(&mut c, 1, |s| {
        s.custom_icon = Some(" https://x.org/me.png ".into());
    });
    assert_eq!(
        snap(&fx).accounts[0].icon.as_deref(),
        Some("https://x.org/me.png")
    );
    // even with a cached favicon
    let fx = c.handle(Input::Ui(UiCommand::RefreshIcon { account: id(1) }));
    assert_eq!(fx, find_icon());
}

#[test]
fn icon_data_urls_are_taken_directly_and_bad_results_logged() {
    let (mut c, _) = started(vec![acc(1, "slack", "A")]);
    let fx = icon_found(&mut c, "data:image/svg+xml;base64,BB");
    assert!(persists_favicon(&fx, "data:image/svg+xml;base64,BB"));
    for raw in ["null", "\"javascript:alert(1)\"", "garbage"] {
        let fx = c.handle(Input::IconFound {
            account: id(1),
            raw: raw.into(),
        });
        assert!(matches!(fx.as_slice(), [Effect::Log(_)]), "{raw}: {fx:?}");
    }
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
        s.custom_icon = Some("https://x.org/me.png".into());
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
