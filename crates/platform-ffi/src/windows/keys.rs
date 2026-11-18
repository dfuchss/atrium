//! Key presses of a webview that the app may take before the page: `WebView2` hands the keys
//! with Ctrl or Alt (and the function keys) to its `AcceleratorKeyPressed` event first.

use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, ICoreWebView2Controller,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
pub use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_ADD, VK_NUMPAD0, VK_OEM_4, VK_OEM_6, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PLUS,
    VK_SUBTRACT,
};

/// A key pressed down in a webview, with the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: VIRTUAL_KEY,
    pub ctrl: bool,
    pub shift: bool,
    /// Alt, also held for `AltGr` (which Windows reports as Ctrl+Alt).
    pub alt: bool,
}

/// Calls `take` with each key pressed down in `controller`'s webview that `WebView2` offers
/// the app; a key `take` returns true for goes no further, neither to the page nor to
/// `WebView2`'s own shortcuts. Call it on the main thread; `take` runs there, inside
/// `WebView2`'s event, so it must neither block nor touch windows.
pub fn on_key_down(
    controller: &ICoreWebView2Controller,
    take: impl Fn(KeyPress) -> bool + 'static,
) -> windows::core::Result<()> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, event| {
        let Some(event) = event else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        let mut key = 0;
        // SAFETY: COM getters of the event on the main thread, with valid out pointers.
        unsafe {
            event.KeyEventKind(&raw mut kind)?;
            event.VirtualKey(&raw mut key)?;
        }
        let Ok(key) = u16::try_from(key) else {
            return Ok(());
        };
        if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && take(with_modifiers(VIRTUAL_KEY(key))) {
            // SAFETY: a COM setter of the event on the main thread, before the handler returns.
            unsafe { event.SetHandled(true) }?;
        }
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: a plain COM call on the main thread, which owns the controller; the handler is a
    // valid COM object and the token a valid out pointer for the duration of the call.
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &raw mut token) }
}

/// `key` with the modifiers held while it was pressed, read from the thread's keyboard state
/// as `WebView2`'s own samples do: the event comes before the thread reads its next message.
fn with_modifiers(key: VIRTUAL_KEY) -> KeyPress {
    // SAFETY: `GetKeyState` only reads the calling thread's keyboard state.
    let held = |modifier: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(modifier.0)) } < 0;
    KeyPress {
        key,
        ctrl: held(VK_CONTROL),
        shift: held(VK_SHIFT),
        alt: held(VK_MENU),
    }
}
