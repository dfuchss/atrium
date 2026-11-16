//! Resumes from suspend: systemd-logind (or elogind) sends `PrepareForSleep(true)` on the system
//! bus before a suspend and `PrepareForSleep(false)` after the resume. Unlike a Power Nap on
//! macOS, a Linux resume means the user is back.

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::names::BusName;

use crate::Logger;
use crate::logger::spawn_logged;

const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";

/// Calls `on_resume` after each resume from suspend, on a thread of its own that waits for
/// logind's signal until the app exits. Without the system bus or logind it logs one line and
/// never calls.
pub(crate) fn observe_resume(logger: &Logger, on_resume: impl Fn() + Send + Sync + 'static) {
    let thread_logger = logger.clone();
    spawn_logged("resume", logger, move || {
        if let Err(error) = watch_prepare_for_sleep(&thread_logger, &on_resume) {
            thread_logger.log(&format!("no resume signal: {error}"));
        }
    });
}

/// Blocks on logind's `PrepareForSleep` signals and calls `on_resume` for each `false` (the
/// system resumed). Returns when there is no logind or the bus connection ends, logging why.
fn watch_prepare_for_sleep(logger: &Logger, on_resume: &impl Fn()) -> zbus::Result<()> {
    let connection = Connection::system()?;
    if !DBusProxy::new(&connection)?.name_has_owner(BusName::try_from(LOGIND)?)? {
        logger.log("no resume signal: logind is not running");
        return Ok(());
    }
    let manager = Proxy::new(&connection, LOGIND, LOGIND_PATH, LOGIND_MANAGER)?;
    let signals = manager.receive_signal("PrepareForSleep")?;
    logger.log("listening for resumes from suspend (logind)");
    for signal in signals {
        let suspending: bool = signal.body().deserialize()?;
        if !suspending {
            logger.log("resumed from suspend");
            on_resume();
        }
    }
    logger.log("no resume signal: the system bus connection closed");
    Ok(())
}
