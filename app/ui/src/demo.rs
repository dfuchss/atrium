//! Browser demo (`?demo`): sample data, and commands applied to it locally instead of in Rust.

use protocol::{
    AccountSettings, AccountView, AppInfo, AppSettings, CustomService, ServiceInfo, Snapshot,
    UiCommand,
};
use uuid::Uuid;

/// Inline white glyph standing in for a Simple Icons download in the demo.
const DEMO_GLYPH: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path fill='white' d='M12 2 2 7v10l10 5 10-5V7zm0 4a6 6 0 1 1 0 12 6 6 0 0 1 0-12z'/></svg>";
const DEMO_ICON: &str = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect width='32' height='32' rx='8' fill='%23e11d48'/><circle cx='16' cy='16' r='8' fill='white'/></svg>";

pub(crate) fn demo_snapshot() -> Snapshot {
    let services = demo_services();
    Snapshot {
        accounts: demo_accounts(&services),
        active: Some(Uuid::from_u128(1)),
        custom_services: vec![CustomService {
            id: "custom-wiki".into(),
            name: "Wiki".into(),
            url: "https://{instance}/".into(),
            color: "#0f766e".into(),
            instance_label: Some("Host".into()),
            title_regex: None,
            user_agent: None,
        }],
        services,
        dialog_open: false,
        settings: AppSettings::default(),
        service_errors: vec![],
        app: AppInfo {
            version: "0.1.0-demo".into(),
            services_dir: "~/Library/Application Support/org.fuchss.atrium/services".into(),
            config_dir: "~/Library/Application Support/org.fuchss.atrium".into(),
            log_file: "~/Library/Logs/org.fuchss.atrium/atrium.log".into(),
        },
    }
}

fn demo_services() -> Vec<ServiceInfo> {
    let service = |id: &str, name: &str, color: &str, url: &str| ServiceInfo {
        id: id.into(),
        name: name.into(),
        color: color.into(),
        url: url.into(),
        ..ServiceInfo::default()
    };
    vec![
        ServiceInfo {
            instance_label: Some("Workspace".into()),
            title_regex: Some(r"^\((\d+)\)".into()),
            ..service("slack", "Slack", "#4A154B", "https://{instance}.slack.com/")
        },
        ServiceInfo {
            instance_label: Some("Instance".into()),
            instance_default: Some("app.element.io".into()),
            title_regex: Some(r"^\[(\d+)\]".into()),
            user_agent: Some("chrome".into()),
            ..service("element", "Element", "#0DBD8B", "https://{instance}/")
        },
        ServiceInfo {
            instance_label: Some("Instance".into()),
            instance_default: Some("mastodon.social".into()),
            user_agent: Some("Mozilla/5.0 (Macintosh) Mastodon".into()),
            ..service("mastodon", "Mastodon", "#6364FF", "https://{instance}/")
        },
        service("discord", "Discord", "#5865F2", "https://discord.com/app"),
        ServiceInfo {
            instance_label: Some("Host".into()),
            custom: true,
            ..service("custom-wiki", "Wiki", "#0f766e", "https://{instance}/")
        },
    ]
}

/// An account of `service_id` with default settings.
fn demo_account(
    services: &[ServiceInfo],
    id: u128,
    service_id: &str,
    label: &str,
    initials: &str,
) -> AccountView {
    let service = services
        .iter()
        .find(|service| service.id == service_id)
        .cloned()
        .unwrap_or_default();
    AccountView {
        id: Uuid::from_u128(id),
        service: service.id,
        service_name: service.name,
        color: service.color,
        initials: initials.into(),
        unread: 0,
        indirect: false,
        instance_label: service.instance_label.clone(),
        icon: None,
        icon_glyph: false,
        hibernated: false,
        settings: AccountSettings {
            label: label.into(),
            instance: service.instance_label.map(|_| "example".to_string()),
            muted: false,
            show_badge: true,
            in_dock_badge: true,
            user_agent: None,
            enabled: true,
            link_target: None,
            custom_icon: None,
            simple_icon: None,
            zoom: 1.0,
            hibernate_after_min: None,
            start_asleep: false,
            after_wake: None,
            custom_css: None,
            allow_media: None,
            app_hosts: vec![],
        },
    }
}

/// One account per sidebar state: custom icon with unread, sleeping, muted glyph, disabled,
/// and a custom service.
fn demo_accounts(services: &[ServiceInfo]) -> Vec<AccountView> {
    let mut slack = demo_account(services, 1, "slack", "KIT", "S");
    slack.unread = 3;
    slack.icon = Some(DEMO_ICON.into());
    slack.settings.custom_icon = Some(DEMO_ICON.into());

    let mut element = demo_account(services, 2, "element", "Home", "E");
    element.indirect = true;
    element.hibernated = true;
    element.settings.hibernate_after_min = Some(15);

    let mut discord = demo_account(services, 3, "discord", "Friends", "D");
    discord.unread = 120;
    discord.settings.muted = true;
    discord.icon = Some(DEMO_GLYPH.into());
    discord.icon_glyph = true;
    discord.settings.simple_icon = Some("discord".into());

    let mut work = demo_account(services, 4, "slack", "Work", "SW");
    work.settings.enabled = false;
    work.hibernated = true;

    let mut wiki = demo_account(services, 5, "custom-wiki", "Team wiki", "W");
    wiki.instance_label = None;
    wiki.settings.instance = None;

    vec![slack, element, discord, work, wiki]
}

/// Applies a command to the demo snapshot the way the core would, as far as the demo needs.
pub(crate) fn apply_locally(snapshot: &mut Snapshot, command: UiCommand) {
    match command {
        UiCommand::Select { account } => snapshot.active = Some(account),
        UiCommand::SetDialog { open } => snapshot.dialog_open = open,
        UiCommand::SetDnd { enabled } => snapshot.settings.dnd = enabled,
        UiCommand::UpdateSettings { settings } => snapshot.settings = settings,
        UiCommand::UpdateAccount { account, settings } => {
            if let Some(view) = account_view_mut(snapshot, account) {
                view.settings = settings;
            }
        }
        UiCommand::SetMuted { account, muted } => {
            if let Some(view) = account_view_mut(snapshot, account) {
                view.settings.muted = muted;
            }
        }
        UiCommand::RemoveAccount { account } => {
            snapshot.accounts.retain(|view| view.id != account);
        }
        UiCommand::Hibernate { account } => {
            if let Some(view) = account_view_mut(snapshot, account) {
                view.hibernated = true;
            }
        }
        UiCommand::AddCustomService { service } => add_custom_service(snapshot, service),
        UiCommand::UpdateCustomService { service } => {
            if let Some(stored) = snapshot
                .custom_services
                .iter_mut()
                .find(|stored| stored.id == service.id)
            {
                *stored = service;
            }
        }
        UiCommand::RemoveCustomService { id } => remove_custom_service(snapshot, &id),
        UiCommand::Move { account, delta } => move_account(snapshot, account, delta),
        _ => {}
    }
}

fn account_view_mut(snapshot: &mut Snapshot, id: Uuid) -> Option<&mut AccountView> {
    snapshot
        .accounts
        .iter_mut()
        .find(|account| account.id == id)
}

fn add_custom_service(snapshot: &mut Snapshot, mut service: CustomService) {
    service.id = format!("custom-{}", service.name.to_lowercase().replace(' ', "-"));
    snapshot.services.push(ServiceInfo {
        id: service.id.clone(),
        name: service.name.clone(),
        color: service.color.clone(),
        instance_label: service.instance_label.clone(),
        instance_default: None,
        custom: true,
        allow_media: false,
        url: service.url.clone(),
        title_regex: service.title_regex.clone(),
        user_agent: service.user_agent.clone(),
    });
    snapshot.custom_services.push(service);
}

fn remove_custom_service(snapshot: &mut Snapshot, id: &str) {
    if !snapshot
        .accounts
        .iter()
        .any(|account| account.service == id)
    {
        snapshot.custom_services.retain(|service| service.id != id);
        snapshot.services.retain(|service| service.id != id);
    }
}

fn move_account(snapshot: &mut Snapshot, account: Uuid, delta: i32) {
    let accounts = &mut snapshot.accounts;
    if let Some(index) = accounts.iter().position(|view| view.id == account) {
        let target = index
            .saturating_add_signed(isize::try_from(delta).unwrap_or(0))
            .min(accounts.len() - 1);
        let moved = accounts.remove(index);
        accounts.insert(target, moved);
    }
}
