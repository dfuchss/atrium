//! The sidebar: one button per account (reorderable by dragging, see `drag`) and the tool
//! buttons.

mod drag;

use leptos::prelude::*;
use protocol::{Badge, Snapshot, UiCommand, UpdateStatus};
use uuid::Uuid;

use crate::commands::send_command;
use crate::glyphs::{ICON_BELL, ICON_BELL_OFF, ICON_GEAR, ICON_PLUS};
use crate::settings::{Tab, open_dialog};
use crate::update::UpdateStatusSignal;
use crate::widgets::{Icon, account_by_id, account_icon};
use drag::{DragState, drag_style};

fn badge_text(unread: u32) -> String {
    if unread > 99 {
        "99+".into()
    } else {
        unread.to_string()
    }
}

#[component]
pub(crate) fn Sidebar(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let drag = DragState::new(snapshot);
    window_event_listener(leptos::ev::keydown, move |event| {
        if event.key() == "Escape" {
            drag.escape();
        }
    });
    view! {
        <nav class="sidebar">
            <div
                class="accounts"
                node_ref=drag.list
                class:dragging=move || drag.preview.get().is_some()
                on:pointerdown=move |event| drag.pointer_down(&event)
                on:pointermove=move |event| drag.pointer_move(&event)
                on:pointerup=move |event| drag.pointer_up(&event)
                on:pointercancel=move |_| drag.pointer_cancel()
                on:lostpointercapture=move |_| drag.lost_pointer_capture()
                on:dragstart=|event: web_sys::DragEvent| event.prevent_default()
            >
                <For
                    each=move || drag.order.get()
                    key=|id| *id
                    children=move |id| view! { <AccountButton snapshot=snapshot drag=drag id=id /> }
                />
            </div>
            <SidebarTools snapshot=snapshot />
        </nav>
    }
}

#[component]
fn AccountButton(snapshot: ReadSignal<Snapshot>, drag: DragState, id: Uuid) -> impl IntoView {
    let account = account_by_id(snapshot, id);
    let active = move || snapshot.with(|snapshot| snapshot.active == Some(id));
    let badge_when_muted =
        Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.badge_when_muted));
    let global_badge_style =
        Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.badge_style));
    view! {
        <button
            class="acct"
            class:active=active
            class:muted=move || account.get().is_some_and(|account| account.settings.muted)
            class:disabled=move || account.get().is_some_and(|account| !account.settings.enabled)
            class:hibernated=move || account.get().is_some_and(|account| account.settings.enabled && account.hibernated)
            title=move || account.get().map_or_else(String::new, |account| format!("{} \u{2013} {}", account.service_name, account.settings.label))
            data-id=id.to_string()
            draggable="false"
            class:dragged=move || drag.preview.get().is_some_and(|preview| preview.account == id)
            style=move || {
                let index = drag.order.with(|order| order.iter().position(|other| *other == id)).unwrap_or(0);
                drag_style(drag.preview.get(), id, index)
            }
            on:click=move |_| {
                if drag.suppress_click.get_value() {
                    drag.suppress_click.set_value(false);
                } else {
                    send_command(UiCommand::Select { account: id });
                }
            }
            on:contextmenu=move |event| {
                event.prevent_default();
                send_command(UiCommand::ShowAccountMenu { account: id });
            }
        >
            {move || account.get().map(|account| {
                let show_badge = account.settings.shows_badge(badge_when_muted.get());
                let muted = account.settings.muted;
                let hibernated = account.settings.enabled && account.hibernated;
                view! {
                    {account_icon(&account, false)}
                    {show_badge.then(|| match account.settings.effective_badge_style(global_badge_style.get()).badge(account.unread, account.indirect) {
                        Badge::Count(count) => view! { <span class="badge">{badge_text(count)}</span> }.into_any(),
                        Badge::UrgentDot => view! { <span class="dot urgent"></span> }.into_any(),
                        Badge::NormalDot => view! { <span class="dot"></span> }.into_any(),
                        Badge::None => ().into_any(),
                    })}
                    {muted.then(|| view! { <span class="mute-mark">"\u{1F507}"</span> })}
                    {hibernated.then(|| view! { <span class="moon-mark" title="Sleeping">"\u{1F319}"</span> })}
                }
            })}
        </button>
    }
}

/// Add account, settings and Do Not Disturb.
#[component]
fn SidebarTools(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let dnd = Memo::new(move |_| snapshot.with(|snapshot| snapshot.settings.dnd));
    let UpdateStatusSignal(update_status) = expect_context::<UpdateStatusSignal>();
    let has_update = Memo::new(move |_| {
        update_status.with(|status| status.as_ref().is_some_and(UpdateStatus::has_update))
    });
    view! {
        <div class="bottom">
            <button class="tool" title="Add account" on:click=move |_| open_dialog(Tab::Accounts, true)><Icon svg=ICON_PLUS /></button>
            <button
                class="tool"
                class:update=move || has_update.get()
                title=move || if has_update.get() { "Settings \u{2013} update available" } else { "Settings" }
                on:click=move |_| open_dialog(if has_update.get_untracked() { Tab::About } else { Tab::General }, false)
            >
                <Icon svg=ICON_GEAR />
            </button>
            <button
                class="tool"
                class:on=move || dnd.get()
                title=move || if dnd.get() { "Do not disturb: on" } else { "Do not disturb: off" }
                on:click=move |_| send_command(UiCommand::SetDnd { enabled: !dnd.get_untracked() })
            >
                {move || view! { <Icon svg=if dnd.get() { ICON_BELL_OFF } else { ICON_BELL } /> }}
            </button>
        </div>
    }
}
