//! Glass behind the page, when the learner chooses it in Settings.
//!
//! The window opens opaque, exactly as it always has. Only when the page asks
//! for glass does it turn the window clear, stop the web view painting a
//! ground of its own, and put one of the Mac's own materials under the page;
//! the page then paints its ground part-way see-through (`data-glass` in the
//! styles), and how far is the page's alone. Turning it off puts every one of
//! those back.
//!
//! The glass itself is AppKit's, through Tauri's window effects (the
//! `window-vibrancy` crate): an `NSGlassEffectView`, on macOS 26 and later.
//! Frosted is its regular style, which blurs what is behind the window;
//! Liquid Glass is its clear style, which leaves it close to sharp. Before
//! macOS 26 there is one material for both, an `NSVisualEffectView` with the
//! under-window material. The window server draws any of them, not the page,
//! so the page's scrolling and typing do no more work than over an opaque
//! ground.

use serde::{Deserialize, Serialize};

/// Which of the Mac's materials goes under the page.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    Frosted,
    Liquid,
}

/// What this Mac can do, as the page needs to know it before it asks.
#[derive(Serialize)]
pub struct GlassState {
    /// The window can be made see-through here.
    can: bool,
    /// This Mac has the new glass (macOS 26 and later). Without it there is one older material, and the page
    /// offers the frosted half only.
    liquid: bool,
    /// The learner asked the Mac to reduce transparency or increase contrast
    /// (System Settings, Accessibility, Display). Glass is not drawn then, as
    /// AppKit's own materials turn opaque under Reduce transparency.
    reduce: bool,
}

#[tauri::command]
pub fn glass_state() -> GlassState {
    GlassState {
        can: cfg!(target_os = "macos"),
        liquid: has_liquid_glass(),
        reduce: reduce_transparency(),
    }
}

/// Put a material under the page, or with none take the glass away. `dark` is
/// the page's scheme when the learner chose one; with none the glass follows
/// the Mac.
#[tauri::command]
pub fn set_glass(
    window: tauri::WebviewWindow,
    material: Option<Material>,
    dark: Option<bool>,
) -> Result<(), String> {
    use tauri::window::{Effect, EffectsBuilder};
    use tauri::Theme;

    let Some(material) = material else {
        window.set_effects(None).map_err(|e| e.to_string())?;
        clear(&window, false)?;
        return window.set_theme(None).map_err(|e| e.to_string());
    };
    // The glass is tinted by the window's appearance: it has to match the page's scheme, not the Mac's.
    window
        .set_theme(dark.map(|dark| if dark { Theme::Dark } else { Theme::Light }))
        .map_err(|e| e.to_string())?;
    clear(&window, true)?;
    let glass = match material {
        Material::Frosted => Effect::LiquidGlassRegular,
        Material::Liquid => Effect::LiquidGlassClear,
    };
    // Tauri falls back to the second effect on macOS 15 and earlier.
    let effects = EffectsBuilder::new()
        .effect(glass)
        .effect(Effect::UnderWindowBackground);
    // Setting effects takes away the ones set before, so one material replaces the other.
    window
        .set_effects(effects.build())
        .map_err(|e| e.to_string())
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
fn has_liquid_glass() -> bool {
    objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some()
}

#[cfg(not(target_os = "macos"))]
fn has_liquid_glass() -> bool {
    false
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
