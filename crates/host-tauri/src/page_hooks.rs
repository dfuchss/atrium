//! The hooks service views and the windows they open share: the navigation allow-list
//! ([`nav_allowed`]) and media permission ([`domain::ViewPolicy::allow_media`], scoped to the
//! service's own hosts by [`ViewSpec::media_allowed_at`]).

use std::sync::Arc;

use domain::{Input, ViewSpec, nav_allowed, redact_url};
use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::{Runtime, Url};
use uuid::Uuid;

use crate::links::{ViewPolicies, policy_of};
use crate::{Feed, Logger};

/// Camera, microphone and screen capture are granted only with `media_allowed`, else denied;
/// everything else gets the engine's default handling.
fn permission_response(kind: PermissionKind, media_allowed: bool) -> PermissionResponse {
    match kind {
        PermissionKind::Camera | PermissionKind::Microphone | PermissionKind::DisplayCapture => {
            if media_allowed {
                PermissionResponse::Allow
            } else {
                PermissionResponse::Deny
            }
        }
        _ => PermissionResponse::Default,
    }
}

/// Navigation hook of service views and their windows: [`nav_allowed`], denials are logged.
/// `mailto:` is never loaded in the view but handed to the core as an external link (mail app).
pub(crate) fn navigation_hook(
    account: Uuid,
    logger: Logger,
    feed: Feed,
) -> impl Fn(&Url) -> bool + Send + 'static {
    move |url| {
        let allowed = nav_allowed(url);
        if allowed {
            logger.log_navigation(
                account,
                &format!("[{account}] navigation {}", redact_url(url.as_str())),
            );
        } else if url.scheme() == "mailto" {
            logger.log(&format!("[{account}] mailto link -> mail app if allowed"));
            feed(Input::ExternalLink {
                account,
                url: url.to_string(),
            });
        } else {
            logger.log_navigation(
                account,
                &format!(
                    "[{account}] navigation DENIED (scheme) {}",
                    redact_url(url.as_str())
                ),
            );
        }
        allowed
    }
}

/// Permission hook of service views and their windows (reads the current policy). wry drops
/// the requesting origin, so media is scoped by the page currently shown in the main frame.
pub(crate) fn permission_hook<R: Runtime>(
    spec: Arc<ViewSpec>,
    policies: ViewPolicies,
    logger: Logger,
    what: &'static str,
) -> impl Fn(tauri::Webview<R>, PermissionKind) -> PermissionResponse + Send + Sync + 'static {
    move |webview, kind| {
        let account = spec.account;
        let page = webview.url().ok();
        let host = page.as_ref().and_then(Url::host_str).unwrap_or("-");
        let media_allowed =
            spec.media_allowed_at(policy_of(&policies, account).allow_media, page.as_ref());
        let response = permission_response(kind, media_allowed);
        logger.log(&format!(
            "[{account}] {what} permission {kind} at {host} -> {response:?}"
        ));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowed_media_is_granted() {
        for kind in [
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::DisplayCapture,
        ] {
            assert!(matches!(
                permission_response(kind, true),
                PermissionResponse::Allow
            ));
            assert!(matches!(
                permission_response(kind, false),
                PermissionResponse::Deny
            ));
        }
        assert!(matches!(
            permission_response(PermissionKind::Geolocation, true),
            PermissionResponse::Default
        ));
    }
}
