//! Print what the library, the course page, the lesson's steps and the
//! progress page would show for a study folder, as JSON. It starts no agent
//! and changes nothing.
//!
//!   cargo run -p mg-core --example look -- ~/path/to/study-folder

use std::path::PathBuf;

use mg_core::{study, Store};

fn main() {
    let folder = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("a current folder"));
    // A store nobody else reads, so looking leaves the window's own state alone.
    let store = Store::at(std::env::temp_dir().join("magnesium-window-look"));
    let found = |name: &str| mg_core::paths::find_program(name).map(|p| p.display().to_string());
    let report = serde_json::json!({
        "programs": {
            "claude": found("claude"),
            "magnesium": mg_core::runtime::program().map(|p| p.display().to_string()),
            "path_for_the_agent": mg_core::paths::child_path().to_string_lossy(),
        },
        "card": study::card(&folder, &store),
        "home": study::home(&folder, &store),
        "rail": study::rail(&folder),
        "progress": study::progress(&folder, None),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("printable")
    );
}
