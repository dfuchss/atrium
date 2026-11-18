//! Editor for one service definition file: a plain text area, live validation by the backend
//! (the same parser as the loader) and inline errors with line numbers.

use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use protocol::ServiceProblem;

use crate::service_api;
use crate::widgets::{ErrorNote, Row, TextField, no_autocorrect};

/// How long typing must pause before the text is validated.
const VALIDATE_DELAY: Duration = Duration::from_millis(400);
/// File name offered for a new definition.
const DEFAULT_NAME: &str = "myservice.toml";

/// What the editor opens: an existing file (`name`) or a new one (`None`, named in the editor).
#[derive(Clone)]
pub(super) struct EditTarget {
    pub(super) name: Option<String>,
    pub(super) text: String,
}

/// Validates the current text now and stores the result if the text did not change meanwhile.
fn validate_now(text: RwSignal<String>, problems: RwSignal<Vec<ServiceProblem>>) {
    let checked = text.get_untracked();
    spawn_local(async move {
        let found = service_api::validate(&checked).await;
        if text.get_untracked() == checked {
            problems.set(found);
        }
    });
}

#[component]
pub(super) fn ServiceEditor(target: EditTarget, on_close: Callback<()>) -> impl IntoView {
    let creating = target.name.is_none();
    let file_name = RwSignal::new(target.name.unwrap_or_else(|| DEFAULT_NAME.into()));
    let text = RwSignal::new(target.text);
    let problems: RwSignal<Vec<ServiceProblem>> = RwSignal::new(vec![]);
    let failure: RwSignal<Option<String>> = RwSignal::new(None);
    let edit_count = RwSignal::new(0u32);
    validate_now(text, problems);

    let on_input = move |event| {
        text.set(event_target_value(&event));
        failure.set(None);
        let edit = edit_count.get_untracked() + 1;
        edit_count.set(edit);
        set_timeout(
            move || {
                if edit_count.get_untracked() == edit {
                    validate_now(text, problems);
                }
            },
            VALIDATE_DELAY,
        );
    };
    let save = move |_| {
        let (name, content) = (file_name.get_untracked(), text.get_untracked());
        spawn_local(async move {
            match service_api::save(&name, &content, creating).await {
                Ok(found) if found.is_empty() => on_close.run(()),
                Ok(found) => {
                    failure.set(Some("Not saved: fix the errors first.".into()));
                    problems.set(found);
                }
                Err(message) => failure.set(Some(message)),
            }
        });
    };
    view! {
        <div class="edit form custom-form">
            <h3>{if creating { "New service definition" } else { "Edit service definition" }}</h3>
            <Row label="File name" hint="Letters a-z, digits, - and _, ending in .toml.">
                {if creating {
                    view! { <TextField value=file_name class="mono" /> }.into_any()
                } else {
                    view! { <code>{file_name.get_untracked()}</code> }.into_any()
                }}
            </Row>
            <textarea class="mono toml-editor" rows="24" spellcheck="false" autocapitalize="off" autocomplete="off" wrap="off"
                prop:value=move || text.get() on:input=on_input use:no_autocorrect></textarea>
            <Problems problems=problems text=text />
            <ErrorNote message=failure />
            <div class="actions">
                <button class="btn primary" disabled=move || file_name.with(String::is_empty) on:click=save>"Save"</button>
                <button class="btn" on:click=move |_| on_close.run(())>"Cancel"</button>
            </div>
        </div>
    }
}

/// The problems under the editor: position, message and the offending line.
#[component]
fn Problems(problems: RwSignal<Vec<ServiceProblem>>, text: RwSignal<String>) -> impl IntoView {
    view! {
        {move || {
            let found = problems.get();
            if found.is_empty() {
                return view! { <div class="note ok">"Valid definition."</div> }.into_any();
            }
            let items = found.into_iter().map(|problem| {
                let source = problem.line.and_then(|line| {
                    text.with(|text| text.lines().nth(usize::try_from(line).ok()?.checked_sub(1)?).map(str::to_string))
                });
                let position = match (problem.line, problem.column) {
                    (Some(line), Some(column)) => format!("Line {line}, column {column}: "),
                    (Some(line), None) => format!("Line {line}: "),
                    _ => String::new(),
                };
                view! {
                    <div class="problem">
                        <div><strong>{position}</strong>{problem.message}</div>
                        {source.map(|source| view! { <code class="source">{source}</code> })}
                    </div>
                }
            }).collect_view();
            view! { <div class="note error problems">{items}</div> }.into_any()
        }}
    }
}
