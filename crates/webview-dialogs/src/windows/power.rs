//! Resumes from sleep: Windows calls a registered callback with `PBT_APMRESUMEAUTOMATIC` after
//! every resume from sleep or hibernation, and with `PBT_APMRESUMESUSPEND` after it once the
//! user woke the system (a key, the lid, the power button).

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerRegisterSuspendResumeNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
};

type OnResume = Box<dyn Fn() + Send + Sync>;

/// Calls `on_resume` for both notifications of each resume, on a thread of the system's, for
/// the rest of the process's life.
pub fn on_resume(on_resume: impl Fn() + Send + Sync + 'static) -> windows::core::Result<()> {
    // Leaked, like the registration: Windows calls back until the process ends.
    let on_resume: OnResume = Box::new(on_resume);
    let context: &'static OnResume = Box::leak(Box::new(on_resume));
    let parameters = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(power_event),
        Context: ptr::from_ref(context).cast_mut().cast(),
    }));
    let mut registration = ptr::null_mut();
    // SAFETY: with `DEVICE_NOTIFY_CALLBACK` the recipient points to subscribe parameters, which
    // live (leaked) as long as the process, as does the context they pass to `power_event`;
    // the registration handle is a valid out pointer for the duration of the call.
    unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            HANDLE(ptr::from_mut(parameters).cast()),
            &raw mut registration,
        )
    }
    .ok()
}

/// The callback of [`on_resume`]: `context` is its leaked [`OnResume`].
unsafe extern "system" fn power_event(context: *const c_void, kind: u32, _: *const c_void) -> u32 {
    if is_resume(kind) {
        // SAFETY: `context` is the `OnResume` that `on_resume` leaked, alive until the process
        // ends and callable from any thread (`Send + Sync`).
        let on_resume = unsafe { &*context.cast::<OnResume>() };
        on_resume();
    }
    ERROR_SUCCESS.0
}

/// Whether a power event of `kind` tells of a resume.
fn is_resume(kind: u32) -> bool {
    matches!(kind, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND)
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{PBT_APMPOWERSTATUSCHANGE, PBT_APMSUSPEND};

    use super::*;

    #[test]
    fn only_resumes_count() {
        assert!(is_resume(PBT_APMRESUMEAUTOMATIC));
        assert!(is_resume(PBT_APMRESUMESUSPEND));
        assert!(!is_resume(PBT_APMSUSPEND));
        assert!(!is_resume(PBT_APMPOWERSTATUSCHANGE));
    }
}
