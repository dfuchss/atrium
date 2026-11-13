//! JavaScript snippets the core asks the adapter to inject or evaluate in service views.

use crate::catalog::ServiceDef;
use crate::model::Account;

/// Picks the best site icon: an SVG `rel=icon` if present, otherwise the largest raster icon
/// (from `sizes` or a `NNxNN` in the file name) among `rel=icon` and apple-touch icons, falling
/// back to `/favicon.ico`. Returns an absolute URL (or null).
pub const ICON_SCRIPT: &str = r#"(()=>{try{const abs=h=>new URL(h,document.baseURI).href;let best=null,score=-1;for(const l of document.querySelectorAll('link[rel~="icon"][href],link[rel~="apple-touch-icon"][href],link[rel~="apple-touch-icon-precomposed"][href]')){const href=l.getAttribute('href');const rel=(l.getAttribute('rel')||'').toLowerCase();const touch=rel.includes('apple-touch');const z=(l.getAttribute('sizes')||'').toLowerCase();const svg=!touch&&((l.getAttribute('type')||'').includes('svg')||/\.svg(\?|$)/i.test(href)||z==='any');let s=0;if(svg)s=1e6;else{for(const p of z.split(/\s+/)){const n=parseInt(p,10);if(n>s)s=n;}if(!s){const m=href.match(/(\d{2,4})x\d{2,4}/);s=m?parseInt(m[1],10):(touch?180:16);}}if(!touch)s+=0.5;if(s>score){score=s;best=href;}}if(best)return abs(best);return location.origin+'/favicon.ico';}catch(e){return null;}})()"#;

pub(super) const HISTORY_BACK: &str = "history.back()";
pub(super) const HISTORY_FORWARD: &str = "history.forward()";

/// Init script of an account's views: the service's storage cleaner, the notification shim and
/// the account's custom CSS.
pub(super) fn init_script(service: &ServiceDef, account: &Account) -> String {
    let mut script = String::new();
    if !service.clear_local_storage.is_empty() {
        script.push_str(&storage_cleaner(&service.clear_local_storage));
        script.push('\n');
    }
    script.push_str(crate::NOTIFY_SHIM);
    if let Some(css) = account
        .custom_css
        .as_deref()
        .filter(|css| !css.trim().is_empty())
    {
        script.push('\n');
        script.push_str(&css_injector(css));
    }
    script
}

/// Drains the shim's notification queue and evaluates the service's unread expression, both
/// returned as one JSON string.
pub(super) fn poll_script(unread_expression: Option<&str>) -> String {
    let expression = unread_expression.unwrap_or("null");
    format!(
        "(()=>{{const m=window.__msgr;let u=null;try{{u=({expression})}}catch(e){{}}return JSON.stringify({{n:m?m.drain():[],u:u}});}})()"
    )
}

/// Forwards a click on a native notification to the page's notification object.
pub(super) fn notification_click_script(shim_id: u64) -> String {
    format!("window.__msgr&&window.__msgr.click({shim_id})")
}

/// Removes the given `localStorage` keys at document start (before page scripts run).
fn storage_cleaner(keys: &[String]) -> String {
    let keys = json_string_array(keys);
    format!("(()=>{{try{{for(const k of {keys})localStorage.removeItem(k);}}catch(e){{}}}})();")
}

/// Adds `css` as a `<style>` element at document start, and again after `DOMContentLoaded` if
/// the page dropped it.
fn css_injector(css: &str) -> String {
    let css = json_string(css);
    format!(
        "(()=>{{const css={css};const add=()=>{{if(document.getElementById('__msgr_css'))return;const s=document.createElement('style');s.id='__msgr_css';s.textContent=css;(document.head||document.documentElement).appendChild(s);}};if(document.documentElement)add();document.addEventListener('DOMContentLoaded',add);}})();"
    )
}

/// `text` as a JavaScript string literal.
fn json_string(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// `items` as a JavaScript array literal of strings.
fn json_string_array(items: &[String]) -> String {
    serde_json::Value::from(items.to_vec()).to_string()
}
