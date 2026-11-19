use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;
use tauri::{Manager as _, State};

use crate::Shell;

pub const DRIVE_ENV: &str = "MAGNESIUM_APP_DRIVE";
pub const PERF_ENV: &str = "MAGNESIUM_APP_PERF";

static STARTED_MS: OnceLock<i64> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

pub fn mark_start() {
    let _ = STARTED_MS.set(now_ms());
}

pub fn note(what: &str) {
    if perf_file().is_some() {
        let started = STARTED_MS.get().copied().unwrap_or_else(now_ms);
        eprintln!("[startup] {what}: {} ms", now_ms() - started);
    }
}

fn named(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn driven() -> bool {
    drive_dir().is_some()
}

fn drive_dir() -> Option<PathBuf> {
    named(DRIVE_ENV)
}

fn perf_file() -> Option<PathBuf> {
    named(PERF_ENV)
}

pub fn keep_in_view(app: &tauri::App) {
    if drive_dir().is_none() {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(true);
        let _ = window.set_visible_on_all_workspaces(true);
        crate::occlusion::keep_drawing(&window);
    }
}

fn write_whole(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

fn last_answered(dir: &Path) -> u64 {
    std::fs::read_dir(dir.join("out"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json")?.parse::<u64>().ok()
        })
        .max()
        .unwrap_or(0)
}

#[derive(Serialize)]
pub struct Config {
    drive: bool,
    perf: bool,
    started_ms: i64,
}

#[tauri::command]
pub fn harness_config() -> Config {
    Config {
        drive: drive_dir().is_some(),
        perf: perf_file().is_some(),
        started_ms: STARTED_MS.get().copied().unwrap_or(0),
    }
}

#[tauri::command(async)]
pub fn harness_next() -> Option<Value> {
    let dir = drive_dir()?;
    if NEXT.load(Ordering::SeqCst) == 0 {
        let _ = std::fs::create_dir_all(dir.join("in"));
        let _ = std::fs::create_dir_all(dir.join("out"));
        let _ = NEXT.compare_exchange(
            0,
            last_answered(&dir) + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
    let n = NEXT.load(Ordering::SeqCst);
    let text = std::fs::read_to_string(dir.join("in").join(format!("{n}.json"))).ok()?;
    let mut value: Value = serde_json::from_str(&text).ok()?;
    if !value.is_object() {
        value = serde_json::json!({"op": value});
    }
    value["id"] = n.into();
    NEXT.store(n + 1, Ordering::SeqCst);
    Some(value)
}

#[tauri::command(async)]
pub fn harness_done(id: u64, result: Value) -> Result<(), String> {
    let Some(dir) = drive_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(dir.join("out")).map_err(|e| e.to_string())?;
    write_whole(&dir.join("out").join(format!("{id}.json")), &result)
}

#[tauri::command(async)]
pub fn harness_write(name: String, value: Value) -> Result<(), String> {
    if name == "perf" {
        return match perf_file() {
            Some(file) => write_whole(&file, &value),
            None => Ok(()),
        };
    }
    let plain = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    match drive_dir() {
        Some(dir) if plain => write_whole(&dir.join(format!("{name}.json")), &value),
        Some(_) => Err("not a plain name".into()),
        None => Ok(()),
    }
}

#[tauri::command(async)]
pub fn harness_idle(shell: State<'_, Shell>) -> bool {
    drive_dir().is_none() || !shell.manager.busy()
}

#[tauri::command(async)]
pub fn harness_menu(app: tauri::AppHandle) -> Result<Value, String> {
    if drive_dir().is_none() {
        return Err("the harness is off".into());
    }
    Ok(crate::menu::read(&app))
}

#[tauri::command(async)]
pub fn harness_menu_replaced(app: tauri::AppHandle) -> Result<usize, String> {
    if drive_dir().is_none() {
        return Err("the harness is off".into());
    }
    Ok(app.state::<crate::menu::MenuBar>().replaced())
}

#[tauri::command(async)]
pub fn harness_menu_native(app: tauri::AppHandle) -> Result<Value, String> {
    if drive_dir().is_none() {
        return Err("the harness is off".into());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(native_menu());
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn native_menu() -> Value {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSMenu};

    fn items(menu: &NSMenu) -> Vec<Value> {
        menu.itemArray()
            .iter()
            .map(|item| {
                if item.isSeparatorItem() {
                    return Value::from("—");
                }
                let title = item.title().to_string();
                let key = item.keyEquivalent().to_string();
                Value::from(if key.is_empty() {
                    title
                } else {
                    format!("{title} ({key})")
                })
            })
            .collect()
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return Value::Null;
    };
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else {
        return Value::Null;
    };
    Value::from(
        bar.itemArray()
            .iter()
            .map(|top| {
                serde_json::json!({
                    "title": top.submenu().map(|menu| menu.title().to_string()).unwrap_or_default(),
                    "items": top.submenu().map(|menu| items(&menu)).unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(not(target_os = "macos"))]
fn native_menu() -> Value {
    Value::Null
}

#[tauri::command(async)]
pub fn harness_menu_click(app: tauri::AppHandle, path: Vec<String>) -> Result<String, String> {
    if drive_dir().is_none() {
        return Err("the harness is off".into());
    }
    crate::menu::click(&app, &path)
}

#[tauri::command(async)]
pub fn harness_frame(
    app: tauri::AppHandle,
    width: Option<f64>,
    height: Option<f64>,
    x: Option<f64>,
    y: Option<f64>,
    on_top: Option<bool>,
) -> Result<Value, String> {
    if drive_dir().is_none() {
        return Err("the harness is off".into());
    }
    let window = app.get_webview_window("main").ok_or("no window")?;
    if let Some(on_top) = on_top {
        window
            .set_always_on_top(on_top)
            .map_err(|e| e.to_string())?;
    }
    if let (Some(width), Some(height)) = (width, height) {
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
    }
    if let (Some(x), Some(y)) = (x, y) {
        window
            .set_position(tauri::LogicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
    }
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let size = window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    let place = window
        .outer_position()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    Ok(serde_json::json!({"width": size.width, "height": size.height, "x": place.x, "y": place.y}))
}

#[tauri::command(async)]
pub fn harness_quit(app: tauri::AppHandle) {
    if drive_dir().is_none() {
        return;
    }
    app.state::<Shell>().manager.shut_down();
    app.exit(0);
}
