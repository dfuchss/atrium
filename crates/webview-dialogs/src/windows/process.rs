//! Failed processes of a webview: `WebView2` raises `ProcessFailed` when a process of the
//! webview's browser ends or its page stops responding. A page whose render process ended
//! shows `WebView2`'s error page until it is reloaded.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE, ICoreWebView2Controller,
};
use webview2_com::ProcessFailedEventHandler;

/// Which process of a webview failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessFailure {
    /// The render process of the page ended: it crashed, ran out of memory or was ended.
    RenderProcessExited,
    /// The page's render process stopped responding.
    RenderProcessUnresponsive,
    /// The browser process ended, and with it every webview of its user data folder.
    BrowserProcessExited,
    /// Another process (a frame's, the GPU's, a utility's), which `WebView2` starts again.
    Other,
}

/// Calls `report` with each process failure of `controller`'s webview. Call it on the main
/// thread; `report` runs there, inside `WebView2`'s event.
pub fn on_process_failed(
    controller: &ICoreWebView2Controller,
    report: impl Fn(ProcessFailure) + 'static,
) -> windows::core::Result<()> {
    let handler = ProcessFailedEventHandler::create(Box::new(move |_, event| {
        let Some(event) = event else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        // SAFETY: a COM getter of the event on the main thread, with a valid out pointer.
        unsafe { event.ProcessFailedKind(&raw mut kind) }?;
        report(failure_of(kind));
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: plain COM calls on the main thread, which owns the controller; the handler is a
    // valid COM object and the token a valid out pointer for the duration of the call.
    unsafe {
        controller
            .CoreWebView2()?
            .add_ProcessFailed(&handler, &raw mut token)
    }
}

fn failure_of(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> ProcessFailure {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => {
            ProcessFailure::RenderProcessExited
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
            ProcessFailure::RenderProcessUnresponsive
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => {
            ProcessFailure::BrowserProcessExited
        }
        _ => ProcessFailure::Other,
    }
}

#[cfg(test)]
mod tests {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
    };

    use super::*;

    #[test]
    fn failures_follow_the_webview2_kinds() {
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            ProcessFailure::RenderProcessExited
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE),
            ProcessFailure::RenderProcessUnresponsive
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            ProcessFailure::BrowserProcessExited
        );
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        ] {
            assert_eq!(failure_of(kind), ProcessFailure::Other);
        }
    }
}
