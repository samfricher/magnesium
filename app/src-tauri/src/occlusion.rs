//! Keep the window drawing while another window covers it.
//!
//! WebKit treats a web view whose window is covered (or on a Space that is
//! not showing) as hidden: it stops animation frames and painting until the
//! window is seen again. The page draws a PDF page and the Open… list on
//! those frames, so either stayed blank behind another window until the
//! learner brought it forward. WebKit's own switch for that is not public on
//! macOS; it is asked for only where the web view answers to it.

#[cfg(target_os = "macos")]
pub fn keep_drawing(window: &tauri::WebviewWindow) {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};

    let _ = window.with_webview(|webview| {
        // SAFETY: wry's handle on macOS is the WKWebView, alive while the window is.
        let view: &AnyObject = unsafe { &*(webview.inner() as *const AnyObject) };
        if view
            .class()
            .responds_to(sel!(_setWindowOcclusionDetectionEnabled:))
        {
            // SAFETY: the selector takes one BOOL and returns nothing; the web view answered to it above.
            let _: () = unsafe { msg_send![view, _setWindowOcclusionDetectionEnabled: false] };
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn keep_drawing(_window: &tauri::WebviewWindow) {}
