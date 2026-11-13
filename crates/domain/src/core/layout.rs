//! Placement of the sidebar and the active view, and the settings dialog that covers both.

use uuid::Uuid;

use super::{Bounds, Core, Effect};

/// Window content size in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct WindowSize {
    pub(super) width: f64,
    pub(super) height: f64,
}

impl Core {
    /// Width of the sidebar (logical px); follows the icon size setting.
    fn sidebar_width(&self) -> f64 {
        self.settings().icon_size.sidebar_width()
    }

    pub(super) fn content_bounds(&self) -> Bounds {
        let x = self.sidebar_width();
        Bounds {
            x,
            y: 0.0,
            width: (self.window.width - x).max(0.0),
            height: self.window.height,
        }
    }

    /// The sidebar webview spans the whole window while the dialog is open.
    pub(super) fn sidebar_bounds(&self) -> Bounds {
        let width = if self.dialog_open {
            self.window.width
        } else {
            self.sidebar_width()
        };
        Bounds {
            x: 0.0,
            y: 0.0,
            width,
            height: self.window.height,
        }
    }

    /// Places the sidebar and the active view (unless the dialog covers the content) after the
    /// window size or the sidebar width changed.
    pub(super) fn layout(&self) -> Vec<Effect> {
        let mut effects = vec![Effect::SetUiBounds(self.sidebar_bounds())];
        if let (Some(active), false) = (self.active, self.dialog_open)
            && self.is_loaded(active)
        {
            effects.push(Effect::SetBounds {
                account: active,
                bounds: self.content_bounds(),
            });
        }
        effects
    }

    pub(super) fn window_resized(&mut self, size: WindowSize) -> Vec<Effect> {
        self.window = size;
        self.layout()
    }

    /// True if the account's view is the one on screen.
    pub(super) fn is_visible(&self, account_id: Uuid) -> bool {
        self.active == Some(account_id) && !self.dialog_open
    }

    /// Shows the active account's view (creating it if it is not loaded), unless the dialog
    /// covers the content.
    pub(super) fn show_active(&mut self) -> Vec<Effect> {
        match (self.active, self.dialog_open) {
            (Some(active), false) if self.is_loaded(active) => vec![
                Effect::SetBounds {
                    account: active,
                    bounds: self.content_bounds(),
                },
                Effect::Show(active),
            ],
            (Some(active), false) if self.started => self.load_view(active, true),
            _ => vec![],
        }
    }

    pub(super) fn set_dialog(&mut self, open: bool) -> Vec<Effect> {
        if open == self.dialog_open {
            return vec![self.emit()];
        }
        self.dialog_open = open;
        let mut effects = vec![Effect::SetUiBounds(self.sidebar_bounds())];
        if open {
            if let Some(active) = self.active.filter(|&id| self.is_loaded(id)) {
                effects.push(Effect::Hide(active));
            }
        } else {
            effects.extend(self.show_active());
        }
        effects.push(self.emit());
        effects
    }
}
