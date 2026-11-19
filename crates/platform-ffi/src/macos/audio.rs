//! Muting the audio of a webview. `WKWebView` has no public API for it, only the private
//! `-[WKWebView _setPageMuted:]` (`WKWebViewPrivate.h`, Safari's tab mute), which sets the
//! page's whole `_WKMediaMutedState`. That state also mutes media capture, so the current one is
//! read (`_mediaMutedState`) and only its audio bit changed.

use objc2::runtime::{NSObjectProtocol, Sel};
use objc2::{msg_send, sel};
use objc2_web_kit::WKWebView;
use tauri::webview::PlatformWebview;

/// `_WKMediaAudioMuted` of the `NSUInteger` bitmask `_WKMediaMutedState` (`_WKMediaNoneMuted`
/// is 0). The capture devices (`1 << 1`) and the screen capture (`1 << 2`) have bits of their
/// own, which are left as they are.
const AUDIO_MUTED: usize = 1 << 0;

/// The private methods this module calls: the state's getter and setter.
fn mute_selectors() -> [Sel; 2] {
    [sel!(_mediaMutedState), sel!(_setPageMuted:)]
}

/// Mutes or unmutes the audio the page of `webview` plays (media elements and Web Audio);
/// microphone, camera and screen capture stay as they are. An error, and nothing changed, if
/// this `WebKit` lacks the private methods. Call it through `Webview::with_webview`, which runs
/// on the main thread.
pub fn set_audio_muted(webview: &PlatformWebview, muted: bool) -> Result<(), String> {
    let inner: *const WKWebView = webview.inner().cast();
    // SAFETY: `inner()` is the `WKWebView` Tauri built for this webview, alive while `webview`
    // is borrowed.
    let view = unsafe { &*inner };
    if let Some(missing) = mute_selectors()
        .into_iter()
        .find(|&selector| !view.respondsToSelector(selector))
    {
        return Err(format!("WebKit lacks -[WKWebView {missing}]"));
    }
    // SAFETY: the web view implements `_mediaMutedState` (checked above), a read-only property
    // that takes no arguments and returns a `_WKMediaMutedState`, an `NSUInteger` (`usize`);
    // `with_webview` calls this on the main thread `WKWebView` requires.
    let state: usize = unsafe { msg_send![view, _mediaMutedState] };
    let state = if muted {
        state | AUDIO_MUTED
    } else {
        state & !AUDIO_MUTED
    };
    // SAFETY: the web view implements `_setPageMuted:` (checked above), which takes one
    // `_WKMediaMutedState` and returns nothing; the value only differs from the page's current
    // state in the audio bit. On the main thread, as above.
    let () = unsafe { msg_send![view, _setPageMuted: state] };
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use objc2::ClassType;

    use super::*;

    /// `WebKit` still has the private methods, with the types [`set_audio_muted`] passes:
    /// an `NSUInteger` (`Q`) returned and taken.
    #[test]
    fn webkit_has_the_private_mute_methods() {
        let [getter, setter] = mute_selectors();
        let class = WKWebView::class();
        let getter = class.instance_method(getter).expect("_mediaMutedState");
        assert_eq!(&*getter.return_type(), c"Q" as &CStr);
        let setter = class.instance_method(setter).expect("_setPageMuted:");
        assert_eq!(&*setter.return_type(), c"v" as &CStr);
        assert_eq!(
            setter.argument_type(2).as_deref(),
            Some(c"Q" as &CStr),
            "the argument after self and _cmd"
        );
    }
}
