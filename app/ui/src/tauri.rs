//! Minimal bridge to `window.__TAURI__` (withGlobalTauri).
use js_sys::{Function, Promise, Reflect};
use protocol::{CMD_GET_SNAPSHOT, CMD_UI_COMMAND, EVENT_SNAPSHOT, Snapshot, UiCommand};
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["__TAURI__", "core"], js_name = invoke, catch)]
    fn invoke_raw(cmd: &str, args: JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["__TAURI__", "event"], js_name = listen, catch)]
    fn listen_raw(event: &str, handler: &Function) -> Result<Promise, JsValue>;
}

pub fn available() -> bool {
    web_sys::window()
        .and_then(|w| Reflect::get(&w, &JsValue::from_str("__TAURI__")).ok())
        .is_some_and(|v| !v.is_undefined() && !v.is_null())
}

fn log_err(what: &str, e: &JsValue) {
    web_sys::console::error_2(&JsValue::from_str(what), e);
}

async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(invoke_raw(cmd, args)?).await
}

/// Send a `UiCommand`; errors are logged to the console.
pub async fn send(cmd: UiCommand) {
    #[derive(Serialize)]
    struct Args {
        cmd: UiCommand,
    }
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    let args = match (Args { cmd }).serialize(&ser) {
        Ok(a) => a,
        Err(e) => return log_err("serialize ui_command", &e.into()),
    };
    if let Err(e) = invoke(CMD_UI_COMMAND, args).await {
        log_err("ui_command failed", &e);
    }
}

pub async fn get_snapshot() -> Option<Snapshot> {
    let v = match invoke(CMD_GET_SNAPSHOT, JsValue::UNDEFINED).await {
        Ok(v) => v,
        Err(e) => {
            log_err("get_snapshot failed", &e);
            return None;
        }
    };
    parse(v)
}

fn parse(v: JsValue) -> Option<Snapshot> {
    match serde_wasm_bindgen::from_value(v) {
        Ok(s) => Some(s),
        Err(e) => {
            log_err("bad snapshot payload", &e.into());
            None
        }
    }
}

/// Subscribe to snapshot events for the lifetime of the page.
pub fn listen_snapshots(on: impl Fn(Snapshot) + 'static) {
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
        if let Ok(p) = Reflect::get(&e, &JsValue::from_str("payload"))
            && let Some(s) = parse(p)
        {
            on(s);
        }
    });
    let f: Function = handler.as_ref().unchecked_ref::<Function>().clone();
    handler.forget();
    wasm_bindgen_futures::spawn_local(async move {
        match listen_raw(EVENT_SNAPSHOT, &f) {
            Ok(p) => {
                if let Err(e) = JsFuture::from(p).await {
                    log_err("listen failed", &e);
                }
            }
            Err(e) => log_err("listen failed", &e),
        }
    });
}
