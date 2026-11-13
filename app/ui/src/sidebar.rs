//! The sidebar: one button per account (reorderable by dragging) and the tool buttons.

use std::time::Duration;

use leptos::prelude::*;
use protocol::{Snapshot, UiCommand};
use uuid::Uuid;
use wasm_bindgen::JsCast;

use crate::commands::send_command;
use crate::glyphs::{ICON_BELL, ICON_BELL_OFF, ICON_GEAR, ICON_PLUS};
use crate::settings::{Tab, open_dialog};
use crate::widgets::{Icon, account_icon};

/// Pointer movement (px) before a press on an icon becomes a drag.
const DRAG_THRESHOLD: f64 = 4.0;
/// How long a click right after a drag stays suppressed if no click event arrives.
const CLICK_SUPPRESSION: Duration = Duration::from_millis(50);

/// Pointer-drag bookkeeping, measured once at pointerdown so snapshots cannot disturb it.
#[derive(Clone)]
struct DragStart {
    account: Uuid,
    pointer_id: i32,
    start_y: f64,
    source: usize,
    /// Vertical centres of all account buttons, in sidebar order.
    centers: Vec<f64>,
    /// Moved past the threshold: the pointer is captured and the icons follow it.
    active: bool,
    /// Cancelled with Escape; the release must not select or move anything.
    cancelled: bool,
}

/// Visible drag state that drives the per-icon transforms.
#[derive(Clone, Copy, PartialEq)]
struct DragPreview {
    account: Uuid,
    source: usize,
    target: usize,
    offset_y: f64,
    /// Distance between two neighbouring icons.
    pitch: f64,
}

fn badge_text(unread: u32) -> String {
    if unread > 99 {
        "99+".into()
    } else {
        unread.to_string()
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a sidebar holds a handful of icons"
)]
fn as_f64(count: usize) -> f64 {
    count as f64
}

/// Positions moved from `source` to `target`, as the core's `Move` delta.
fn index_delta(source: usize, target: usize) -> i32 {
    let to_i32 = |index: usize| i32::try_from(index).unwrap_or(i32::MAX);
    to_i32(target).saturating_sub(to_i32(source))
}

/// Transform of the icon `account` at position `index` while `preview` is shown.
fn drag_style(preview: Option<DragPreview>, account: Uuid, index: usize) -> String {
    let Some(preview) = preview else {
        return String::new();
    };
    if preview.account == account {
        return format!(
            "transform: translateY({}px) scale(1.08); transition: none; z-index: 10;",
            preview.offset_y
        );
    }
    let (source, target) = (preview.source, preview.target);
    let shift = if source < target && index > source && index <= target {
        -preview.pitch
    } else if source > target && index >= target && index < source {
        preview.pitch
    } else {
        0.0
    };
    format!("transform: translateY({shift}px);")
}

/// Measures the account buttons for a press on one of them.
fn start_drag(event: &web_sys::PointerEvent, list: &web_sys::Element) -> Option<DragStart> {
    let pressed = event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .and_then(|target| target.closest(".acct").ok().flatten())?;
    let buttons = list.children();
    let mut centers = Vec::new();
    let mut source = None;
    for position in 0..buttons.length() {
        let Some(button) = buttons.item(position) else {
            continue;
        };
        let rect = button.get_bounding_client_rect();
        if button == pressed {
            source = Some(centers.len());
        }
        centers.push(rect.top() + rect.height() / 2.0);
    }
    let account = pressed
        .get_attribute("data-id")
        .and_then(|id| id.parse::<Uuid>().ok())?;
    Some(DragStart {
        account,
        pointer_id: event.pointer_id(),
        start_y: f64::from(event.client_y()),
        source: source?,
        centers,
        active: false,
        cancelled: false,
    })
}

/// Where the dragged icon is and which slot it would drop into.
fn drag_preview(start: &DragStart, offset_y: f64) -> DragPreview {
    let centers = &start.centers;
    let (first, last) = (centers[0], centers[centers.len() - 1]);
    let here = centers[start.source];
    let offset_y = offset_y.clamp(first - here, last - here);
    let dragged_center = here + offset_y;
    let target = centers
        .iter()
        .enumerate()
        .min_by(|left, right| {
            (left.1 - dragged_center)
                .abs()
                .total_cmp(&(right.1 - dragged_center).abs())
        })
        .map_or(start.source, |(index, _)| index);
    let pitch = if centers.len() > 1 {
        (last - first) / as_f64(centers.len() - 1)
    } else {
        0.0
    };
    DragPreview {
        account: start.account,
        source: start.source,
        target,
        offset_y,
        pitch,
    }
}

/// `order` with the dragged account moved to its target slot.
fn reordered(mut order: Vec<Uuid>, preview: DragPreview) -> Vec<Uuid> {
    if let Some(index) = order.iter().position(|id| *id == preview.account) {
        let moved = order.remove(index);
        order.insert(preview.target.min(order.len()), moved);
    }
    order
}

/// Drag state of the account list, shared by its pointer handlers.
#[derive(Clone, Copy)]
struct DragState {
    snapshot: ReadSignal<Snapshot>,
    preview: RwSignal<Option<DragPreview>>,
    start: StoredValue<Option<DragStart>>,
    /// Swallows the click that follows a drag.
    suppress_click: StoredValue<bool>,
    /// Rendered order; frozen while a drag is active, resynced from the snapshot afterwards.
    order: RwSignal<Vec<Uuid>>,
    list: NodeRef<leptos::html::Div>,
}

impl DragState {
    fn new(snapshot: ReadSignal<Snapshot>) -> DragState {
        let state = DragState {
            snapshot,
            preview: RwSignal::new(None),
            start: StoredValue::new(None),
            suppress_click: StoredValue::new(false),
            order: RwSignal::new(Vec::new()),
            list: NodeRef::new(),
        };
        Effect::new(move |_| {
            let ids = state.snapshot_ids();
            if state.preview.get_untracked().is_none()
                && state.order.with_untracked(|order| *order != ids)
            {
                state.order.set(ids);
            }
        });
        state
    }

    fn snapshot_ids(self) -> Vec<Uuid> {
        self.snapshot
            .with(|snapshot| snapshot.accounts.iter().map(|account| account.id).collect())
    }

    fn suppress_next_click(self) {
        self.suppress_click.set_value(true);
        // Backstop in case no click follows.
        set_timeout(
            move || self.suppress_click.set_value(false),
            CLICK_SUPPRESSION,
        );
    }

    /// Ends a drag; with `drop` the account moves to its target slot.
    fn finish(self, drop: bool) {
        let preview = self.preview.get_untracked();
        self.preview.set(None);
        let mut next = self.snapshot_ids();
        if let Some(preview) = preview {
            let moved = drop && preview.target != preview.source;
            if moved {
                // Optimistic local reorder; the next snapshot carries the same order.
                next = reordered(self.order.get_untracked(), preview);
            }
            self.suppress_next_click();
            if moved {
                send_command(UiCommand::Move {
                    account: preview.account,
                    delta: index_delta(preview.source, preview.target),
                });
            }
        }
        self.order.set(next);
    }

    fn pointer_down(self, event: &web_sys::PointerEvent) {
        if event.button() != 0 || !event.is_primary() {
            return;
        }
        event.prevent_default();
        let Some(list) = self
            .list
            .get_untracked()
            .map(|list| list.unchecked_into::<web_sys::Element>())
        else {
            return;
        };
        if let Some(start) = start_drag(event, &list) {
            self.start.set_value(Some(start));
        }
    }

    fn pointer_move(self, event: &web_sys::PointerEvent) {
        let Some(mut start) = self.start.get_value() else {
            return;
        };
        if start.cancelled || event.pointer_id() != start.pointer_id {
            return;
        }
        let offset_y = f64::from(event.client_y()) - start.start_y;
        if !start.active {
            if offset_y.abs() <= DRAG_THRESHOLD {
                return;
            }
            start.active = true;
            if let Some(list) = self.list.get_untracked() {
                let _ = list.set_pointer_capture(start.pointer_id);
            }
            self.start.set_value(Some(start.clone()));
        }
        self.preview.set(Some(drag_preview(&start, offset_y)));
    }

    fn pointer_up(self, event: &web_sys::PointerEvent) {
        let Some(start) = self.start.get_value() else {
            return;
        };
        if event.pointer_id() != start.pointer_id {
            return;
        }
        self.start.set_value(None);
        if let Some(list) = self.list.get_untracked() {
            let _ = list.release_pointer_capture(start.pointer_id);
        }
        if start.active {
            if start.cancelled {
                self.suppress_next_click();
            } else {
                self.finish(true);
            }
        }
    }

    fn pointer_cancel(self) {
        self.start.set_value(None);
        self.finish(false);
    }

    fn lost_pointer_capture(self) {
        if self.start.get_value().is_some_and(|start| start.active) {
            self.start.set_value(None);
            self.finish(false);
        }
    }

    /// Escape during a drag puts the icons back.
    fn escape(self) {
        if let Some(mut start) = self.start.get_value()
            && start.active
            && !start.cancelled
        {
            start.cancelled = true;
            self.start.set_value(Some(start));
            self.finish(false);
        }
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
    let account = Memo::new(move |_| {
        snapshot.with(|snapshot| {
            snapshot
                .accounts
                .iter()
                .find(|account| account.id == id)
                .cloned()
        })
    });
    let active = move || snapshot.with(|snapshot| snapshot.active == Some(id));
    view! {
        <button
            class="acct"
            class:active=active
            class:muted=move || account.get().is_some_and(|account| account.settings.muted)
            class:disabled=move || account.get().is_some_and(|account| !account.settings.enabled)
            class:hibernated=move || account.get().is_some_and(|account| account.settings.enabled && account.hibernated)
            title=move || account.get().map(|account| format!("{} \u{2013} {}", account.service_name, account.settings.label)).unwrap_or_default()
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
        >
            {move || account.get().map(|account| {
                let show_badge = account.settings.show_badge;
                let muted = account.settings.muted;
                let hibernated = account.settings.enabled && account.hibernated;
                view! {
                    {account_icon(&account, false)}
                    {(show_badge && account.unread > 0).then(|| view! { <span class="badge">{badge_text(account.unread)}</span> })}
                    {(show_badge && account.unread == 0 && account.indirect).then(|| view! { <span class="dot"></span> })}
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
    view! {
        <div class="bottom">
            <button class="tool" title="Add account" on:click=move |_| open_dialog(Tab::Accounts, true)><Icon svg=ICON_PLUS /></button>
            <button class="tool" title="Settings" on:click=move |_| open_dialog(Tab::General, false)><Icon svg=ICON_GEAR /></button>
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
