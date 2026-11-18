//! Minimal bridge to `window.__TAURI__` (withGlobalTauri).

use js_sys::{Function, Promise, Reflect};
use protocol::{CMD_GET_SNAPSHOT, CMD_GET_UPDATE_STATUS, Snapshot, UpdateStatus};
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

/// Logs `error` to the console, after `context`.
pub(crate) fn log_error(context: &str, error: &JsValue) {
    web_sys::console::error_2(&JsValue::from_str(context), error);
}

async fn invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(invoke_raw(command, args)?).await
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

/// Sends a `UiCommand` or an `UpdateCommand` to the backend command `command`; errors are
/// logged to the console.
pub(crate) async fn send(command: &str, payload: impl Serialize) {
    #[derive(Serialize)]
    struct Args<P> {
        command: P,
    }
    if let Err(error) = call::<()>(command, Args { command: payload }).await {
        log_error(&format!("{command} failed"), &JsValue::from_str(&error));
    }
}

/// Calls a command without arguments; errors are logged to the console.
async fn fetch<R: DeserializeOwned>(command: &str) -> Option<R> {
    call(command, ())
        .await
        .map_err(|error| log_error(&format!("{command} failed"), &JsValue::from_str(&error)))
        .ok()
}

pub(crate) async fn get_snapshot() -> Option<Snapshot> {
    fetch(CMD_GET_SNAPSHOT).await
}

pub(crate) async fn get_update_status() -> Option<UpdateStatus> {
    fetch::<Option<UpdateStatus>>(CMD_GET_UPDATE_STATUS)
        .await
        .flatten()
}

/// Subscribes to the backend event `event` for the lifetime of the page.
pub(crate) fn listen<T: DeserializeOwned>(event: &'static str, handler: impl Fn(T) + 'static) {
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |message: JsValue| {
        match Reflect::get(&message, &JsValue::from_str("payload"))
            .and_then(|payload| serde_wasm_bindgen::from_value(payload).map_err(JsValue::from))
        {
            Ok(payload) => handler(payload),
            Err(error) => log_error(&format!("bad {event} payload"), &error),
        }
    });
    let callback: Function = closure.as_ref().unchecked_ref::<Function>().clone();
    closure.forget();
    wasm_bindgen_futures::spawn_local(async move {
        let listening = match listen_raw(event, &callback) {
            Ok(promise) => JsFuture::from(promise).await.map(|_| ()),
            Err(error) => Err(error),
        };
        if let Err(error) = listening {
            log_error("listen failed", &error);
        }
    });
}
