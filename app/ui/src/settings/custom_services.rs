//! Custom services tab: user-defined services and the service definition folder.

use leptos::prelude::*;
use protocol::{CustomService, ServiceInfo, Snapshot, UiCommand};

use super::UiState;
use super::account_form::user_agent_mode;
use super::service_files::ServiceFiles;
use crate::commands::send_command;
use crate::widgets::{ConfirmButton, Row, no_autocorrect, non_empty};

/// Color of a new custom service.
const DEFAULT_COLOR: &str = "#6366f1";
/// Shown instead of a color the color input cannot display.
const FALLBACK_COLOR: &str = "#888888";

/// `None` = form closed, `Some(None)` = adding, `Some(Some(_))` = editing.
type CustomDraft = Option<Option<CustomService>>;

/// The color if the color input can show it (`#rrggbb`), else a neutral grey.
fn displayable_color(color: String) -> String {
    if color.len() == 7 && color.starts_with('#') {
        color
    } else {
        FALLBACK_COLOR.into()
    }
}

#[component]
pub(super) fn CustomServicesTab(snapshot: ReadSignal<Snapshot>) -> impl IntoView {
    let ui = expect_context::<UiState>();
    let initial: CustomDraft = ui.add_custom.get_untracked().then_some(None);
    ui.add_custom.set(false);
    let form: RwSignal<CustomDraft> = RwSignal::new(initial);
    let services = Memo::new(move |_| snapshot.with(|snapshot| snapshot.custom_services.clone()));
    view! {
        <h2>"Custom services"</h2>
        <p class="note">"Add any web app or site as a service. It then shows up in the service list when adding an account. A service can only be removed when no account uses it."</p>
        <Show
            when=move || !services.with(Vec::is_empty)
            fallback=|| view! { <p class="empty">"No custom services yet."</p> }
        >
            <section class="list">
                <For
                    each=move || services.get()
                    key=|service| (service.id.clone(), service.name.clone(), service.url.clone(), service.color.clone(), service.instance_label.clone(), service.title_regex.clone(), service.user_agent.clone())
                    children=move |service| view! { <CustomServiceRow snapshot=snapshot service=service form=form /> }
                />
            </section>
        </Show>
        {move || match form.get() {
            None => view! { <button class="btn primary" on:click=move |_| form.set(Some(None))>"Add custom service\u{2026}"</button> }.into_any(),
            Some(existing) => view! { <CustomServiceForm snapshot=snapshot existing=existing form=form /> }.into_any(),
        }}
        <ServiceFiles snapshot=snapshot />
    }
}

#[component]
fn CustomServiceRow(
    snapshot: ReadSignal<Snapshot>,
    service: CustomService,
    form: RwSignal<CustomDraft>,
) -> impl IntoView {
    let edited = service.clone();
    let (removed_id, used_id) = (service.id.clone(), service.id.clone());
    let users = Memo::new(move |_| {
        snapshot.with(|snapshot| {
            snapshot
                .accounts
                .iter()
                .filter(|account| account.service == used_id)
                .count()
        })
    });
    let initial = service
        .name
        .chars()
        .next()
        .map_or_default(|first| first.to_uppercase().to_string());
    view! {
        <div class="row">
            <span class="icon small" style=format!("background:{}", service.color)>{initial}</span>
            <span class="names"><strong>{service.name.clone()}</strong><span class="sub">{service.url.clone()}</span></span>
            <button class="btn" on:click=move |_| form.set(Some(Some(edited.clone())))>"Edit"</button>
            <ConfirmButton
                label="Remove"
                confirm_label="Confirm remove"
                disabled=Signal::derive(move || users.get() > 0)
                title=Signal::derive(move || match users.get() {
                    0 => String::new(),
                    count => format!("In use by {count} account(s)"),
                })
                on_confirm=move |()| send_command(UiCommand::RemoveCustomService { id: removed_id.clone() })
            />
        </div>
    }
}

/// Editable copy of a custom service, one signal per field.
#[derive(Clone, Copy)]
struct CustomServiceDraft {
    name: RwSignal<String>,
    url: RwSignal<String>,
    color: RwSignal<String>,
    instance_label: RwSignal<String>,
    title_regex: RwSignal<String>,
    user_agent_mode: RwSignal<String>,
    user_agent_custom: RwSignal<String>,
}

impl CustomServiceDraft {
    fn new(service: &CustomService) -> CustomServiceDraft {
        let (mode, custom) = user_agent_mode(service.user_agent.as_deref(), "safari");
        CustomServiceDraft {
            name: RwSignal::new(service.name.clone()),
            url: RwSignal::new(service.url.clone()),
            color: RwSignal::new(displayable_color(service.color.clone())),
            instance_label: RwSignal::new(service.instance_label.clone().unwrap_or_default()),
            title_regex: RwSignal::new(service.title_regex.clone().unwrap_or_default()),
            user_agent_mode: RwSignal::new(mode),
            user_agent_custom: RwSignal::new(custom),
        }
    }

    /// Fills the fields from a built-in service, or clears them for `None`.
    fn prefill(self, template: Option<ServiceInfo>) {
        let template = template.unwrap_or_else(|| ServiceInfo {
            color: DEFAULT_COLOR.into(),
            ..ServiceInfo::default()
        });
        let name = if template.name.is_empty() {
            String::new()
        } else {
            format!("{} (custom)", template.name)
        };
        let (mode, custom) = user_agent_mode(template.user_agent.as_deref(), "safari");
        self.name.set(name);
        self.url.set(template.url);
        self.color.set(displayable_color(template.color));
        self.instance_label
            .set(template.instance_label.unwrap_or_default());
        self.title_regex
            .set(template.title_regex.unwrap_or_default());
        self.user_agent_mode.set(mode);
        self.user_agent_custom.set(custom);
    }

    fn is_valid(self) -> bool {
        !self.name.with(|name| name.trim().is_empty())
            && !self.url.with(|url| url.trim().is_empty())
    }

    fn to_service(self, id: String) -> CustomService {
        let url = self.url.get_untracked().trim().to_string();
        CustomService {
            id,
            name: self.name.get_untracked().trim().to_string(),
            color: self.color.get_untracked(),
            instance_label: non_empty(&self.instance_label.get_untracked())
                .filter(|_| url.contains("{instance}")),
            url,
            title_regex: non_empty(&self.title_regex.get_untracked()),
            user_agent: match self.user_agent_mode.get_untracked().as_str() {
                "safari" => None,
                "custom" => non_empty(&self.user_agent_custom.get_untracked()),
                preset => Some(preset.to_string()),
            },
        }
    }
}

#[component]
fn CustomServiceForm(
    snapshot: ReadSignal<Snapshot>,
    existing: Option<CustomService>,
    form: RwSignal<CustomDraft>,
) -> impl IntoView {
    let editing = existing.is_some();
    let service = existing.unwrap_or_else(|| CustomService {
        id: String::new(),
        name: String::new(),
        url: String::new(),
        color: DEFAULT_COLOR.into(),
        instance_label: None,
        title_regex: None,
        user_agent: None,
    });
    let id = service.id.clone();
    let draft = CustomServiceDraft::new(&service);
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if !draft.is_valid() {
            return;
        }
        let service = draft.to_service(id.clone());
        send_command(if editing {
            UiCommand::UpdateCustomService { service }
        } else {
            UiCommand::AddCustomService { service }
        });
        form.set(None);
    };
    view! {
        <form class="edit form custom-form" novalidate on:submit=save>
            <h3>{if editing { "Edit custom service" } else { "Add custom service" }}</h3>
            {(!editing).then(|| view! { <TemplateRow snapshot=snapshot draft=draft /> })}
            <CustomServiceFields draft=draft />
            <Row label="">
                <div class="actions">
                    <button class="btn primary" type="submit" disabled=move || !draft.is_valid()>{if editing { "Save" } else { "Add" }}</button>
                    <button class="btn" type="button" on:click=move |_| form.set(None)>"Cancel"</button>
                </div>
            </Row>
        </form>
    }
}

/// Prefill from a built-in service (adding only); "Blank" resets the fields.
#[component]
fn TemplateRow(snapshot: ReadSignal<Snapshot>, draft: CustomServiceDraft) -> impl IntoView {
    let template = RwSignal::new(String::new());
    let builtin = Memo::new(move |_| {
        snapshot.with(|snapshot| {
            snapshot
                .services
                .iter()
                .filter(|service| !service.custom)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let choose =
        move |id: String| {
            template.set(id.clone());
            draft.prefill(builtin.with_untracked(|services| {
                services.iter().find(|service| service.id == id).cloned()
            }));
        };
    view! {
        <Row label="Start from" hint="Prefill the fields from a built-in service, then adjust them.">
            <select prop:value=move || template.get() on:change=move |event| choose(event_target_value(&event))>
                <option value="">"Blank"</option>
                <For each=move || builtin.get() key=|service| service.id.clone() children=|service| view! { <option value=service.id.clone()>{service.name.clone()}</option> } />
            </select>
        </Row>
    }
}

#[component]
fn CustomServiceFields(draft: CustomServiceDraft) -> impl IntoView {
    view! {
        <Row label="Name">
            <input type="text" spellcheck="false" prop:value=move || draft.name.get() on:input=move |event| draft.name.set(event_target_value(&event)) />
        </Row>
        <Row label="URL" hint="Use {instance} for the part that differs per account, e.g. the server host of a self-hosted service (https://{instance}/) or a workspace subdomain. Each account then asks for it.">
            <input type="url" class="wide" placeholder="https://example.com/"
                autocapitalize="off" spellcheck="false" autocomplete="off"
                prop:value=move || draft.url.get() on:input=move |event| draft.url.set(event_target_value(&event)) use:no_autocorrect />
        </Row>
        <Row label="Color">
            <input type="color" prop:value=move || draft.color.get() on:input=move |event| draft.color.set(event_target_value(&event)) />
        </Row>
        {move || draft.url.with(|url| url.contains("{instance}")).then(|| view! {
            <Row label="Instance field label" hint="Optional, e.g. Instance.">
                <input type="text" placeholder="e.g. Instance" autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || draft.instance_label.get() on:input=move |event| draft.instance_label.set(event_target_value(&event)) use:no_autocorrect />
            </Row>
        })}
        <Row label="Unread title pattern" hint="Optional regular expression on the page title; capture group 1 is the unread count.">
            <input type="text" class="wide mono" placeholder="^\\((\\d+)\\)"
                autocapitalize="off" spellcheck="false" autocomplete="off"
                prop:value=move || draft.title_regex.get() on:input=move |event| draft.title_regex.set(event_target_value(&event)) use:no_autocorrect />
        </Row>
        <Row label="User agent">
            <select prop:value=move || draft.user_agent_mode.get() on:change=move |event| draft.user_agent_mode.set(event_target_value(&event))>
                <option value="safari">"Safari (default)"</option>
                <option value="chrome">"Chrome"</option>
                <option value="default">"System WebKit"</option>
                <option value="custom">"Custom\u{2026}"</option>
            </select>
        </Row>
        {move || (draft.user_agent_mode.get() == "custom").then(|| view! {
            <Row label="Custom user agent">
                <input type="text" class="wide" autocapitalize="off" spellcheck="false" autocomplete="off"
                    prop:value=move || draft.user_agent_custom.get() on:input=move |event| draft.user_agent_custom.set(event_target_value(&event)) use:no_autocorrect />
            </Row>
        })}
    }
}
