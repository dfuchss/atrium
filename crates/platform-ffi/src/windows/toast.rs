//! Toast notifications that Windows activates by launching a URI (protocol activation): a click
//! works the same while the toast is on screen and after it moved to the notification centre,
//! where an in-process `Activated` handler is gone.

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

/// `PowerShell`'s `AppUserModelID`, under which an app without a Start menu shortcut can show
/// toasts (Windows names them after `PowerShell`).
pub const POWERSHELL_APP_ID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// Shows the toast `xml` (see [`toast_xml`]) under the `AppUserModelID` `app_id`.
pub fn show_toast(app_id: &str, xml: &str) -> windows::core::Result<()> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&toast)
}

/// The XML of a toast showing `title` and `body` that launches the URI `launch` when clicked,
/// with the sound `ms-winsoundevent:Notification.<sound>` or silent.
pub fn toast_xml(title: &str, body: &str, launch: &str, sound: Option<&str>) -> String {
    let audio = match sound {
        Some(sound) => format!(
            r#"<audio src="ms-winsoundevent:Notification.{}"/>"#,
            escape_xml(sound)
        ),
        None => r#"<audio silent="true"/>"#.into(),
    };
    format!(
        r#"<toast launch="{}" activationType="protocol"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual>{audio}</toast>"#,
        escape_xml(launch),
        escape_xml(title),
        escape_xml(body),
    )
}

/// `text` as XML character data or attribute value: markup characters become entities, control
/// characters that XML cannot hold are dropped.
fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t' | '\n' | '\r' => escaped.push(character),
            '\0'..='\u{1f}' => {}
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_and_control_characters_are_escaped() {
        assert_eq!(
            escape_xml("<b>Tom & \"Jerry\"</b> it's\u{1}\n"),
            "&lt;b&gt;Tom &amp; &quot;Jerry&quot;&lt;/b&gt; it&apos;s\n"
        );
        assert_eq!(escape_xml("Grüße 👋"), "Grüße 👋");
    }

    #[test]
    fn a_toast_launches_its_uri_with_the_sound_or_silently() {
        assert_eq!(
            toast_xml("A & B", "<hi>", "app://notification/7", Some("IM")),
            r#"<toast launch="app://notification/7" activationType="protocol"><visual><binding template="ToastGeneric"><text>A &amp; B</text><text>&lt;hi&gt;</text></binding></visual><audio src="ms-winsoundevent:Notification.IM"/></toast>"#
        );
        assert!(
            toast_xml("t", "b", "app://x", None).ends_with(r#"<audio silent="true"/></toast>"#)
        );
    }

    #[test]
    fn the_toast_xml_loads() {
        let document = XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(toast_xml(
                "<&>", "\u{7}'\"", "app://x", None,
            )))
            .unwrap();
    }
}
