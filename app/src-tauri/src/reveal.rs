use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::Manager as _;

use crate::harness;

const SHOW_ANYWAY: Duration = Duration::from_millis(1500);

static SHOWN: AtomicBool = AtomicBool::new(false);

pub fn hold(app: &tauri::App) {
    if let Some(window) = app.get_webview_window("main") {
        see_through(&window, true);
        crate::occlusion::detect(&window, false);
        let _ = window.show();
        let _ = window.set_focus();
    }
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_ANYWAY);
        let main = handle.clone();
        let _ = handle.run_on_main_thread(move || show(&main, "window shown, page late"));
    });
}

pub fn show(app: &tauri::AppHandle, why: &str) {
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        see_through(&window, false);
        if !harness::driven() {
            crate::occlusion::detect(&window, true);
        }
        harness::note(why);
    }
}

#[tauri::command]
pub fn show_window(app: tauri::AppHandle) {
    show(&app, "window shown, page drawn");
}

#[cfg(target_os = "macos")]
fn see_through(window: &tauri::WebviewWindow, unseen: bool) {
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    let ns_window: &objc2_app_kit::NSWindow =
        unsafe { &*(pointer as *const objc2_app_kit::NSWindow) };
    ns_window.setAlphaValue(if unseen { 0.0 } else { 1.0 });
    ns_window.setIgnoresMouseEvents(unseen);
}

#[cfg(not(target_os = "macos"))]
fn see_through(_window: &tauri::WebviewWindow, _unseen: bool) {}
