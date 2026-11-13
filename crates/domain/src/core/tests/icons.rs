use super::*;

fn page_loaded(core: &mut Core) -> Vec<Effect> {
    core.handle(Input::PageLoaded {
        account: id(1),
        url: "https://app.slack.com".into(),
    })
}

fn icon_found(core: &mut Core, url: &str) -> Vec<Effect> {
    core.handle(Input::IconFound {
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

fn persists_favicon(effects: &[Effect], favicon: &str) -> bool {
    effects.iter().any(
        |effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].favicon.as_deref() == Some(favicon)),
    )
}

#[test]
fn favicon_lookup_is_retried_on_page_loads_at_most_three_times() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    assert_eq!(page_loaded(&mut core), find_icon());
    // single-page apps set their icon late
    assert_eq!(page_loaded(&mut core).len(), 1);
    assert_eq!(page_loaded(&mut core).len(), 1);
    assert!(
        page_loaded(&mut core).is_empty(),
        "at most MAX_FAVICON_ATTEMPTS per run"
    );
}

#[test]
fn favicon_is_downloaded_and_cached() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = icon_found(&mut core, "https://a.slack-edge.com/icon.png");
    assert_eq!(
        effects,
        vec![Effect::FetchIcon {
            account: id(1),
            url: "https://a.slack-edge.com/icon.png".into()
        }]
    );
    let effects = core.handle(Input::IconFetched {
        account: id(1),
        data_url: None,
    });
    assert!(matches!(effects.as_slice(), [Effect::Log(_)]));
    let data = "data:image/png;base64,AAAA".to_string();
    let effects = core.handle(Input::IconFetched {
        account: id(1),
        data_url: Some(data.clone()),
    });
    assert!(persists_favicon(&effects, &data));
    assert_eq!(
        last_snapshot(&effects).accounts[0].icon.as_deref(),
        Some(&*data)
    );
    // a cached favicon is not looked up again on the next run
    let mut next_run = core_with(core.persisted.accounts.clone());
    next_run.handle(Input::Started {
        width: 800.0,
        height: 600.0,
    });
    assert!(page_loaded(&mut next_run).is_empty());
}

#[test]
fn custom_icon_wins_and_refresh_looks_up_again() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    core.handle(Input::IconFetched {
        account: id(1),
        data_url: Some("data:image/png;base64,AAAA".into()),
    });
    let effects = update_account(&mut core, 1, |settings| {
        settings.custom_icon = Some(" https://x.org/me.png ".into());
    });
    assert_eq!(
        last_snapshot(&effects).accounts[0].icon.as_deref(),
        Some("https://x.org/me.png")
    );
    // even with a cached favicon
    let effects = core.handle(Input::Ui(UiCommand::RefreshIcon { account: id(1) }));
    assert_eq!(effects, find_icon());
}

#[test]
fn icon_data_urls_are_taken_directly_and_bad_results_logged() {
    let (mut core, _) = started(vec![account(1, "slack", "A")]);
    let effects = icon_found(&mut core, "data:image/svg+xml;base64,BB");
    assert!(persists_favicon(&effects, "data:image/svg+xml;base64,BB"));
    for raw in ["null", "\"javascript:alert(1)\"", "garbage"] {
        let effects = core.handle(Input::IconFound {
            account: id(1),
            raw: raw.into(),
        });
        assert!(
            matches!(effects.as_slice(), [Effect::Log(_)]),
            "{raw}: {effects:?}"
        );
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
    let (mut core, _) = started(vec![account(1, "discord", "D")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.simple_icon = Some(" Discord ".into());
    });
    assert!(effects.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "discord".into()
    }));
    assert_eq!(
        core.account(id(1)).unwrap().simple_icon.as_deref(),
        Some("discord")
    );
    // unchanged slug: no refetch
    let effects = update_account(&mut core, 1, |settings| settings.muted = true);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchSimpleIcon { .. }))
    );
    let effects = fetched(&mut core, 1, "discord", SimpleIconResult::Found(svg("AA")));
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Persist(persisted) if persisted.accounts[0].simple_icon_data.as_deref() == Some(&*svg("AA")))));
    // change: cached glyph dropped, new fetch; a stale result is ignored
    let effects = update_account(&mut core, 1, |settings| {
        settings.simple_icon = Some("matrix".into());
    });
    assert!(effects.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "matrix".into()
    }));
    assert!(core.account(id(1)).unwrap().simple_icon_data.is_none());
    assert!(fetched(&mut core, 1, "discord", SimpleIconResult::Found(svg("BB"))).is_empty());
    assert!(core.account(id(1)).unwrap().simple_icon_data.is_none());
    // clearing the slug fetches nothing
    let effects = update_account(&mut core, 1, |settings| settings.simple_icon = None);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchSimpleIcon { .. }))
    );
    assert!(core.account(id(1)).unwrap().simple_icon.is_none());
}

#[test]
fn simple_icon_invalid_slug_logged_no_glyph() {
    let (mut core, _) = started(vec![account(1, "discord", "D")]);
    let effects = update_account(&mut core, 1, |settings| {
        settings.simple_icon = Some("dis cord".into());
    });
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::Log(m) if m.contains("invalid Simple Icons slug"))
        )
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchSimpleIcon { .. }))
    );
    assert!(core.account(id(1)).unwrap().simple_icon.is_none());
}

#[test]
fn simple_icon_fetched_on_start_when_missing() {
    let mut a = account(1, "discord", "D");
    a.simple_icon = Some("discord".into());
    let mut b = account(2, "discord", "E");
    b.simple_icon = Some("matrix".into());
    b.simple_icon_data = Some(svg("CC"));
    let (_, effects) = started(vec![a, b]);
    let n = effects
        .iter()
        .filter(|effect| matches!(effect, Effect::FetchSimpleIcon { .. }))
        .count();
    assert_eq!(n, 1);
    assert!(effects.contains(&Effect::FetchSimpleIcon {
        account: id(1),
        slug: "discord".into()
    }));
}

#[test]
fn simple_icon_precedence_and_404_fallback() {
    let (mut core, _) = started(vec![account(1, "slack", "S")]);
    let fav = "data:image/png;base64,FF".to_string();
    core.handle(Input::IconFetched {
        account: id(1),
        data_url: Some(fav.clone()),
    });
    update_account(&mut core, 1, |settings| {
        settings.simple_icon = Some("slack".into());
    });
    // 404: logged, favicon stays
    let effects = fetched(&mut core, 1, "slack", SimpleIconResult::NotFound);
    assert!(
        effects.iter().any(
            |effect| matches!(effect, Effect::Log(m) if m == "Simple Icons has no icon 'slack'")
        )
    );
    let snapshot = core.snapshot();
    assert_eq!(
        (
            snapshot.accounts[0].icon.as_deref(),
            snapshot.accounts[0].icon_glyph
        ),
        (Some(&*fav), false)
    );
    // glyph beats favicon
    let effects = fetched(&mut core, 1, "slack", SimpleIconResult::Found(svg("GG")));
    let v = &last_snapshot(&effects).accounts[0];
    assert_eq!((v.icon.as_deref(), v.icon_glyph), (Some(&*svg("GG")), true));
    // custom icon beats glyph
    let effects = update_account(&mut core, 1, |settings| {
        settings.custom_icon = Some("https://x.org/me.png".into());
    });
    let v = &last_snapshot(&effects).accounts[0];
    assert_eq!(
        (v.icon.as_deref(), v.icon_glyph),
        (Some("https://x.org/me.png"), false)
    );
    // slug without cached glyph and without favicon: initials
    let (next_run, _) = started(vec![{
        let mut a = account(1, "slack", "S");
        a.simple_icon = Some("slack".into());
        a
    }]);
    let v = &next_run.snapshot().accounts[0];
    assert_eq!((v.icon.clone(), v.icon_glyph), (None, false));
}
