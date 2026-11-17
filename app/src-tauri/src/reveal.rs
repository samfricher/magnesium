//! The window comes on screen with its page drawn.
//!
//! WebKit draws nothing for a window that is off screen, not even an
//! animation frame, so a window kept hidden until its page is ready would
//! still open on an unpainted page. Instead the window is put on screen
//! fully see-through as soon as it exists (it is made hidden, `visible: false`
//! in tauri.conf.json, so it is never seen before that), the page draws into
//! it as into any window on screen, and it turns opaque once the page says
//! its first screen is painted. While it waits, WebKit is told to draw it
//! although it counts as covered (see `occlusion`). Before this the window opened blank about
//! 0.2 s before its page filled it: white for a moment in the dark scheme too.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::Manager as _;

use crate::harness;

/// How long after setup the window shows whether or not its page is drawn:
/// a page that breaks while it starts still has its window seen.
const SHOW_ANYWAY: Duration = Duration::from_millis(1500);

/// Whether the window has been shown. Only the first show acts: a later one
/// would bring the window forward over whatever the learner has moved to.
static SHOWN: AtomicBool = AtomicBool::new(false);

/// Put the window on screen unseen, and start the wait after which it shows
/// anyway. Called from setup.
pub fn hold(app: &tauri::App) {
    if let Some(window) = app.get_webview_window("main") {
        see_through(&window, true);
        // Unseen, the window counts as covered, and WebKit would stop drawing
        // it, sometimes before the first screen, so it would turn opaque blank.
        crate::occlusion::detect(&window, false);
        let _ = window.show();
    }
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_ANYWAY);
        let main = handle.clone();
        let _ = handle.run_on_main_thread(move || show(&main, "window shown, page late"));
    });
}

/// Let the window be seen, the first time only.
pub fn show(app: &tauri::AppHandle, why: &str) {
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        see_through(&window, false);
        if !harness::driven() {
            crate::occlusion::detect(&window, true);
        }
        // A window shown after launch does not make the app active the way
        // one made visible at launch does: it would open behind the window
        // the learner launched it from, with the keys going there.
        let _ = window.set_focus();
        harness::note(why);
    }
}

/// The page's first screen is painted.
#[tauri::command]
pub fn show_window(app: tauri::AppHandle) {
    show(&app, "window shown, page drawn");
}

/// At once, not through `with_webview`, which waits its turn behind the page's
/// own work on the main thread. Every caller is on the main thread already:
/// setup, a command that is not async, the run loop's events.
#[cfg(target_os = "macos")]
fn see_through(window: &tauri::WebviewWindow, unseen: bool) {
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    // SAFETY: Tauri's handle on macOS is the window's NSWindow, alive while the window is, used on the main thread.
    let ns_window: &objc2_app_kit::NSWindow =
        unsafe { &*(pointer as *const objc2_app_kit::NSWindow) };
    ns_window.setAlphaValue(if unseen { 0.0 } else { 1.0 });
}

/// Elsewhere the window is simply shown from the start.
#[cfg(not(target_os = "macos"))]
fn see_through(_window: &tauri::WebviewWindow, _unseen: bool) {}
