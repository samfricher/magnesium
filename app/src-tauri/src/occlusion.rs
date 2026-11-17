//! Keep a driven window drawing while another window covers it.
//!
//! WebKit treats a web view whose window is covered (or on a Space that is
//! not showing) as hidden: it stops animation frames and painting until the
//! window is seen again. A check that drives the window behind other windows
//! would then wait for paints that never come. WebKit's switch for that is
//! Apple-internal, so it is asked for only where the web view answers to it:
//! by the harness for the whole run (`harness::keep_in_view`), and for a
//! learner's window only while it waits unseen for its first screen
//! (`reveal`), as a window with no opacity counts as covered.

pub fn keep_drawing(window: &tauri::WebviewWindow) {
    detect(window, false);
}

/// Whether WebKit stops drawing while the window is covered: its usual way, or
/// off.
#[cfg(target_os = "macos")]
pub fn detect(window: &tauri::WebviewWindow, on: bool) {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};

    let _ = window.with_webview(move |webview| {
        // SAFETY: wry's handle on macOS is the WKWebView, alive while the window is.
        let view: &AnyObject = unsafe { &*(webview.inner() as *const AnyObject) };
        if view
            .class()
            .responds_to(sel!(_setWindowOcclusionDetectionEnabled:))
        {
            // SAFETY: the selector takes one BOOL and returns nothing; the web view answered to it above.
            let _: () = unsafe { msg_send![view, _setWindowOcclusionDetectionEnabled: on] };
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn detect(_window: &tauri::WebviewWindow, _on: bool) {}
