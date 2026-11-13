//! The services folder of the browser demo: a few files kept in memory, with a line-based
//! stand-in for the backend's validation (the real one is `Catalog::diagnose`).

use std::cell::RefCell;

use protocol::{ServiceFile, ServiceProblem};

const WIKI: &str = "id = \"wiki\"\nname = \"Team wiki\"\ncolor = \"#0f766e\"\nurl = \"https://{instance}/\"\ninstance_label = \"Host\"\n\n[unread]\ntitle_regex = '^\\((\\d+)\\)'\n";
const BROKEN: &str = "id = \"chat\"\nname = \"Chat\"\ncolor = \nurl = \"https://chat.example/\"\n";
const EXAMPLE: &str = "# Example service definition.\nid = \"example\"\nname = \"Example Chat\"\ncolor = \"#3B82F6\"\nurl = \"https://{instance}/\"\n";

thread_local! {
    static FILES: RefCell<Vec<(String, String)>> = RefCell::new(vec![
        ("broken.toml".into(), BROKEN.into()),
        ("example.toml.disabled".into(), EXAMPLE.into()),
        ("wiki.toml".into(), WIKI.into()),
    ]);
}

pub(crate) fn list() -> Vec<ServiceFile> {
    FILES.with_borrow(|files| {
        files
            .iter()
            .map(|(name, _)| ServiceFile {
                name: name.clone(),
                enabled: !name.ends_with(".disabled"),
            })
            .collect()
    })
}

pub(crate) fn read(name: &str) -> Result<String, String> {
    FILES.with_borrow(|files| {
        files
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, text)| text.clone())
            .ok_or_else(|| format!("cannot read {name}"))
    })
}

pub(crate) fn validate(text: &str) -> Vec<ServiceProblem> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.trim_end().ends_with('='))
        .map(|(index, line)| ServiceProblem {
            message: "invalid string: expected a value".into(),
            line: u32::try_from(index + 1).ok(),
            column: u32::try_from(line.trim_end().chars().count() + 1).ok(),
        })
        .collect()
}

pub(crate) fn save(name: &str, text: &str, create: bool) -> Result<Vec<ServiceProblem>, String> {
    if name
        .trim_end_matches(".disabled")
        .strip_suffix(".toml")
        .is_none()
    {
        return Err("the file name must end in .toml".into());
    }
    let problems = validate(text);
    if !problems.is_empty() {
        return Ok(problems);
    }
    FILES.with_borrow_mut(|files| {
        match files.iter_mut().find(|(existing, _)| existing == name) {
            Some(_) if create => return Err(format!("{name} already exists")),
            Some((_, stored)) => *stored = text.to_string(),
            None => files.push((name.to_string(), text.to_string())),
        }
        files.sort();
        Ok(vec![])
    })
}

pub(crate) fn delete(name: &str) {
    FILES.with_borrow_mut(|files| files.retain(|(existing, _)| existing != name));
}

pub(crate) fn set_enabled(name: &str, enabled: bool) {
    FILES.with_borrow_mut(|files| {
        if let Some((existing, _)) = files.iter_mut().find(|(existing, _)| existing == name) {
            *existing = if enabled {
                name.trim_end_matches(".disabled").to_string()
            } else if name.ends_with(".disabled") {
                name.to_string()
            } else {
                format!("{name}.disabled")
            };
        }
        files.sort();
    });
}
