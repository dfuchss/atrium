//! The browser for links: the system default, or an application. Where the backend lists the
//! installed browsers (Linux) one is picked from a list; elsewhere the name is typed.

use leptos::prelude::*;
use protocol::{BrowserChoice, CMD_LIST_BROWSERS, Snapshot};

use crate::commands::update_settings;
use crate::widgets::{Row, no_autocorrect};
use crate::{query, tauri};

const BROWSERS: [&str; 6] = [
    "Safari",
    "Firefox",
    "Google Chrome",
    "Arc",
    "Microsoft Edge",
    "Brave Browser",
];

/// System default browser, or an application.
#[component]
pub(super) fn BrowserRow(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let browser = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.browser.clone()));
    let installed = RwSignal::new(None::<Vec<BrowserChoice>>);
    if query::use_tauri() {
        leptos::task::spawn_local(async move {
            if let Ok(list) = tauri::call::<Option<Vec<BrowserChoice>>>(CMD_LIST_BROWSERS, ()).await
            {
                installed.set(list);
            }
        });
    }
    move || match installed.get() {
        Some(list) => installed_row(snapshot, browser, list).into_any(),
        None => typed_row(snapshot, browser).into_any(),
    }
}

fn default_radio(snapshot: ReadSignal<Snapshot>, browser: Memo<Option<String>>) -> impl IntoView {
    view! {
        <label class="radio">
            <input type="radio" name="browser" prop:checked=move || browser.get().is_none()
                on:change=move |_| update_settings(snapshot, |settings| settings.browser = None) />
            <span>"System default browser"</span>
        </label>
    }
}

/// The browser is typed, with the usual application names as suggestions.
fn typed_row(snapshot: ReadSignal<Snapshot>, browser: Memo<Option<String>>) -> impl IntoView {
    // Draft of the text field: committed on change/blur, never overwritten by snapshots.
    let draft = RwSignal::new(browser.get_untracked().unwrap_or_default());
    let commit = move || {
        let name = draft.get_untracked().trim().to_string();
        update_settings(snapshot, |settings| {
            settings.browser = (!name.is_empty()).then_some(name);
        });
    };
    view! {
        <Row label="Browser" hint="Enter the name of an installed application, e.g. Firefox.">
            {default_radio(snapshot, browser)}
            <label class="radio">
                <input type="radio" name="browser" prop:checked=move || browser.get().is_some()
                    on:change=move |_| {
                        if draft.get_untracked().trim().is_empty() {
                            draft.set("Safari".into());
                        }
                        commit();
                    } />
                <span>"Open links in:"</span>
                <input
                    type="text"
                    list="browsers"
                    placeholder="Application name"
                    autocapitalize="off"
                    spellcheck="false"
                    autocomplete="off"
                    prop:value=move || draft.get()
                    on:input=move |event| draft.set(event_target_value(&event))
                    on:change=move |_| commit()
                    use:no_autocorrect />
                <datalist id="browsers">
                    {BROWSERS.map(|name| view! { <option value=name></option> }).into_iter().collect_view()}
                </datalist>
            </label>
        </Row>
    }
}

/// The browser is chosen from the installed ones; a saved one that is gone stays visible.
fn installed_row(
    snapshot: ReadSignal<Snapshot>,
    browser: Memo<Option<String>>,
    list: Vec<BrowserChoice>,
) -> impl IntoView {
    let first = list.first().map(|choice| choice.id.clone());
    let options = StoredValue::new(list);
    let missing = move || {
        browser
            .get()
            .filter(|id| options.with_value(|list| list.iter().all(|choice| &choice.id != id)))
    };
    let choose =
        move |id: String| update_settings(snapshot, |settings| settings.browser = Some(id));
    let hint = if first.is_some() {
        "Choose an installed browser."
    } else {
        "No browsers found; links open in the default browser."
    };
    let disabled = first.is_none();
    // The select sits below its radio: next to it, the radio's text would wrap.
    view! {
        <Row label="Browser" hint=hint>
            {default_radio(snapshot, browser)}
            <label class="radio">
                <input type="radio" name="browser" prop:checked=move || browser.get().is_some()
                    disabled=disabled
                    on:change=move |_| {
                        if let Some(id) = first.clone() {
                            choose(id);
                        }
                    } />
                <span>"Installed browser"</span>
            </label>
            <select class="indented" disabled=disabled on:change=move |event| choose(event_target_value(&event))>
                {move || missing().map(|id| view! {
                    <option value=id.clone() selected>{format!("{id} (not installed)")}</option>
                })}
                {move || options.get_value().into_iter().map(|choice| {
                    let id = choice.id.clone();
                    view! {
                        <option value=choice.id selected=move || browser.get().as_deref() == Some(id.as_str())>
                            {choice.name}
                        </option>
                    }
                }).collect_view()}
            </select>
        </Row>
    }
}
