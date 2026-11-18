//! Reordering the account buttons by dragging them.

use std::time::Duration;

use leptos::prelude::*;
use protocol::{Snapshot, UiCommand};
use uuid::Uuid;
use wasm_bindgen::JsCast;

use crate::commands::send_command;

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
pub(super) struct DragPreview {
    pub(super) account: Uuid,
    source: usize,
    target: usize,
    offset_y: f64,
    /// Distance between two neighbouring icons.
    pitch: f64,
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
pub(super) fn drag_style(preview: Option<DragPreview>, account: Uuid, index: usize) -> String {
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
pub(super) struct DragState {
    snapshot: ReadSignal<Snapshot>,
    pub(super) preview: RwSignal<Option<DragPreview>>,
    start: StoredValue<Option<DragStart>>,
    /// Swallows the click that follows a drag.
    pub(super) suppress_click: StoredValue<bool>,
    /// Rendered order; frozen while a drag is active, resynced from the snapshot afterwards.
    pub(super) order: RwSignal<Vec<Uuid>>,
    pub(super) list: NodeRef<leptos::html::Div>,
}

impl DragState {
    pub(super) fn new(snapshot: ReadSignal<Snapshot>) -> DragState {
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

    pub(super) fn pointer_down(self, event: &web_sys::PointerEvent) {
        if event.button() != 0 || !event.is_primary() {
            return;
        }
        event.prevent_default();
        let Some(list) = self
            .list
            .get_untracked()
            .map(wasm_bindgen::JsCast::unchecked_into::<web_sys::Element>)
        else {
            return;
        };
        if let Some(start) = start_drag(event, &list) {
            self.start.set_value(Some(start));
        }
    }

    pub(super) fn pointer_move(self, event: &web_sys::PointerEvent) {
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

    pub(super) fn pointer_up(self, event: &web_sys::PointerEvent) {
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

    pub(super) fn pointer_cancel(self) {
        self.start.set_value(None);
        self.finish(false);
    }

    pub(super) fn lost_pointer_capture(self) {
        if self.start.get_value().is_some_and(|start| start.active) {
            self.start.set_value(None);
            self.finish(false);
        }
    }

    /// Escape during a drag puts the icons back.
    pub(super) fn escape(self) {
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
