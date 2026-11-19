use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(target_os = "macos")]
pub fn show(window: &tauri::WebviewWindow, file: PathBuf, at: Anchor) -> Result<(), String> {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSSharingServicePicker, NSView};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

    thread_local! {
        static SHOWN: RefCell<Option<Retained<NSSharingServicePicker>>> = const { RefCell::new(None) };
    }

    window
        .with_webview(move |webview| {
            let Some(main) = MainThreadMarker::new() else {
                return;
            };
            let view: &NSView = unsafe { &*(webview.inner() as *const NSView) };
            let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
            let item: &AnyObject = &url;
            let items = NSArray::from_slice(&[item]);
            let picker = unsafe { NSSharingServicePicker::initWithItems(main.alloc(), &items) };
            let flipped = view.isFlipped();
            let bounds = view.bounds();
            let y = if flipped {
                at.y
            } else {
                bounds.size.height - at.y - at.height
            };
            let rect = NSRect::new(
                NSPoint::new(at.x, y),
                NSSize::new(at.width.max(1.0), at.height.max(1.0)),
            );
            picker.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
            SHOWN.with(|kept| *kept.borrow_mut() = Some(picker));
        })
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn show(_window: &tauri::WebviewWindow, _file: PathBuf, _at: Anchor) -> Result<(), String> {
    Err("Sharing is on the Mac only.".into())
}
