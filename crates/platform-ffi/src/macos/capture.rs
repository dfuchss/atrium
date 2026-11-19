//! Whether a webview's page captures the microphone or camera (a call): the public
//! `microphoneCaptureState` and `cameraCaptureState` of `WKWebView` (macOS 12+), observed with
//! key-value observing. A muted capture counts as capturing: the call goes on.
//!
//! The observer lives as long as the web view (an associated object) and is never removed:
//! `WKWebView` notifies these keys automatically (`+automaticallyNotifiesObserversForKey:` is
//! YES), and for such objects KVO drops the observations when the object is deallocated
//! (macOS 10.13+), so the observer never outlives what it observes while registered.

use std::ffi::c_void;
use std::ptr;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, ffi, msg_send};
use objc2_foundation::{
    NSKeyValueObservingOptions, NSObjectNSKeyValueObserverRegistration, NSString,
};
use objc2_web_kit::{WKMediaCaptureState, WKWebView};
use tauri::webview::PlatformWebview;

/// The observed keys.
const CAPTURE_KEYS: [&str; 2] = ["microphoneCaptureState", "cameraCaptureState"];

/// Key of the observer's association with its web view; only its address matters.
static OBSERVER_KEY: u8 = 0;

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and the class does not implement
    // `dealloc`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AtriumCaptureObserver"]
    #[ivars = Box<dyn Fn(bool)>]
    struct CaptureObserver;

    impl CaptureObserver {
        // SAFETY: the signature of `-[NSObject observeValueForKeyPath:ofObject:change:context:]`.
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            if let Some(view) = object.and_then(AnyObject::downcast_ref::<WKWebView>) {
                (self.ivars())(capturing(view));
            }
        }
    }
);

impl CaptureObserver {
    fn new(mtm: MainThreadMarker, on_change: Box<dyn Fn(bool)>) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(on_change);
        // SAFETY: `init` of `NSObject` on an allocated, ivar-initialized instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// True while the page captures the microphone or camera, muted or not.
fn capturing(view: &WKWebView) -> bool {
    // SAFETY: plain property reads (macOS 12+, Atrium's minimum is 14) on the main thread, where
    // the observer and `observe_capture` run.
    let (microphone, camera) =
        unsafe { (view.microphoneCaptureState(), view.cameraCaptureState()) };
    microphone != WKMediaCaptureState::None || camera != WKMediaCaptureState::None
}

/// Calls `on_change` on the main thread with whether `webview`'s page captures the microphone
/// or camera on each change of either, repeats possible, for as long as the web view lives. Call it once per web view (a
/// second call would release the first observer while it is still registered), through
/// `Webview::with_webview`, which runs on the main thread; an error off it.
pub fn observe_capture(
    webview: &PlatformWebview,
    on_change: impl Fn(bool) + 'static,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let inner: *const WKWebView = webview.inner().cast();
    // SAFETY: `inner()` is the `WKWebView` Tauri built for this webview, alive while `webview`
    // is borrowed.
    let view = unsafe { &*inner };
    let observer = CaptureObserver::new(mtm, Box::new(on_change));
    for key in CAPTURE_KEYS {
        // SAFETY: `observer` implements `observeValueForKeyPath:ofObject:change:context:` and
        // lives as long as the web view (associated below); the key path is a valid string and
        // the context may be null.
        unsafe {
            view.addObserver_forKeyPath_options_context(
                &observer,
                &NSString::from_str(key),
                NSKeyValueObservingOptions::empty(),
                ptr::null_mut(),
            );
        }
    }
    // SAFETY: the web view is alive (see above); the association retains the observer, so it
    // lives as long as the web view, and the static key is unique to this module.
    unsafe {
        ffi::objc_setAssociatedObject(
            ptr::from_ref(view).cast_mut().cast(),
            ptr::from_ref(&OBSERVER_KEY).cast(),
            Retained::as_ptr(&observer).cast_mut().cast(),
            ffi::OBJC_ASSOCIATION_RETAIN,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use objc2::ClassType;

    use super::*;

    /// The relaxed unregistration this module relies on holds only for keys `WebKit` notifies
    /// automatically.
    #[test]
    fn webkit_notifies_the_capture_keys_automatically() {
        for key in CAPTURE_KEYS {
            // SAFETY: a class method taking any key string, returning a BOOL.
            let automatic: bool = unsafe {
                msg_send![
                    WKWebView::class(),
                    automaticallyNotifiesObserversForKey: &*NSString::from_str(key)
                ]
            };
            assert!(automatic, "{key}");
        }
    }
}
