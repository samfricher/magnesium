#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mg_core::activity::Activity;
use mg_core::agent::{Account, AgentEvent, Answer, Decision, Intent, Payment, Permissions};
use mg_core::claude::ClaudeCode;
use mg_core::direct::DirectTutor;
use mg_core::files::Source;
use mg_core::install::RuntimeSetup;
use mg_core::manager::{EngineChoice, ModelChoice, Opened, Setup};
use mg_core::material::{Added, Listed};
use mg_core::plan::Plan;
use mg_core::runtime::Mark;
use mg_core::store::{Engine, PaperSitting, Turn};
use mg_core::study::{Card, Home, Paper, Progress, Rail};
use mg_core::workspace::{LessonInfo, Workspace};
use mg_core::{Manager, Store};
use serde::Serialize;
use tauri::{Emitter, Manager as _, RunEvent, State, WindowEvent};

mod glass;
mod harness;
mod menu;
mod occlusion;
mod reveal;
mod share;

const SELFTEST_ENV: &str = "MAGNESIUM_APP_SELFTEST";
const FOLDER_ENV: &str = "MAGNESIUM_APP_FOLDER";

pub(crate) struct Shell {
    manager: Manager,
}

type Reply<T> = Result<T, String>;

fn plain<T, E: std::fmt::Display>(result: Result<T, E>) -> Reply<T> {
    result.map_err(|error| error.to_string())
}

fn named(folder: &Option<String>) -> Option<&Path> {
    folder.as_deref().filter(|f| !f.is_empty()).map(Path::new)
}

#[tauri::command(async)]
fn initial_folder() -> Option<String> {
    std::env::args()
        .skip(1)
        .chain(std::env::var(FOLDER_ENV).ok())
        .find(|arg| !arg.starts_with('-') && Path::new(arg).is_dir())
}

#[tauri::command(async)]
fn recent(shell: State<'_, Shell>) -> Vec<String> {
    shell.manager.recent()
}

#[tauri::command(async)]
fn library(shell: State<'_, Shell>) -> Vec<Card> {
    shell.manager.library()
}

#[tauri::command(async)]
fn forget_folder(shell: State<'_, Shell>, path: String) -> Reply<()> {
    plain(shell.manager.forget_folder(Path::new(&path)))
}

#[tauri::command(async)]
fn find_folders(shell: State<'_, Shell>) -> usize {
    shell.manager.find_folders()
}

#[tauri::command(async)]
fn activity(shell: State<'_, Shell>) -> Activity {
    shell.manager.activity()
}

#[tauri::command(async)]
fn open_folder(shell: State<'_, Shell>, path: String) -> Reply<Home> {
    plain(shell.manager.open_folder(Path::new(&path)))
}

#[tauri::command(async)]
fn home(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Home> {
    plain(shell.manager.home_at(named(&folder)))
}

#[tauri::command(async)]
fn visit_folder(shell: State<'_, Shell>, path: String) -> Reply<Home> {
    plain(shell.manager.visit_folder(Path::new(&path)))
}

#[tauri::command(async)]
fn workspaces(shell: State<'_, Shell>) -> Vec<Workspace> {
    shell.manager.workspaces()
}

#[tauri::command(async)]
fn lessons(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Vec<LessonInfo>> {
    plain(shell.manager.lessons(named(&folder)))
}

#[tauri::command(async)]
fn lesson_turns(
    shell: State<'_, Shell>,
    folder: Option<String>,
    session: String,
) -> Reply<Vec<Turn>> {
    plain(shell.manager.lesson_turns(named(&folder), &session))
}

#[tauri::command(async)]
fn touch_session(shell: State<'_, Shell>, folder: Option<String>, session: String) -> Reply<()> {
    plain(shell.manager.touch_session(named(&folder), &session))
}

#[tauri::command(async)]
fn window_state(shell: State<'_, Shell>) -> Option<serde_json::Value> {
    shell.manager.window_state()
}

#[tauri::command(async)]
fn set_window_state(shell: State<'_, Shell>, value: serde_json::Value) {
    shell.manager.set_window_state(value);
}

#[tauri::command]
fn close_window(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command(async)]
fn set_menu(app: tauri::AppHandle, spec: menu::Spec) -> Reply<()> {
    let bar = app.state::<menu::MenuBar>();
    if bar.spec() == spec {
        return Ok(());
    }
    plain(bar.apply(&app, spec))
}

#[tauri::command(async)]
fn set_up(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Home> {
    plain(shell.manager.set_up_at(named(&folder)))
}

#[tauri::command(async)]
fn upgrade_folder(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Home> {
    plain(shell.manager.upgrade_folder_at(named(&folder)))
}

#[tauri::command(async)]
fn new_folder(shell: State<'_, Shell>, path: String) -> Reply<Home> {
    plain(shell.manager.new_folder(Path::new(&path)))
}

#[tauri::command(async)]
fn setup(shell: State<'_, Shell>) -> Setup {
    shell.manager.setup()
}

#[tauri::command(async)]
fn replace_runtime(shell: State<'_, Shell>) -> RuntimeSetup {
    shell.manager.replace_runtime()
}

#[tauri::command(async)]
fn keep_runtime(shell: State<'_, Shell>) -> RuntimeSetup {
    shell.manager.keep_runtime()
}

#[tauri::command(async)]
fn account(shell: State<'_, Shell>) -> Account {
    shell.manager.account()
}

#[tauri::command(async)]
fn set_payment(shell: State<'_, Shell>, payment: Payment) -> Account {
    shell.manager.set_payment(payment)
}

#[tauri::command(async)]
fn set_permissions(shell: State<'_, Shell>, permissions: Permissions) -> Reply<()> {
    plain(shell.manager.set_permissions(permissions))
}

#[tauri::command(async)]
fn sign_in(shell: State<'_, Shell>) -> Reply<()> {
    plain(shell.manager.sign_in())
}

#[derive(Serialize)]
struct About {
    version: String,
    commit: Option<String>,
}

#[tauri::command]
fn about(app: tauri::AppHandle) -> About {
    About {
        version: app.package_info().version.to_string(),
        commit: option_env!("MAGNESIUM_BUILD_COMMIT")
            .map(str::trim)
            .filter(|commit| !commit.is_empty())
            .map(str::to_owned),
    }
}

#[tauri::command(async)]
fn engine(shell: State<'_, Shell>) -> EngineChoice {
    shell.manager.engine()
}

#[tauri::command(async)]
fn set_engine(shell: State<'_, Shell>, engine: Engine) -> EngineChoice {
    shell.manager.set_engine(engine)
}

#[tauri::command(async)]
fn models(shell: State<'_, Shell>) -> ModelChoice {
    shell.manager.models()
}

#[tauri::command(async)]
fn ask_models(shell: State<'_, Shell>) -> ModelChoice {
    shell.manager.ask_models()
}

#[tauri::command(async)]
fn set_pick(shell: State<'_, Shell>, model: String, effort: Option<String>) -> Reply<ModelChoice> {
    plain(shell.manager.set_pick(&model, effort.as_deref()))
}

#[tauri::command(async)]
fn session_models(
    shell: State<'_, Shell>,
    folder: Option<String>,
    session: Option<String>,
    live: bool,
) -> ModelChoice {
    shell
        .manager
        .models_at(named(&folder), session.as_deref(), live)
}

#[tauri::command(async)]
fn set_session_model(
    shell: State<'_, Shell>,
    folder: Option<String>,
    session: Option<String>,
    live: bool,
    model: String,
    effort: Option<String>,
) -> Reply<ModelChoice> {
    plain(shell.manager.set_model_at(
        named(&folder),
        session.as_deref(),
        live,
        &model,
        effort.as_deref(),
    ))
}

#[tauri::command(async)]
fn open_session(
    shell: State<'_, Shell>,
    resume: bool,
    folder: Option<String>,
    session: Option<String>,
    title: Option<String>,
) -> Reply<Opened> {
    plain(shell.manager.open_session_at(
        named(&folder),
        resume,
        session.as_deref(),
        title.as_deref(),
    ))
}

#[tauri::command(async)]
fn send(shell: State<'_, Shell>, text: String, intent: Option<Intent>) -> Reply<()> {
    plain(shell.manager.send_with(&text, intent))
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
fn turns(shell: State<'_, Shell>) -> Reply<Vec<Turn>> {
    plain(shell.manager.saved_turns())
}

#[tauri::command(async)]
fn plan(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Plan> {
    plain(shell.manager.plan_at(named(&folder)))
}

#[tauri::command(async)]
fn progress(
    shell: State<'_, Shell>,
    folder: Option<String>,
    target: Option<String>,
) -> Reply<Progress> {
    plain(shell.manager.progress_at(named(&folder), target.as_deref()))
}

#[tauri::command(async)]
fn set_block_minutes(shell: State<'_, Shell>, minutes: u32) {
    shell.manager.set_block_minutes(minutes);
}

#[tauri::command(async)]
fn attach(
    shell: State<'_, Shell>,
    folder: Option<String>,
    path: String,
    group: String,
) -> Reply<String> {
    plain(
        shell
            .manager
            .attach_at(named(&folder), Path::new(&path), &group),
    )
}

#[tauri::command(async)]
fn attach_bytes(
    shell: State<'_, Shell>,
    folder: Option<String>,
    name: String,
    bytes: Vec<u8>,
    group: String,
) -> Reply<String> {
    plain(
        shell
            .manager
            .attach_bytes_at(named(&folder), &name, &bytes, &group),
    )
}

#[tauri::command(async)]
fn add_material(
    shell: State<'_, Shell>,
    folder: Option<String>,
    paths: Vec<String>,
) -> Reply<Added> {
    let sources: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    plain(shell.manager.add_material_at(named(&folder), &sources))
}

#[tauri::command(async)]
fn marks(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Option<Vec<Mark>>> {
    plain(shell.manager.marks_at(named(&folder)))
}

#[tauri::command(async)]
fn material_list(shell: State<'_, Shell>, folder: Option<String>) -> Reply<Listed> {
    plain(shell.manager.material_list_at(named(&folder)))
}

#[tauri::command(async)]
fn add_pasted_text(shell: State<'_, Shell>, folder: Option<String>, text: String) -> Reply<Added> {
    plain(shell.manager.add_pasted_text_at(named(&folder), &text))
}

#[tauri::command(async)]
fn source(shell: State<'_, Shell>, folder: Option<String>, path: String) -> Reply<Source> {
    plain(shell.manager.source_at(named(&folder), &path))
}

#[tauri::command(async)]
fn source_pdf(
    shell: State<'_, Shell>,
    folder: Option<String>,
    path: String,
) -> Result<tauri::ipc::Response, String> {
    plain(shell.manager.source_pdf_at(named(&folder), &path)).map(tauri::ipc::Response::new)
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn share_file(
    app: tauri::AppHandle,
    shell: State<'_, Shell>,
    folder: Option<String>,
    path: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Reply<()> {
    let file = plain(shell.manager.shareable_at(named(&folder), &path))?;
    let window = app
        .get_webview_window("main")
        .ok_or("The window is not open.")?;
    share::show(
        &window,
        file,
        share::Anchor {
            x,
            y,
            width,
            height,
        },
    )
}

#[tauri::command(async)]
fn rename_session(
    shell: State<'_, Shell>,
    folder: Option<String>,
    session: String,
    title: String,
) -> Reply<()> {
    plain(
        shell
            .manager
            .rename_session(named(&folder), &session, &title),
    )
}

#[tauri::command(async)]
fn open_file(shell: State<'_, Shell>, folder: Option<String>, path: String) -> Reply<()> {
    plain(shell.manager.open_file_at(named(&folder), &path))
}

#[tauri::command(async)]
fn image(shell: State<'_, Shell>, folder: Option<String>, path: String) -> Reply<String> {
    plain(shell.manager.image_at(named(&folder), &path))
}

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
    if cfg!(target_os = "macos") {
        let _ = child.wait();
    } else {
        std::thread::spawn(move || child.wait());
    }
    Ok(())
}

#[tauri::command(async)]
fn paper_start(
    shell: State<'_, Shell>,
    folder: Option<String>,
    paper: Paper,
    minutes: u32,
) -> Reply<PaperSitting> {
    plain(
        shell
            .manager
            .paper_start_at(named(&folder), &paper, minutes),
    )
}

#[derive(Serialize)]
struct Words {
    shown: String,
    sent: String,
}

#[tauri::command(async)]
fn paper_end(
    shell: State<'_, Shell>,
    folder: Option<String>,
    files: Vec<String>,
) -> Reply<Option<Words>> {
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    plain(shell.manager.paper_end_at(named(&folder), &files))
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
    harness::mark_start();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            harness::keep_in_view(app);
            harness::note("setup start");
            mg_core::paths::warm_up();
            let handle = app.handle().clone();
            let store = Arc::new(Store::open());
            let manager = Manager::new(
                Arc::clone(&store),
                Arc::new(ClaudeCode::new()),
                Arc::new(move |event: AgentEvent| {
                    let _ = handle.emit("agent", event);
                }),
            )
            .with_direct(Arc::new(DirectTutor::new().with_store(store)));
            app.manage(Shell { manager });
            let asker = app.handle().clone();
            std::thread::spawn(move || {
                asker.state::<Shell>().manager.ask_models();
            });
            let bar = menu::MenuBar::default();
            bar.apply(app.handle(), menu::Spec::default())?;
            app.manage(bar);
            app.on_menu_event(|app, event| {
                app.state::<menu::MenuBar>()
                    .chosen(app, event.id().as_ref());
            });
            harness::note("setup done");
            reveal::hold(app);
            if let Some(path) = std::env::var_os(SELFTEST_ENV).filter(|p| !p.is_empty()) {
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
        .on_page_load(|_webview, payload| {
            harness::note(match payload.event() {
                tauri::webview::PageLoadEvent::Started => "page load started",
                tauri::webview::PageLoadEvent::Finished => "page load finished",
            });
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                window.state::<Shell>().manager.shut_down();
            }
        })
        .invoke_handler(tauri::generate_handler![
            initial_folder,
            recent,
            library,
            forget_folder,
            find_folders,
            activity,
            open_folder,
            home,
            visit_folder,
            workspaces,
            lessons,
            lesson_turns,
            touch_session,
            rename_session,
            share_file,
            window_state,
            set_window_state,
            close_window,
            set_menu,
            set_up,
            upgrade_folder,
            new_folder,
            setup,
            replace_runtime,
            keep_runtime,
            account,
            set_payment,
            set_permissions,
            sign_in,
            about,
            reveal::show_window,
            engine,
            set_engine,
            models,
            ask_models,
            set_pick,
            session_models,
            set_session_model,
            open_session,
            send,
            send_new_lesson,
            send_shown,
            interrupt,
            decide,
            answer,
            stop,
            rail,
            turns,
            plan,
            progress,
            set_block_minutes,
            attach,
            attach_bytes,
            add_material,
            add_pasted_text,
            marks,
            material_list,
            source,
            source_pdf,
            open_file,
            image,
            open_url,
            paper_start,
            paper_end,
            selftest,
            selftest_report,
            glass::glass_state,
            glass::set_glass,
            harness::harness_config,
            harness::harness_next,
            harness::harness_done,
            harness::harness_write,
            harness::harness_idle,
            harness::harness_quit,
            harness::harness_menu,
            harness::harness_menu_replaced,
            harness::harness_menu_native,
            harness::harness_menu_click,
            harness::harness_frame,
        ])
        .build(tauri::generate_context!())
        .expect("the window could not start")
        .run(|app, event| {
            if let RunEvent::Reopen { .. } = event {
                reveal::show(app, "window shown, Dock");
            }
        });
}
