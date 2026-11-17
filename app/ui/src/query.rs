//! The page's query string: demo mode and the screenshot aids ("Debug options" in the wiki).

use crate::tauri;

fn query_params() -> Vec<(String, String)> {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (pair.to_string(), String::new()),
        })
        .collect()
}

pub(crate) fn query_param(key: &str) -> Option<String> {
    query_params()
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// `?demo` wins over a present `window.__TAURI__`.
pub(crate) fn is_demo() -> bool {
    query_param("demo").is_some()
}

/// Talk to the Rust side only when not in demo mode.
pub(crate) fn use_tauri() -> bool {
    !is_demo() && tauri::available()
}
