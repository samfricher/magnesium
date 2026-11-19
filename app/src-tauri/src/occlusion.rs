pub fn keep_drawing(window: &tauri::WebviewWindow) {
    detect(window, false);
}

#[cfg(target_os = "macos")]
pub fn detect(window: &tauri::WebviewWindow, on: bool) {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};

    let _ = window.with_webview(move |webview| {
        let view: &AnyObject = unsafe { &*(webview.inner() as *const AnyObject) };
        if view
            .class()
            .responds_to(sel!(_setWindowOcclusionDetectionEnabled:))
        {
            let _: () = unsafe { msg_send![view, _setWindowOcclusionDetectionEnabled: on] };
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn detect(_window: &tauri::WebviewWindow, _on: bool) {}
