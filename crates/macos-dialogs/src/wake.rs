//! Display wakes, the sign of a wake the user sees: a dark wake (Power Nap) leaves the displays
//! off and posts no `NSWorkspaceScreensDidWakeNotification`.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceScreensDidWakeNotification};
use objc2_foundation::NSNotification;

/// Calls `on_wake` each time the displays wake, until the app exits.
pub fn observe_screens_wake(on_wake: impl Fn() + Send + Sync + 'static) {
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| on_wake());
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is an AppKit constant that lives for the whole process. No object filter
    // is given, and without a queue the block runs on the posting thread, which is fine because
    // it is `Send + Sync`.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceScreensDidWakeNotification),
            None,
            None,
            &block,
        )
    };
    // Never removed: the observation lasts as long as the app.
    std::mem::forget(observer);
}
