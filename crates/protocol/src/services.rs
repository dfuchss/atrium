//! User-defined services and the files of the service definition editor.

use serde::{Deserialize, Serialize};

/// Placeholder in service URLs and hosts for the per-account part (instance host, workspace
/// subdomain...), see [`AccountSettings::instance`](crate::AccountSettings::instance).
pub const INSTANCE_PLACEHOLDER: &str = "{instance}";

/// Id prefix of user-defined services.
const CUSTOM_PREFIX: &str = "custom-";

/// `custom-<slug of name>`: the lowercase ASCII letters and digits of `name`, other runs as one
/// dash; `custom-service` if none are left. Not unique: the backend appends a counter.
pub fn custom_service_base_id(name: &str) -> String {
    let mut slug = String::new();
    for character in name.trim().to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    format!(
        "{CUSTOM_PREFIX}{}",
        if slug.is_empty() { "service" } else { slug }
    )
}

/// A file in the services folder: `<name>.toml` (enabled, loaded) or `<name>.toml.disabled`
/// (ignored). `name` is the file name on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceFile {
    pub name: String,
    pub enabled: bool,
}

/// A problem of a service definition text; `line` and `column` (1-based) are known when the
/// TOML parser reports a position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceProblem {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// A user-defined service. Same meaning as the fields of the TOML service definitions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CustomService {
    /// Empty when adding; the backend assigns `custom-<slug>`.
    pub id: String,
    pub name: String,
    /// May contain `{instance}`.
    pub url: String,
    /// CSS color for the initials icon.
    pub color: String,
    /// Ask for an instance value when adding an account (only meaningful if `url` has `{instance}`).
    pub instance_label: Option<String>,
    /// Regex on the page title; capture group 1 = unread count. None = `^\((\d+)\)`.
    pub title_regex: Option<String>,
    /// "webkit" | "chrome" | "default" | literal; None = "webkit".
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_base_ids_are_slugs_of_the_name() {
        assert_eq!(custom_service_base_id(" My Chat! "), "custom-my-chat");
        assert_eq!(custom_service_base_id("ÄÖ"), "custom-service");
    }
}
