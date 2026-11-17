use tauri_build::{Attributes, DefaultPermissionRule, InlinedPlugin};

fn main() {
    // The plugin of the JavaScript dialog windows (`host_tauri`, Windows); the capability
    // `script-dialog` grants it to those windows only.
    let script_dialog = InlinedPlugin::new()
        .commands(&["get", "show", "answer"])
        .default_permission(DefaultPermissionRule::AllowAllCommands);
    tauri_build::try_build(Attributes::new().plugin("script-dialog", script_dialog))
        .unwrap_or_else(|error| panic!("{error:#}"));
}
