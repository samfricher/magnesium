//! The window's shell. It only passes messages between the page and
//! `mg-core`: every decision about the agent and the study folder is made
//! there, where it is tested.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mg_core::agent::{AgentEvent, Answer, Decision};
use mg_core::claude::ClaudeCode;
use mg_core::manager::Opened;
use mg_core::store::PaperSitting;
use mg_core::study::{Home, Paper, Progress, Rail};
use mg_core::{Manager, Store};
use serde::Serialize;
use tauri::{Emitter, Manager as _, State, WindowEvent};

/// When set to a file path, the window runs a short check of itself, writes
/// what it found to that file, and quits. See `scripts/selftest.sh`.
const SELFTEST_ENV: &str = "MAGNESIUM_APP_SELFTEST";
const FOLDER_ENV: &str = "MAGNESIUM_APP_FOLDER";

struct Shell {
    manager: Manager,
}

type Reply<T> = Result<T, String>;

fn plain<T, E: std::fmt::Display>(result: Result<T, E>) -> Reply<T> {
    result.map_err(|error| error.to_string())
}

/// The folder to open first: one named on the command line or in the
/// environment, else the opened folder whose exam is nearest.
#[tauri::command(async)]
fn initial_folder(shell: State<'_, Shell>) -> Option<String> {
    std::env::args()
        .skip(1)
        .chain(std::env::var(FOLDER_ENV).ok())
        .find(|arg| !arg.starts_with('-') && Path::new(arg).is_dir())
        .or_else(|| shell.manager.folder_to_open())
}

#[tauri::command(async)]
fn recent(shell: State<'_, Shell>) -> Vec<String> {
    shell.manager.recent()
}

#[tauri::command(async)]
fn open_folder(shell: State<'_, Shell>, path: String) -> Reply<Home> {
    plain(shell.manager.open_folder(Path::new(&path)))
}

#[tauri::command(async)]
fn home(shell: State<'_, Shell>) -> Reply<Home> {
    plain(shell.manager.home())
}

#[tauri::command(async)]
fn set_up(shell: State<'_, Shell>) -> Reply<Home> {
    plain(shell.manager.set_up())
}

#[tauri::command(async)]
fn open_session(shell: State<'_, Shell>, resume: bool) -> Reply<Opened> {
    plain(shell.manager.open_session(resume))
}

#[tauri::command(async)]
fn send(shell: State<'_, Shell>, text: String) -> Reply<()> {
    plain(shell.manager.send(&text))
}

#[tauri::command(async)]
fn send_new_lesson(shell: State<'_, Shell>, goal: String) -> Reply<()> {
    plain(shell.manager.send_new_lesson(&goal))
}

#[tauri::command(async)]
fn send_shown(shell: State<'_, Shell>, shown: String, sent: String) -> Reply<()> {
    plain(shell.manager.send_shown(&shown, &sent))
}

#[tauri::command(async)]
fn interrupt(shell: State<'_, Shell>) -> Reply<()> {
    plain(shell.manager.interrupt())
}

#[tauri::command(async)]
fn decide(shell: State<'_, Shell>, id: String, decision: Decision) -> Reply<()> {
    plain(shell.manager.decide(&id, decision))
}

#[tauri::command(async)]
fn answer(shell: State<'_, Shell>, id: String, answers: Vec<Answer>) -> Reply<()> {
    plain(shell.manager.answer(&id, &answers))
}

#[tauri::command(async)]
fn stop(shell: State<'_, Shell>, stopped_at: Option<String>) -> Reply<()> {
    plain(shell.manager.stop(stopped_at.as_deref()))
}

#[tauri::command(async)]
fn rail(shell: State<'_, Shell>) -> Reply<Rail> {
    plain(shell.manager.rail())
}

#[tauri::command(async)]
fn progress(shell: State<'_, Shell>, target: Option<String>) -> Reply<Progress> {
    plain(shell.manager.progress(target.as_deref()))
}

#[tauri::command(async)]
fn set_progress_view(shell: State<'_, Shell>, view: String) {
    shell.manager.set_progress_view(&view);
}

#[tauri::command(async)]
fn set_block_minutes(shell: State<'_, Shell>, minutes: u32) {
    shell.manager.set_block_minutes(minutes);
}

#[tauri::command(async)]
fn attach(shell: State<'_, Shell>, path: String, group: String) -> Reply<String> {
    plain(shell.manager.attach(Path::new(&path), &group))
}

#[tauri::command(async)]
fn open_file(shell: State<'_, Shell>, path: String) -> Reply<()> {
    plain(shell.manager.open_file(&path))
}

#[tauri::command(async)]
fn image(shell: State<'_, Shell>, path: String) -> Reply<String> {
    plain(shell.manager.image(&path))
}

/// Open a web address in the learner's browser. Only web addresses: the page
/// shows text the tutor wrote, and a link in it must not start a program.
#[tauri::command(async)]
fn open_url(url: String) -> Reply<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map_err(|error| error.to_string())?;
    // Waited for, so no finished process is left behind. `open` returns at
    // once; `xdg-open` can wait for the browser, so it is waited for apart.
    if cfg!(target_os = "macos") {
        let _ = child.wait();
    } else {
        std::thread::spawn(move || child.wait());
    }
    Ok(())
}

#[tauri::command(async)]
fn paper_start(shell: State<'_, Shell>, paper: Paper, minutes: u32) -> Reply<PaperSitting> {
    plain(shell.manager.paper_start(&paper, minutes))
}

#[derive(Serialize)]
struct Words {
    shown: String,
    sent: String,
}

#[tauri::command(async)]
fn paper_end(shell: State<'_, Shell>, files: Vec<String>) -> Reply<Option<Words>> {
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    plain(shell.manager.paper_end(&files))
        .map(|words| words.map(|(shown, sent)| Words { shown, sent }))
}

#[tauri::command]
fn selftest() -> bool {
    std::env::var_os(SELFTEST_ENV).is_some_and(|path| !path.is_empty())
}

#[tauri::command(async)]
fn selftest_report(app: tauri::AppHandle, report: serde_json::Value) {
    if let Some(path) = std::env::var_os(SELFTEST_ENV) {
        let text = serde_json::to_string_pretty(&report).unwrap_or_default();
        let _ = std::fs::write(path, text);
    }
    app.exit(0);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            mg_core::paths::warm_up();
            let handle = app.handle().clone();
            let manager = Manager::new(
                Arc::new(Store::open()),
                Arc::new(ClaudeCode::new()),
                Arc::new(move |event: AgentEvent| {
                    let _ = handle.emit("agent", event);
                }),
            );
            app.manage(Shell { manager });
            if let Some(path) = std::env::var_os(SELFTEST_ENV).filter(|p| !p.is_empty()) {
                // A check that never reports must still end.
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(90));
                    let _ =
                        std::fs::write(&path, r#"{"ok":false,"error":"the page never reported"}"#);
                    handle.exit(1);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                window.state::<Shell>().manager.shut_down();
            }
        })
        .invoke_handler(tauri::generate_handler![
            initial_folder,
            recent,
            open_folder,
            home,
            set_up,
            open_session,
            send,
            send_new_lesson,
            send_shown,
            interrupt,
            decide,
            answer,
            stop,
            rail,
            progress,
            set_progress_view,
            set_block_minutes,
            attach,
            open_file,
            image,
            open_url,
            paper_start,
            paper_end,
            selftest,
            selftest_report,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not start");
}
