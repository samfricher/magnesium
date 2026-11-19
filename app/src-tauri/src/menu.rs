use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Spec {
    #[serde(default)]
    keys: HashMap<String, String>,
    #[serde(default)]
    workspaces: Vec<Named>,
    #[serde(default)]
    tabs: Vec<Named>,
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default)]
    titles: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Named {
    id: String,
    title: String,
}

#[derive(Clone, Debug)]
enum Key {
    Learner,
    Tab(usize),
    Fixed(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
struct Shown {
    text: String,
    enabled: bool,
    accelerator: Option<String>,
}

#[derive(Clone, Debug)]
struct Entry {
    command: String,
    arg: Option<String>,
    title: String,
    key: Key,
    shown: Shown,
}

impl Entry {
    fn wanted(&self, id: &str, spec: &Spec) -> Shown {
        Shown {
            text: spec
                .titles
                .get(id)
                .cloned()
                .unwrap_or_else(|| self.title.clone()),
            enabled: !spec
                .disabled
                .iter()
                .any(|off| off == id || *off == self.command),
            accelerator: match &self.key {
                Key::Learner => MenuBar::key(spec, id),
                Key::Tab(n) => tab_key(spec, *n),
                Key::Fixed(key) => key.clone(),
            },
        }
    }
}

#[derive(Default)]
pub struct MenuBar {
    spec: Mutex<Spec>,
    table: Mutex<HashMap<String, Entry>>,
    applying: Mutex<()>,
    replaced: AtomicUsize,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

const DEFAULT_KEYS: [(&str, &str); 26] = [
    ("home", "cmd+1"),
    ("commands", "cmd+k"),
    ("new-tab", "cmd+t"),
    ("new-session", "cmd+n"),
    ("just-checks", "cmd+shift+n"),
    ("open-workspace", "cmd+o"),
    ("close-session", "cmd+w"),
    ("attach", "cmd+u"),
    ("settings", "cmd+,"),
    ("plan", "cmd+p"),
    ("stop-reply", "cmd+."),
    ("hint", "cmd+shift+h"),
    ("lost", "cmd+shift+l"),
    ("unknown", "cmd+shift+d"),
    ("look-back", "cmd+arrowup"),
    ("symbols", "cmd+shift+m"),
    ("model", "cmd+shift+j"),
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

pub fn accelerator(combo: &str) -> Option<String> {
    let combo = combo.trim().to_ascii_lowercase();
    if combo.is_empty() || combo == "none" {
        return None;
    }
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

    pub fn apply<R: Runtime>(&self, app: &AppHandle<R>, spec: Spec) -> tauri::Result<()> {
        let _one = lock(&self.applying);
        let Some(menu) = app.menu().filter(|menu| menu.get(WINDOW).is_some()) else {
            return self.replace(app, spec);
        };
        if self.spec() == spec {
            return Ok(());
        }
        match self.change(app, &menu, &spec) {
            Ok(()) => Ok(()),
            Err(_) => self.replace(app, spec),
        }
    }

    fn replace<R: Runtime>(&self, app: &AppHandle<R>, spec: Spec) -> tauri::Result<()> {
        let mut table = HashMap::new();
        let menu = build(app, &spec, &mut table)?;
        *lock(&self.table) = table;
        *lock(&self.spec) = spec;
        app.set_menu(menu)?;
        self.replaced.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn change<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        menu: &Menu<R>,
        spec: &Spec,
    ) -> tauri::Result<()> {
        let before = self.spec();
        let mut table = lock(&self.table).clone();
        let mut make = Maker {
            app,
            spec,
            table: &mut table,
        };
        for (list, id) in [(List::Go, GO), (List::Tabs, WINDOW)] {
            if let Some(MenuItemKind::Submenu(sub)) = menu.get(id) {
                relist(&mut make, &sub, list, list.named(&before).len())?;
            }
        }
        let live = items_on(menu)?;
        for (id, entry) in table.iter_mut() {
            let wanted = entry.wanted(id, spec);
            if wanted == entry.shown {
                continue;
            }
            let Some((item, parent)) = live.get(id) else {
                continue;
            };
            if wanted.text != entry.shown.text {
                item.set_text(&wanted.text)?;
            }
            if wanted.enabled != entry.shown.enabled {
                item.set_enabled(wanted.enabled)?;
            }
            if wanted.accelerator != entry.shown.accelerator {
                match &wanted.accelerator {
                    Some(key) => item.set_accelerator(Some(key))?,
                    None => {
                        let fresh =
                            MenuItem::with_id(app, id, &wanted.text, wanted.enabled, None::<&str>)?;
                        if let Some(at) = parent
                            .items()?
                            .iter()
                            .position(|each| each.id() == item.id())
                        {
                            parent.remove_at(at)?;
                            parent.insert(&fresh, at)?;
                        }
                    }
                }
            }
            entry.shown = wanted;
        }
        *lock(&self.table) = table;
        *lock(&self.spec) = spec.clone();
        Ok(())
    }

    pub fn replaced(&self) -> usize {
        self.replaced.load(Ordering::Relaxed)
    }

    pub fn spec(&self) -> Spec {
        lock(&self.spec).clone()
    }

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
            .and_then(|entry| entry.shown.accelerator.clone())
    }
}

const GO: &str = "menu-go";
const WINDOW: &str = "menu-window";
const WINDOW_FIXED: usize = 5;

fn tab_key(spec: &Spec, n: usize) -> Option<String> {
    let wanted = (n < 9).then(|| format!("Cmd+{}", n + 1))?;
    let taken = MenuBar::key(spec, "home").as_ref() == Some(&wanted)
        || DEFAULT_KEYS
            .iter()
            .any(|(name, _)| *name != "home" && MenuBar::key(spec, name).as_ref() == Some(&wanted));
    (!taken).then_some(wanted)
}

#[derive(Clone, Copy)]
enum List {
    Go,
    Tabs,
}

impl List {
    fn named(self, spec: &Spec) -> &[Named] {
        match self {
            List::Go => &spec.workspaces[..spec.workspaces.len().min(40)],
            List::Tabs => &spec.tabs[..spec.tabs.len().min(20)],
        }
    }

    fn id(self, n: usize) -> String {
        match self {
            List::Go => format!("go-{n}"),
            List::Tabs => format!("tab-{n}"),
        }
    }

    fn start(self) -> usize {
        match self {
            List::Go => 0,
            List::Tabs => WINDOW_FIXED + 1,
        }
    }
}

struct Maker<'a, R: Runtime> {
    app: &'a AppHandle<R>,
    spec: &'a Spec,
    table: &'a mut HashMap<String, Entry>,
}

impl<R: Runtime> Maker<'_, R> {
    fn item(
        &mut self,
        id: &str,
        command: &str,
        arg: Option<&str>,
        title: &str,
        key: Key,
    ) -> tauri::Result<MenuItem<R>> {
        let mut entry = Entry {
            command: command.to_owned(),
            arg: arg.map(str::to_owned),
            title: title.to_owned(),
            key,
            shown: Shown {
                text: String::new(),
                enabled: true,
                accelerator: None,
            },
        };
        entry.shown = entry.wanted(id, self.spec);
        let item = MenuItem::with_id(
            self.app,
            id,
            &entry.shown.text,
            entry.shown.enabled,
            entry.shown.accelerator.as_deref(),
        )?;
        self.table.insert(id.to_owned(), entry);
        Ok(item)
    }

    fn command(&mut self, id: &str, title: &str) -> tauri::Result<MenuItem<R>> {
        self.item(id, id, None, title, Key::Learner)
    }

    fn listed(&mut self, list: List, n: usize) -> tauri::Result<MenuItem<R>> {
        let spec = self.spec;
        let named = &list.named(spec)[n];
        let (command, key) = match list {
            List::Go => ("go", Key::Fixed(None)),
            List::Tabs => ("tab", Key::Tab(n)),
        };
        self.item(&list.id(n), command, Some(&named.id), &named.title, key)
    }
}

fn no_workspaces<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<MenuItem<R>> {
    MenuItem::with_id(app, "go-none", "No spaces yet", false, None::<&str>)
}

fn relist<R: Runtime>(
    make: &mut Maker<'_, R>,
    menu: &Submenu<R>,
    list: List,
    had: usize,
) -> tauri::Result<()> {
    let spec = make.spec;
    let named = list.named(spec);
    let now = named.len();
    for (n, each) in named.iter().enumerate().take(had) {
        if let Some(entry) = make.table.get_mut(&list.id(n)) {
            entry.arg = Some(each.id.clone());
            entry.title = each.title.clone();
        }
    }
    if had == 0 && now > 0 {
        match list {
            List::Go => {
                menu.remove_at(0)?;
            }
            List::Tabs => menu.insert(&PredefinedMenuItem::separator(make.app)?, WINDOW_FIXED)?,
        }
    }
    for n in (now..had).rev() {
        menu.remove_at(list.start() + n)?;
        make.table.remove(&list.id(n));
    }
    for n in had..now {
        let item = make.listed(list, n)?;
        menu.insert(&item, list.start() + n)?;
    }
    if had > 0 && now == 0 {
        match list {
            List::Go => menu.insert(&no_workspaces(make.app)?, 0)?,
            List::Tabs => {
                menu.remove_at(WINDOW_FIXED)?;
            }
        }
    }
    Ok(())
}

type OnBar<R> = HashMap<String, (MenuItem<R>, Submenu<R>)>;

fn items_on<R: Runtime>(menu: &Menu<R>) -> tauri::Result<OnBar<R>> {
    let mut found = HashMap::new();
    for top in menu.items()? {
        if let MenuItemKind::Submenu(sub) = top {
            for each in sub.items()? {
                if let MenuItemKind::MenuItem(item) = each {
                    found.insert(item.id().as_ref().to_owned(), (item, sub.clone()));
                }
            }
        }
    }
    Ok(found)
}

fn build<R: Runtime>(
    app: &AppHandle<R>,
    spec: &Spec,
    table: &mut HashMap<String, Entry>,
) -> tauri::Result<Menu<R>> {
    let mut make = Maker { app, spec, table };
    let separator = || PredefinedMenuItem::separator(app);

    let app_menu = Submenu::with_items(
        app,
        "Magnesium",
        true,
        &[
            &make.command("about", "About Magnesium")?,
            &separator()?,
            &make.command("settings", "Settings…")?,
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
            &make.command("new-tab", "New Tab")?,
            &make.command("new-session", "New Session")?,
            &make.command("just-checks", "Just the Quick Checks")?,
            &make.command("open-workspace", "Open a Folder…")?,
            &separator()?,
            &make.command("close-session", "Close Session")?,
            &separator()?,
            &make.command("open-document", "Open Document…")?,
            &make.command("close-document", "Close Document")?,
            &make.command("share-document", "Share Document…")?,
            &separator()?,
            &make.command("attach", "Attach a Photo or PDF…")?,
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
            &make.command("home", "Home")?,
            &make.command("plan", "Show Plan")?,
            &make.command("expand-panel", "Expand Panel")?,
            &make.command("flip-panel", "Switch to the Document")?,
            &separator()?,
            &make.command("zoom-in-document", "Zoom In on the Document")?,
            &make.command("zoom-out-document", "Zoom Out of the Document")?,
            &make.command("zoom-fit-document", "Fit the Document to the Panel")?,
            &separator()?,
            &make.command("bigger", "Bigger Text")?,
            &make.command("smaller", "Smaller Text")?,
            &make.command("actual-size", "Actual Size")?,
            &separator()?,
            &make.command("commands", "Command Menu…")?,
            &separator()?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    let go = Submenu::with_id(app, GO, "Go", true)?;
    if spec.workspaces.is_empty() {
        go.append(&no_workspaces(app)?)?;
    }
    for n in 0..List::Go.named(spec).len() {
        go.append(&make.listed(List::Go, n)?)?;
    }

    let session = Submenu::with_items(
        app,
        "Session",
        true,
        &[
            &make.command("hint", "Hint")?,
            &make.command("lost", "I’m Lost")?,
            &make.command("unknown", "I Don’t Know")?,
            &separator()?,
            &make.command("look-back", "Show the Lesson")?,
            &separator()?,
            &make.command("symbols", "Symbols")?,
            &make.command("model", "Model and Effort…")?,
            &separator()?,
            &make.command("stop-reply", "Stop the Reply")?,
        ],
    )?;

    let window = Submenu::with_id(app, WINDOW, "Window", true)?;
    window.append_items(&[
        &PredefinedMenuItem::minimize(app, None)?,
        &PredefinedMenuItem::maximize(app, None)?,
        &separator()?,
        &make.item(
            "previous-tab",
            "previous-tab",
            None,
            "Show Previous Tab",
            Key::Fixed(Some("Cmd+Shift+BracketLeft".into())),
        )?,
        &make.item(
            "next-tab",
            "next-tab",
            None,
            "Show Next Tab",
            Key::Fixed(Some("Cmd+Shift+BracketRight".into())),
        )?,
    ])?;
    if !spec.tabs.is_empty() {
        window.append(&separator()?)?;
    }
    for n in 0..List::Tabs.named(spec).len() {
        window.append(&make.listed(List::Tabs, n)?)?;
    }

    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[&make.command("keys-help", "Keys and Commands")?],
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
