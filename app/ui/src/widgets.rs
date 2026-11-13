//! Small reusable view pieces: icons, form rows, switches, confirm buttons and select options.

use std::time::Duration;

use leptos::prelude::*;
use protocol::AccountView;
use wasm_bindgen::JsCast;

/// Directive: switch off macOS auto-correction (not a typed attribute in leptos).
pub(crate) fn no_autocorrect(element: web_sys::Element) {
    let _ = element.set_attribute("autocorrect", "off");
}

/// Focuses the element `id` after the next render.
pub(crate) fn focus_by_id(id: &'static str) {
    request_animation_frame(move || {
        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(id))
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = element.focus();
        }
    });
}

/// Inline SVG drawn with `currentColor`, centred in a fixed-size box.
#[component]
pub(crate) fn Icon(svg: &'static str) -> impl IntoView {
    view! { <span class="ico" inner_html=svg></span> }
}

/// Round icon: the account's image if it has one, else initials on the service color.
pub(crate) fn account_icon(account: &AccountView, small: bool) -> AnyView {
    match &account.icon {
        Some(source) if account.icon_glyph => view! { <span class="icon has-img glyph" class:small=small style=format!("background:{}", account.color)><img src=source.clone() alt="" draggable="false" /></span> }.into_any(),
        Some(source) => view! { <span class="icon has-img" class:small=small><img src=source.clone() alt="" draggable="false" /></span> }.into_any(),
        None => view! { <span class="icon" class:small=small style=format!("background:{}", account.color)>{account.initials.clone()}</span> }.into_any(),
    }
}

/// Two-step destructive action: the first click arms it ("Confirm ..." + "Cancel"), which
/// reverts by itself after 5 s. Replaces `window.confirm`, which WKWebView does not support.
#[component]
pub(crate) fn ConfirmButton(
    label: &'static str,
    confirm_label: &'static str,
    #[prop(into)] on_confirm: Callback<()>,
    #[prop(optional)] small: bool,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] title: Signal<String>,
) -> impl IntoView {
    let armed = RwSignal::new(false);
    let generation = RwSignal::new(0u32);
    let size = if small { " small" } else { "" };
    let arm = move |_| {
        armed.set(true);
        let armed_generation = generation.get_untracked() + 1;
        generation.set(armed_generation);
        set_timeout(
            move || {
                if generation.try_get_untracked() == Some(armed_generation) {
                    armed.try_set(false);
                }
            },
            Duration::from_secs(5),
        );
    };
    view! {
        <Show
            when=move || armed.get()
            fallback=move || view! {
                <button type="button" class=format!("btn danger{size}") disabled=move || disabled.get() title=move || title.get() on:click=arm>{label}</button>
            }
        >
            <button type="button" class=format!("btn danger confirm{size}") on:click=move |_| {
                armed.set(false);
                on_confirm.run(());
            }>{confirm_label}</button>
            <button type="button" class=format!("btn{size}") on:click=move |_| armed.set(false)>"Cancel"</button>
        </Show>
    }
}

/// One form row: label column + control column (with optional muted helper text under it).
#[component]
pub(crate) fn Row(
    #[prop(into)] label: String,
    #[prop(optional)] hint: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="frow">
            <span class="flabel">{label}</span>
            <div class="fctl">
                {children()}
                {(!hint.is_empty()).then(|| view! { <span class="hint">{hint}</span> })}
            </div>
        </div>
    }
}

/// macOS-style switch row.
#[component]
pub(crate) fn Toggle(
    label: &'static str,
    #[prop(optional)] hint: &'static str,
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] on_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <label class="frow toggle">
            <span class="flabel">{label}</span>
            <span class="fctl">
                <input type="checkbox" class="switch" prop:checked=move || checked.get() on:change=move |event| on_change.run(event_target_checked(&event)) />
                {(!hint.is_empty()).then(|| view! { <span class="hint">{hint}</span> })}
            </span>
        </label>
    }
}

/// Trimmed text, None if empty.
pub(crate) fn non_empty(text: &str) -> Option<String> {
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}

/// Label of the per-account instance field; "Instance" when the service defines none.
pub(crate) fn instance_field_label(label: String) -> String {
    if label.trim().is_empty() {
        "Instance".into()
    } else {
        label
    }
}

/// Sleep timeouts offered in the selects, in minutes.
pub(crate) const HIBERNATE_MINUTES: [u32; 5] = [5, 15, 30, 60, 120];

/// "5 min", "2 h", ...
pub(crate) fn minutes_label(minutes: u32) -> String {
    if minutes >= 60 && minutes.is_multiple_of(60) {
        format!("{} h", minutes / 60)
    } else {
        format!("{minutes} min")
    }
}

/// Label of a global sleep timeout.
pub(crate) fn hibernate_label(minutes: Option<u32>) -> String {
    minutes
        .filter(|&minutes| minutes > 0)
        .map_or_else(|| "Never".to_string(), minutes_label)
}

/// Options of a numeric select; a stored value that is not in the list is kept as an extra option.
pub(crate) fn number_options(
    choices: &[u32],
    current: Option<u32>,
    label: fn(u32) -> String,
) -> impl IntoView + use<> {
    let mut values = choices.to_vec();
    if let Some(current) = current
        && current > 0
        && !values.contains(&current)
    {
        values.push(current);
        values.sort_unstable();
    }
    values
        .into_iter()
        .map(|value| view! { <option value=value.to_string()>{label(value)}</option> })
        .collect_view()
}
