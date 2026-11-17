//! One running Atrium per user: two processes would share the accounts' web data folders. The
//! single-instance plugin claims a name of the user's session (on Linux on the D-Bus session
//! bus); a second start hands over to the running Atrium and exits. Where the name cannot be
//! claimed (Linux without a session bus) every start runs.

use tauri::{Builder, Manager, Runtime};

use crate::{Logger, bring_to_front};

/// Registers the guard with `builder` (before the other plugins): when Atrium is started again,
/// the running one brings its window `window_label` to the front and the new process exits.
pub(crate) fn guard_single_instance<R: Runtime>(
    builder: Builder<R>,
    window_label: &'static str,
) -> Builder<R> {
    builder.plugin(tauri_plugin_single_instance::init(
        move |app, _args, _cwd| {
            let logger = Logger::new(app);
            logger.log("started again: showing the running Atrium");
            if let Some(window) = app.get_window(window_label)
                && let Err(error) = bring_to_front(&window)
            {
                logger.log(&format!("cannot bring the window to the front: {error}"));
            }
        },
    ))
}
