//! The evidence harness's half in the shell: a dev-only hook that lets the
//! real window be driven, photographed and timed. Every command here is
//! inert unless the window was started with one of two variables:
//!
//! - `MAGNESIUM_APP_DRIVE=<dir>`: the page reads instructions from
//!   `<dir>/in/<n>.json`, in order from 1, and answers each in
//!   `<dir>/out/<n>.json` (see `scripts/drive.mjs`).
//! - `MAGNESIUM_APP_PERF=<file>`: the page writes what it measured to that
//!   file (see `src/harness.ts`).
//!
//! It reads and writes nothing outside those paths and makes no request.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;
use tauri::{Manager as _, State};

use crate::Shell;

pub const DRIVE_ENV: &str = "MAGNESIUM_APP_DRIVE";
pub const PERF_ENV: &str = "MAGNESIUM_APP_PERF";

/// When the process started, in ms since the epoch: the top of `main`.
static STARTED_MS: OnceLock<i64> = OnceLock::new();
/// The number of the next instruction to read.
static NEXT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// Note the start time. Called first thing in `main`.
pub fn mark_start() {
    let _ = STARTED_MS.set(now_ms());
}

/// With perf on, say on standard error how long after the start `what`
/// happened: the shell's own moments before the page runs (setup done, the
/// page starting and finishing to load), beside the page's own marks.
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

fn drive_dir() -> Option<PathBuf> {
    named(DRIVE_ENV)
}

fn perf_file() -> Option<PathBuf> {
    named(PERF_ENV)
}

/// Write a file whole: to a sibling first, then renamed into place, so a
/// reader never sees half of it.
fn write_whole(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

/// The highest instruction number answered in an earlier window, so a
/// window started again on the same folder goes on from there.
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

/// The next instruction, with its number as `id`, or none yet.
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

/// Answer instruction `id`.
#[tauri::command(async)]
pub fn harness_done(id: u64, result: Value) -> Result<(), String> {
    let Some(dir) = drive_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(dir.join("out")).map_err(|e| e.to_string())?;
    write_whole(&dir.join("out").join(format!("{id}.json")), &result)
}

/// Keep something the page measured. "perf" is the perf file; any other
/// plain name is `<name>.json` in the drive folder.
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

/// Whether a reply is running or the record is still being read after one.
#[tauri::command(async)]
pub fn harness_idle(shell: State<'_, Shell>) -> bool {
    drive_dir().is_none() || !shell.manager.busy()
}

/// Close the window as the learner would, after the last answer is written.
#[tauri::command(async)]
pub fn harness_quit(app: tauri::AppHandle) {
    if drive_dir().is_none() {
        return;
    }
    app.state::<Shell>().manager.shut_down();
    app.exit(0);
}
