//! Declared action groups. Invalid groups never affect the primary launch data.
use super::*;

/// Retained actions per application; an implementation resource bound.
const MAX_ACTIONS: usize = 32;
type Fields<'a> = BTreeMap<&'a str, &'a str>;

pub(super) fn parse(
    text: &str,
    main: &Fields<'_>,
    path: &str,
    locale: &str,
    app_name: &str,
    app_icon: &str,
) -> Vec<ApplicationAction> {
    let mut groups: BTreeMap<&str, Option<Fields<'_>>> = BTreeMap::new();
    let mut current = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            current = line
                .strip_prefix("[Desktop Action ")
                .and_then(|line| line.strip_suffix(']'));
            if let Some(id) = current {
                // Repeated groups/keys are ambiguous, not an override rule.
                match groups.entry(id) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Some(BTreeMap::new()));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        entry.insert(None);
                    }
                }
            }
        } else if !line.starts_with('#')
            && let Some(fields) = current
                .and_then(|id| groups.get_mut(id))
                .and_then(Option::as_mut)
            && let Some((key, value)) = line.split_once('=')
            && fields.insert(key.trim(), value.trim()).is_some()
        {
            *groups.get_mut(current.unwrap()).unwrap() = None;
        }
    }
    let mut actions = vec![];
    for raw in main.get("Actions").copied().unwrap_or("").split(';') {
        if actions
            .iter()
            .any(|action: &ApplicationAction| action.id.as_str() == raw)
        {
            continue;
        }
        let Ok(id) = ApplicationActionId::new(raw) else {
            continue;
        };
        let Some(fields) = groups.get(raw).and_then(Option::as_ref) else {
            continue;
        };
        if fields.get("Name").is_none_or(|name| name.trim().is_empty()) {
            continue;
        }
        let name = localized(fields, "Name", locale);
        if !usable_label(&name) {
            continue;
        }
        let service = main.get("DBusActivatable") == Some(&"true");
        let exec = fields.get("Exec").copied().unwrap_or("");
        let command = if exec.is_empty() {
            if !service {
                continue;
            }
            None
        } else {
            let Ok(argv) = parse_exec(&unescape(exec), app_name, path, app_icon) else {
                continue;
            };
            Some(LaunchCommand {
                program: argv[0].clone(),
                args: argv[1..].into(),
                directory: main
                    .get("Path")
                    .map(|path| unescape(path))
                    .filter(|path| !path.is_empty()),
                terminal: main.get("Terminal") == Some(&"true"),
            })
        };
        actions.push(ApplicationAction {
            id,
            name,
            icon: localized(fields, "Icon", locale),
            launch: if service {
                ApplicationLaunch::DesktopService { fallback: command }
            } else {
                ApplicationLaunch::Command(command.expect("action Exec required"))
            },
        });
        if actions.len() == MAX_ACTIONS {
            break;
        }
    }
    actions
}

#[cfg(test)]
mod tests;
