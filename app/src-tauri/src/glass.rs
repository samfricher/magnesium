//! Frosted glass behind the page, when the learner chooses it in Settings.
//!
//! The window opens opaque, exactly as it always has. Only when the page asks
//! for glass does it turn the window clear, stop the web view painting a
//! ground of its own, and put the Mac's own glass under the page; the page
//! then paints its ground part-way see-through (`data-glass` in the styles).
//! Turning it off puts every one of those back.
//!
//! The blur itself is AppKit's: on macOS 26 and later an `NSGlassEffectView`
//! (Liquid Glass, regular), and before that an `NSVisualEffectView` with the
//! under-window material, both through Tauri's window effects (the
//! `window-vibrancy` crate). The window server draws it, not the page, so the
//! page's scrolling and typing do no more work than over an opaque ground.

use serde::Serialize;

/// What this Mac can do, as the page needs to know it before it asks.
#[derive(Serialize)]
pub struct GlassState {
    /// The window can be made see-through here.
    can: bool,
    /// The learner asked the Mac to reduce transparency or increase contrast
    /// (System Settings, Accessibility, Display). Glass is not drawn then, as
    /// AppKit's own materials turn opaque under Reduce transparency.
    reduce: bool,
}

#[tauri::command]
pub fn glass_state() -> GlassState {
    GlassState {
        can: cfg!(target_os = "macos"),
        reduce: reduce_transparency(),
    }
}

/// Put glass under the page, or take it away. `dark` is the page's scheme
/// when the learner chose one; with none the glass follows the Mac.
#[tauri::command]
pub fn set_glass(window: tauri::WebviewWindow, on: bool, dark: Option<bool>) -> Result<(), String> {
    use tauri::window::{Effect, EffectsBuilder};
    use tauri::Theme;

    if on {
        // The glass is tinted by the window's appearance: it has to match the page's scheme, not the Mac's.
        window
            .set_theme(dark.map(|dark| if dark { Theme::Dark } else { Theme::Light }))
            .map_err(|e| e.to_string())?;
        clear(&window, true)?;
        // Liquid Glass where the Mac has it; Tauri falls back to the second effect on macOS 15 and earlier.
        window
            .set_effects(
                EffectsBuilder::new()
                    .effect(Effect::LiquidGlassRegular)
                    .effect(Effect::UnderWindowBackground)
                    .build(),
            )
            .map_err(|e| e.to_string())
    } else {
        window.set_effects(None).map_err(|e| e.to_string())?;
        clear(&window, false)?;
        window.set_theme(None).map_err(|e| e.to_string())
    }
}

/// Make the window and its web view see-through, or opaque as they open.
#[cfg(target_os = "macos")]
fn clear(window: &tauri::WebviewWindow, see_through: bool) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSColor, NSWindow};
    use objc2_foundation::{ns_string, NSNumber};

    window
        .with_webview(move |webview| {
            // SAFETY: wry's handles on macOS are the WKWebView and its NSWindow, alive while the window is,
            // and this runs on the main thread.
            unsafe {
                let ns_window: &NSWindow = &*(webview.ns_window() as *const NSWindow);
                ns_window.setOpaque(!see_through);
                let ground = if see_through {
                    NSColor::clearColor()
                } else {
                    NSColor::windowBackgroundColor()
                };
                ns_window.setBackgroundColor(Some(&ground));
                // `drawsBackground` is the key wry itself sets for a transparent web view: WebKit has no
                // public property for it.
                let view: &AnyObject = &*(webview.inner() as *const AnyObject);
                let draws = NSNumber::new_bool(!see_through);
                let _: () =
                    msg_send![view, setValue: &*draws, forKey: ns_string!("drawsBackground")];
            }
        })
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn clear(_window: &tauri::WebviewWindow, _see_through: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn reduce_transparency() -> bool {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    workspace.accessibilityDisplayShouldReduceTransparency()
        || workspace.accessibilityDisplayShouldIncreaseContrast()
}

#[cfg(not(target_os = "macos"))]
fn reduce_transparency() -> bool {
    false
}
