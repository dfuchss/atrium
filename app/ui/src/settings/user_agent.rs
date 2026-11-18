//! The user agent choice of an account or custom service: a preset or custom text.

use leptos::prelude::*;

use crate::platform_texts;
use crate::widgets::{Row, TextField, non_empty};

/// Mode of the select that means "use the custom text".
const CUSTOM: &str = "custom";

/// Editable user agent: the select's mode and the custom text.
///
/// `unset_mode` is the mode that stands for "no user agent stored": `""` ("Service default")
/// for an account, `"webkit"` for a custom service, which has no service default to fall back to.
#[derive(Clone, Copy)]
pub(super) struct UserAgentDraft {
    mode: RwSignal<String>,
    custom: RwSignal<String>,
}

impl UserAgentDraft {
    pub(super) fn new(stored: Option<&str>, unset_mode: &str) -> UserAgentDraft {
        let draft = UserAgentDraft {
            mode: RwSignal::new(String::new()),
            custom: RwSignal::new(String::new()),
        };
        draft.set(stored, unset_mode);
        draft
    }

    /// Shows the stored user agent.
    pub(super) fn set(self, stored: Option<&str>, unset_mode: &str) {
        let (mode, custom) = match stored {
            None => (unset_mode, ""),
            Some(preset @ ("webkit" | "chrome" | "default")) => (preset, ""),
            Some(literal) => (CUSTOM, literal),
        };
        self.mode.set(mode.to_string());
        self.custom.set(custom.to_string());
    }

    /// The user agent to store, `None` for the unset mode.
    pub(super) fn value(self, unset_mode: &str) -> Option<String> {
        match self.mode.get_untracked().as_str() {
            mode if mode == unset_mode => None,
            CUSTOM => non_empty(&self.custom.get_untracked()),
            preset => Some(preset.to_string()),
        }
    }
}

/// The select of the user agent and, for "Custom...", the row of its text.
#[component]
pub(super) fn UserAgentRows(
    draft: UserAgentDraft,
    unset_mode: &'static str,
    #[prop(optional)] hint: &'static str,
) -> impl IntoView {
    view! {
        <Row label="User agent" hint=hint>
            <select prop:value=move || draft.mode.get() on:change=move |event| draft.mode.set(event_target_value(&event))>
                {unset_mode.is_empty().then(|| view! { <option value="">"Service default"</option> })}
                <option value="webkit">{if unset_mode == "webkit" { "WebKit browser (default)" } else { "WebKit browser" }}</option>
                <option value="chrome">"Chrome"</option>
                <option value="default">{platform_texts::texts().system_user_agent}</option>
                <option value=CUSTOM>"Custom\u{2026}"</option>
            </select>
        </Row>
        {move || (draft.mode.get() == CUSTOM).then(|| view! {
            <Row label="Custom user agent">
                <TextField value=draft.custom class="wide" />
            </Row>
        })}
    }
}
