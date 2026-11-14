//! The macOS menu bar. Every command the window has is here as well as on
//! screen, so nothing is reachable only through ⌘K. A click sends the page an
//! `menu` event naming the command, which the page handles exactly as it
//! handles the visible control.
//!
//! The page tells the shell what the menu should hold as it changes (the
//! workspaces for Go, the open tabs for Window, the learner's own keys, what
//! is disabled now), and the menu is built again from that.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// What the page says the menu should hold.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Spec {
    /// The learner's key for each command, as "cmd+shift+h".
    #[serde(default)]
    keys: HashMap<String, String>,
    /// The library's workspaces, for Go: the folder and its name.
    #[serde(default)]
    workspaces: Vec<Named>,
    /// The open tabs, for Window: the tab's key and its name.
    #[serde(default)]
    tabs: Vec<Named>,
    /// Commands that do nothing where the learner is now.
    #[serde(default)]
    disabled: Vec<String>,
    /// A name for a command other than its own: "Hide Plan" when it is shown.
    #[serde(default)]
    titles: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Named {
    id: String,
    title: String,
}

/// What a custom item is for, kept as the menu is built.
#[derive(Clone, Debug)]
struct Entry {
    command: String,
    arg: Option<String>,
    accelerator: Option<String>,
}

#[derive(Default)]
pub struct MenuBar {
    spec: Mutex<Spec>,
    table: Mutex<HashMap<String, Entry>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// The keys the window starts with, as the page's own defaults.
const DEFAULT_KEYS: [(&str, &str); 22] = [
    ("home", "cmd+1"),
    ("commands", "cmd+k"),
    ("new-session", "cmd+n"),
    ("open-workspace", "cmd+o"),
    ("close-session", "cmd+w"),
    ("attach", "cmd+u"),
    ("settings", "cmd+,"),
    ("plan", "cmd+p"),
    ("stop-reply", "cmd+."),
    ("hint", "cmd+shift+h"),
    ("lost", "cmd+shift+l"),
    ("unknown", "cmd+shift+d"),
    ("symbols", "cmd+shift+m"),
    ("bigger", "cmd+="),
    ("smaller", "cmd+-"),
    ("actual-size", "cmd+0"),
    ("expand-panel", "cmd+shift+e"),
    ("flip-panel", "cmd+\\"),
    ("close-document", "cmd+shift+w"),
    ("open-document", "cmd+shift+o"),
    ("zoom-in-document", "cmd+alt+="),
    ("zoom-out-document", "cmd+alt+-"),
];

/// "cmd+shift+h" as the menu writes an accelerator, "Cmd+Shift+H".
pub fn accelerator(combo: &str) -> Option<String> {
    let combo = combo.trim().to_ascii_lowercase();
    if combo.is_empty() || combo == "none" {
        return None;
    }
    // A key that is itself "+" is written last, after a "+".
    let (mods, key) = match combo.strip_suffix("++") {
        Some(rest) => (rest, "+".to_owned()),
        None => {
            let mut parts: Vec<&str> = combo.split('+').collect();
            let key = parts.pop()?.to_owned();
            (combo[..combo.len() - key.len()].trim_end_matches('+'), key)
        }
    };
    let mut out = Vec::new();
    for part in mods.split('+').filter(|p| !p.is_empty()) {
        out.push(match part {
            "cmd" | "meta" | "command" => "Cmd",
            "ctrl" | "control" => "Ctrl",
            "alt" | "option" => "Alt",
            "shift" => "Shift",
            _ => return None,
        });
    }
    let key = match key.as_str() {
        "," => "Comma".to_owned(),
        "." => "Period".to_owned(),
        "=" => "Equal".to_owned(),
        "+" => "Equal".to_owned(),
        "-" => "Minus".to_owned(),
        "/" => "Slash".to_owned(),
        "\\" => "Backslash".to_owned(),
        "[" => "BracketLeft".to_owned(),
        "]" => "BracketRight".to_owned(),
        ";" => "Semicolon".to_owned(),
        "'" => "Quote".to_owned(),
        "`" => "Backquote".to_owned(),
        " " | "space" => "Space".to_owned(),
        "enter" | "return" => "Enter".to_owned(),
        "escape" | "esc" => "Escape".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "tab" => "Tab".to_owned(),
        "arrowup" => "ArrowUp".to_owned(),
        "arrowdown" => "ArrowDown".to_owned(),
        "arrowleft" => "ArrowLeft".to_owned(),
        "arrowright" => "ArrowRight".to_owned(),
        k if k.chars().count() == 1 => k.to_ascii_uppercase(),
        k if k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_ascii_uppercase(),
        _ => return None,
    };
    out.push(key.as_str());
    Some(out.join("+"))
}

impl MenuBar {
    fn key(spec: &Spec, command: &str) -> Option<String> {
        let combo = spec.keys.get(command).cloned().or_else(|| {
            DEFAULT_KEYS
                .iter()
                .find(|(name, _)| *name == command)
                .map(|(_, combo)| (*combo).to_owned())
        })?;
        accelerator(&combo)
    }

    /// Build the whole menu from the page's last word and put it on the app.
    pub fn apply<R: Runtime>(&self, app: &AppHandle<R>, spec: Spec) -> tauri::Result<()> {
        let mut table = HashMap::new();
        let menu = build(app, &spec, &mut table)?;
        *lock(&self.table) = table;
        *lock(&self.spec) = spec;
        app.set_menu(menu)?;
        Ok(())
    }

    pub fn spec(&self) -> Spec {
        lock(&self.spec).clone()
    }

    /// A custom item was chosen: tell the page, as a click on its control would.
    pub fn chosen<R: Runtime>(&self, app: &AppHandle<R>, id: &str) -> bool {
        let Some(entry) = lock(&self.table).get(id).cloned() else {
            return false;
        };
        let _ = app.emit(
            "menu",
            json!({ "command": entry.command, "arg": entry.arg }),
        );
        true
    }

    fn accelerator_of(&self, id: &str) -> Option<String> {
        lock(&self.table)
            .get(id)
            .and_then(|entry| entry.accelerator.clone())
    }
}

fn build<R: Runtime>(
    app: &AppHandle<R>,
    spec: &Spec,
    table: &mut HashMap<String, Entry>,
) -> tauri::Result<Menu<R>> {
    let item = |table: &mut HashMap<String, Entry>,
                id: &str,
                command: &str,
                arg: Option<&str>,
                title: &str,
                accelerator: Option<String>|
     -> tauri::Result<MenuItem<R>> {
        let title = spec.titles.get(id).map(String::as_str).unwrap_or(title);
        let enabled = !spec.disabled.iter().any(|off| off == id || off == command);
        table.insert(
            id.to_owned(),
            Entry {
                command: command.to_owned(),
                arg: arg.map(str::to_owned),
                accelerator: accelerator.clone(),
            },
        );
        MenuItem::with_id(app, id, title, enabled, accelerator)
    };
    let command = |table: &mut HashMap<String, Entry>, id: &str, title: &str| {
        let accelerator = MenuBar::key(spec, id);
        item(table, id, id, None, title, accelerator)
    };
    let separator = || PredefinedMenuItem::separator(app);

    let app_menu = Submenu::with_items(
        app,
        "Magnesium",
        true,
        &[
            &command(table, "about", "About Magnesium")?,
            &separator()?,
            &command(table, "settings", "Settings…")?,
            &separator()?,
            &PredefinedMenuItem::services(app, None)?,
            &separator()?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &separator()?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &command(table, "new-session", "New Session")?,
            &command(table, "open-workspace", "Open a Folder…")?,
            &separator()?,
            &command(table, "close-session", "Close Session")?,
            &separator()?,
            &command(table, "open-document", "Open Document…")?,
            &command(table, "close-document", "Close Document")?,
            &command(table, "share-document", "Share Document…")?,
            &separator()?,
            &command(table, "attach", "Attach a Photo or PDF…")?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &separator()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &command(table, "home", "Home")?,
            &command(table, "plan", "Show Panel")?,
            &command(table, "expand-panel", "Expand Panel")?,
            &command(table, "flip-panel", "Switch to the Document")?,
            &separator()?,
            &command(table, "zoom-in-document", "Zoom In on the Document")?,
            &command(table, "zoom-out-document", "Zoom Out of the Document")?,
            &command(table, "zoom-fit-document", "Fit the Document to the Panel")?,
            &separator()?,
            &command(table, "bigger", "Bigger Text")?,
            &command(table, "smaller", "Smaller Text")?,
            &command(table, "actual-size", "Actual Size")?,
            &separator()?,
            &command(table, "commands", "Command Menu…")?,
            &separator()?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    let go = Submenu::new(app, "Go", true)?;
    if spec.workspaces.is_empty() {
        go.append(&MenuItem::new(
            app,
            "No workspaces yet",
            false,
            None::<&str>,
        )?)?;
    }
    for (n, workspace) in spec.workspaces.iter().enumerate().take(40) {
        let id = format!("go-{n}");
        go.append(&item(
            table,
            &id,
            "go",
            Some(&workspace.id),
            &workspace.title,
            None,
        )?)?;
    }

    let session = Submenu::with_items(
        app,
        "Session",
        true,
        &[
            &command(table, "hint", "Hint")?,
            &command(table, "lost", "I’m Lost")?,
            &command(table, "unknown", "I Don’t Know")?,
            &separator()?,
            &command(table, "symbols", "Symbols")?,
            &separator()?,
            &command(table, "stop-reply", "Stop the Reply")?,
        ],
    )?;

    let window = Submenu::new(app, "Window", true)?;
    window.append_items(&[
        &PredefinedMenuItem::minimize(app, None)?,
        &PredefinedMenuItem::maximize(app, None)?,
        &separator()?,
        &item(
            table,
            "previous-tab",
            "previous-tab",
            None,
            "Show Previous Tab",
            Some("Cmd+Shift+BracketLeft".into()),
        )?,
        &item(
            table,
            "next-tab",
            "next-tab",
            None,
            "Show Next Tab",
            Some("Cmd+Shift+BracketRight".into()),
        )?,
    ])?;
    if !spec.tabs.is_empty() {
        window.append(&separator()?)?;
    }
    let home_key = MenuBar::key(spec, "home");
    for (n, tab) in spec.tabs.iter().enumerate().take(20) {
        // ⌘1 to ⌘9 pick the first nine tabs, unless the learner gave one to another command.
        let wanted = (n < 9).then(|| format!("Cmd+{}", n + 1));
        let taken = wanted.as_ref().is_some_and(|key| {
            Some(key) == home_key.as_ref()
                || DEFAULT_KEYS.iter().any(|(name, _)| {
                    *name != "home" && MenuBar::key(spec, name).as_ref() == Some(key)
                })
        });
        let accelerator = if taken { None } else { wanted };
        window.append(&item(
            table,
            &format!("tab-{n}"),
            "tab",
            Some(&tab.id),
            &tab.title,
            accelerator,
        )?)?;
    }

    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[&command(table, "keys-help", "Keys and Commands")?],
    )?;

    let menu = Menu::with_items(
        app,
        &[
            &app_menu, &file, &edit, &view, &go, &session, &window, &help,
        ],
    )?;
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_as_windows_menu_for_nsapp();
        let _ = help.set_as_help_menu_for_nsapp();
    }
    Ok(menu)
}

/// The standard items' keys, by their names, for the harness's reading.
fn standard_key(title: &str) -> Option<&'static str> {
    Some(match title {
        "Undo" => "Cmd+Z",
        "Redo" => "Cmd+Shift+Z",
        "Cut" => "Cmd+X",
        "Copy" => "Cmd+C",
        "Paste" => "Cmd+V",
        "Select All" => "Cmd+A",
        "Minimize" => "Cmd+M",
        "Hide Others" => "Cmd+Alt+H",
        t if t.starts_with("Hide ") => "Cmd+H",
        t if t.starts_with("Quit ") => "Cmd+Q",
        t if t.contains("Full Screen") => "Cmd+Ctrl+F",
        _ => return None,
    })
}

fn describe<R: Runtime>(bar: &MenuBar, kind: &MenuItemKind<R>) -> Value {
    match kind {
        MenuItemKind::MenuItem(item) => json!({
            "title": item.text().unwrap_or_default(),
            "id": item.id().as_ref(),
            "enabled": item.is_enabled().unwrap_or(false),
            "accelerator": bar.accelerator_of(item.id().as_ref()),
        }),
        MenuItemKind::Predefined(item) => {
            let title = item.text().unwrap_or_default();
            if title.is_empty() {
                json!({ "separator": true })
            } else {
                json!({
                    "title": title,
                    "id": item.id().as_ref(),
                    "enabled": true,
                    "accelerator": standard_key(&title),
                    "standard": true,
                })
            }
        }
        MenuItemKind::Submenu(sub) => json!({
            "title": sub.text().unwrap_or_default(),
            "id": sub.id().as_ref(),
            "enabled": sub.is_enabled().unwrap_or(false),
            "items": sub.items().unwrap_or_default().iter().map(|k| describe(bar, k)).collect::<Vec<_>>(),
        }),
        MenuItemKind::Check(item) => json!({
            "title": item.text().unwrap_or_default(),
            "id": item.id().as_ref(),
            "enabled": item.is_enabled().unwrap_or(false),
            "accelerator": bar.accelerator_of(item.id().as_ref()),
        }),
        MenuItemKind::Icon(item) => json!({
            "title": item.text().unwrap_or_default(),
            "id": item.id().as_ref(),
            "enabled": item.is_enabled().unwrap_or(false),
        }),
    }
}

/// The whole menu bar as data: each menu's title and its items.
pub fn read<R: Runtime>(app: &AppHandle<R>) -> Value {
    let bar = app.state::<MenuBar>();
    let menus = app
        .menu()
        .and_then(|menu| menu.items().ok())
        .unwrap_or_default()
        .iter()
        .map(|kind| describe(&bar, kind))
        .collect::<Vec<_>>();
    json!(menus)
}

/// Loose match of an item's title: case, a trailing "…" and curly quotes aside.
fn same(title: &str, wanted: &str) -> bool {
    let plain = |text: &str| {
        text.trim()
            .trim_end_matches('…')
            .trim_end_matches("...")
            .replace(['’', '‘'], "'")
            .to_lowercase()
    };
    plain(title) == plain(wanted)
}

/// Choose the item at `path` (a menu's title, then the item's), exactly as a
/// click on it would.
pub fn click<R: Runtime>(app: &AppHandle<R>, path: &[String]) -> Result<String, String> {
    let [top, rest @ ..] = path else {
        return Err("name a menu and an item, as [\"File\", \"Attach…\"]".into());
    };
    let menu = app.menu().ok_or("the window has no menu")?;
    let mut items = menu.items().map_err(|e| e.to_string())?;
    let mut wanted = std::iter::once(top).chain(rest.iter()).peekable();
    while let Some(name) = wanted.next() {
        let found = items
            .iter()
            .find(|kind| {
                let title = match kind {
                    MenuItemKind::MenuItem(i) => i.text().unwrap_or_default(),
                    MenuItemKind::Submenu(i) => i.text().unwrap_or_default(),
                    MenuItemKind::Predefined(i) => i.text().unwrap_or_default(),
                    MenuItemKind::Check(i) => i.text().unwrap_or_default(),
                    MenuItemKind::Icon(i) => i.text().unwrap_or_default(),
                };
                same(&title, name)
            })
            .cloned()
            .ok_or_else(|| format!("no item called {name:?}"))?;
        let last = wanted.peek().is_none();
        match found {
            MenuItemKind::Submenu(sub) if !last => {
                items = sub.items().map_err(|e| e.to_string())?;
            }
            MenuItemKind::MenuItem(item) if last => {
                if !item.is_enabled().unwrap_or(false) {
                    return Err(format!("{name:?} is not available now"));
                }
                let id = item.id().as_ref().to_owned();
                let bar = app.state::<MenuBar>();
                if !bar.chosen(app, &id) {
                    return Err(format!("{name:?} does nothing in this window"));
                }
                return Ok(id);
            }
            MenuItemKind::Predefined(_) if last => {
                return Err(format!(
                    "{name:?} is one of macOS's own items; the harness does not press those"
                ))
            }
            _ => return Err(format!("{name:?} is not an item to choose")),
        }
    }
    Err("name a menu and an item".into())
}

#[cfg(test)]
mod tests {
    use super::accelerator;

    #[test]
    fn keys_are_written_as_the_menu_writes_them() {
        assert_eq!(accelerator("cmd+shift+h").as_deref(), Some("Cmd+Shift+H"));
        assert_eq!(accelerator("cmd+,").as_deref(), Some("Cmd+Comma"));
        assert_eq!(accelerator("cmd+=").as_deref(), Some("Cmd+Equal"));
        assert_eq!(accelerator("cmd+-").as_deref(), Some("Cmd+Minus"));
        assert_eq!(accelerator("cmd+.").as_deref(), Some("Cmd+Period"));
        assert_eq!(
            accelerator("ctrl+alt+arrowup").as_deref(),
            Some("Ctrl+Alt+ArrowUp")
        );
        assert_eq!(accelerator("cmd+1").as_deref(), Some("Cmd+1"));
        assert_eq!(accelerator("none"), None);
        assert_eq!(accelerator("hyper+k"), None);
    }
}
