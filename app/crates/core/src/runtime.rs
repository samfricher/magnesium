//! The Magnesium runtime, reached through its own commands.
//!
//! The window links none of the runtime's code. It runs the `magnesium`
//! program the learner installed and reads what it prints, so the runtime
//! stays small and the two can be updated apart.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::paths;

/// Set this to use another program in place of `magnesium`. For tests.
pub const PROGRAM_ENV: &str = "MAGNESIUM_APP_RUNTIME";

pub fn program() -> Option<PathBuf> {
    if let Some(named) = env::var_os(PROGRAM_ENV).filter(|v| !v.is_empty()) {
        let named = PathBuf::from(named);
        return named.is_file().then_some(named);
    }
    paths::find_program("magnesium")
}

fn run(folder: &Path, args: &[&str], limit: Duration) -> Result<String, String> {
    let program = program().ok_or("the magnesium program is not installed")?;
    let mut command = paths::light_command(&program);
    command.current_dir(folder).args(args);
    paths::run(command, limit)
}

/// `magnesium digest`: the study folder and the learner record, summarized
/// without changing either.
pub fn digest(folder: &Path) -> Result<Value, String> {
    let out = run(folder, &["digest"], Duration::from_secs(10))?;
    serde_json::from_str(&out).map_err(|e| format!("its digest was not readable ({e})"))
}

/// `magnesium attempts --json`: one row per marked answer. None when this
/// version of the runtime has no such command, or the folder has no record.
pub fn attempts(folder: &Path) -> Option<Vec<Value>> {
    let out = run(folder, &["attempts", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["attempts"].as_array().cloned()
}

/// `magnesium init`: set a folder up for study.
pub fn init(folder: &Path) -> Result<(), String> {
    run(folder, &["init", "--json"], Duration::from_secs(30)).map(|_| ())
}

/// `magnesium init --upgrade-instructions`, in the study root: upgrade a
/// folder an earlier version set up. The runtime copies the record to
/// `.magnesium/backups` before it changes it, and switches the folder to this
/// version's teaching instructions. A large record takes a while to copy.
pub fn upgrade(root: &Path) -> Result<(), String> {
    run(
        root,
        &["init", "--upgrade-instructions", "--json"],
        Duration::from_secs(180),
    )
    .map(|_| ())
}

pub fn version() -> Option<String> {
    let program = program()?;
    let mut command = paths::light_command(&program);
    command.arg("--version");
    let out = paths::run(command, Duration::from_secs(5)).ok()?;
    out.split_whitespace().last().map(str::to_owned)
}
