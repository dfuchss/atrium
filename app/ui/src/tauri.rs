//! Minimal bridge to `window.__TAURI__` (withGlobalTauri).

use js_sys::{Function, Promise, Reflect};
use protocol::{CMD_GET_SNAPSHOT, CMD_UI_COMMAND, EVENT_SNAPSHOT, Snapshot, UiCommand};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["__TAURI__", "core"], js_name = invoke, catch)]
    fn invoke_raw(command: &str, args: JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["__TAURI__", "event"], js_name = listen, catch)]
    fn listen_raw(event: &str, handler: &Function) -> Result<Promise, JsValue>;
}

pub(crate) fn available() -> bool {
    web_sys::window()
        .and_then(|window| Reflect::get(&window, &JsValue::from_str("__TAURI__")).ok())
        .is_some_and(|tauri| !tauri.is_undefined() && !tauri.is_null())
}

fn log_error(context: &str, error: &JsValue) {
    web_sys::console::error_2(&JsValue::from_str(context), error);
}

async fn invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(invoke_raw(command, args)?).await
}

/// Sends a `UiCommand`; errors are logged to the console.
pub(crate) async fn send(command: UiCommand) {
    #[derive(Serialize)]
    struct Args {
        command: UiCommand,
    }
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    let args = match (Args { command }).serialize(&serializer) {
        Ok(args) => args,
        Err(error) => return log_error("serialize ui_command", &error.into()),
    };
    if let Err(error) = invoke(CMD_UI_COMMAND, args).await {
        log_error("ui_command failed", &error);
    }
}

/// Calls a request/response command of the backend. `Err` carries the backend's message or a
/// description of the transport failure.
pub(crate) async fn call<R: DeserializeOwned>(
    command: &str,
    arguments: impl Serialize,
) -> Result<R, String> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    let arguments = arguments
        .serialize(&serializer)
        .map_err(|error| error.to_string())?;
    let payload = invoke(command, arguments).await.map_err(|error| {
        error
            .as_string()
            .unwrap_or_else(|| format!("{command} failed: {error:?}"))
    })?;
    serde_wasm_bindgen::from_value(payload).map_err(|error| error.to_string())
}

pub(crate) async fn get_snapshot() -> Option<Snapshot> {
    match invoke(CMD_GET_SNAPSHOT, JsValue::UNDEFINED).await {
        Ok(payload) => parse(payload),
        Err(error) => {
            log_error("get_snapshot failed", &error);
            None
        }
    }
}

fn parse(payload: JsValue) -> Option<Snapshot> {
    match serde_wasm_bindgen::from_value(payload) {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            log_error("bad snapshot payload", &error.into());
            None
        }
    }
}

/// Subscribes to snapshot events for the lifetime of the page.
pub(crate) fn listen_snapshots(on_snapshot: impl Fn(Snapshot) + 'static) {
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        if let Ok(payload) = Reflect::get(&event, &JsValue::from_str("payload"))
            && let Some(snapshot) = parse(payload)
        {
            on_snapshot(snapshot);
        }
    });
    let callback: Function = handler.as_ref().unchecked_ref::<Function>().clone();
    handler.forget();
    wasm_bindgen_futures::spawn_local(async move {
        match listen_raw(EVENT_SNAPSHOT, &callback) {
            Ok(promise) => {
                if let Err(error) = JsFuture::from(promise).await {
                    log_error("listen failed", &error);
                }
            }
            Err(error) => log_error("listen failed", &error),
        }
    });
}
