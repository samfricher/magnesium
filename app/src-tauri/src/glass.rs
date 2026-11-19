use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    Frosted,
    Liquid,
}

#[derive(Serialize)]
pub struct GlassState {
    can: bool,
    liquid: bool,
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
    window
        .set_theme(dark.map(|dark| if dark { Theme::Dark } else { Theme::Light }))
        .map_err(|e| e.to_string())?;
    clear(&window, true)?;
    let glass = match material {
        Material::Frosted => Effect::LiquidGlassRegular,
        Material::Liquid => Effect::LiquidGlassClear,
    };
    let effects = EffectsBuilder::new()
        .effect(glass)
        .effect(Effect::UnderWindowBackground);
    window
        .set_effects(effects.build())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn clear(window: &tauri::WebviewWindow, see_through: bool) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSColor, NSWindow};
    use objc2_foundation::{ns_string, NSNumber};

    window
        .with_webview(move |webview| unsafe {
            let ns_window: &NSWindow = &*(webview.ns_window() as *const NSWindow);
            ns_window.setOpaque(!see_through);
            let ground = if see_through {
                NSColor::clearColor()
            } else {
                NSColor::windowBackgroundColor()
            };
            ns_window.setBackgroundColor(Some(&ground));
            let view: &AnyObject = &*(webview.inner() as *const AnyObject);
            let draws = NSNumber::new_bool(!see_through);
            let _: () = msg_send![view, setValue: &*draws, forKey: ns_string!("drawsBackground")];
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
