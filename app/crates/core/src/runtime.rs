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

/// `magnesium progress --json`: each skill's passes, the level, what is due
/// and the days with a marked answer, worked out by the runtime from the
/// learner record. None when this version of the runtime has no such
/// command, when the folder is not set up, or when an earlier version set it
/// up: like the digest, the command never upgrades a folder.
pub fn progress(folder: &Path) -> Option<Value> {
    let out = run(folder, &["progress", "--json"], Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    (value["active"].as_bool() == Some(true)
        && value["upgrade_needed"].as_bool() != Some(true)
        && value["days"].is_array())
    .then_some(value)
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

/// The lowest `window_features` this window works with. A runtime that says
/// less, or nothing, is refused: its version number cannot tell, because a
/// build from an older commit can carry the same one. 1 is the first runtime
/// whose `progress --json` counts what the learner did today.
pub const WINDOW_FEATURES_NEEDED: u64 = 1;

/// The `window_features` number that `magnesium digest` prints: which of the
/// window's needs this program meets. It is asked at the root of the disk, in
/// no study folder, so no record is read and nothing changes. None when the
/// program says no number, as every runtime from before it does.
pub fn window_features(program: &Path) -> Option<u64> {
    let mut command = paths::light_command(program);
    command.current_dir("/").arg("digest");
    let out = paths::run(command, Duration::from_secs(10)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    value["window_features"].as_u64()
}

/// Whether a program is new enough for this window.
pub fn fits_window(program: &Path) -> bool {
    window_features(program).is_some_and(|number| number >= WINDOW_FEATURES_NEEDED)
}

pub fn version() -> Option<String> {
    let program = program()?;
    let mut command = paths::light_command(&program);
    command.arg("--version");
    let out = paths::run(command, Duration::from_secs(5)).ok()?;
    out.split_whitespace().last().map(str::to_owned)
}

/// `magnesium searchable --json`: for each named file, whether the tutor's
/// source search will read it, and why not when it will not. The paths are
/// from the folder's root. None when this version of the runtime has no such
/// command. The command works in a folder that is not set up yet, and
/// changes nothing.
pub fn searchable(folder: &Path, paths: &[String]) -> Option<Vec<Value>> {
    let mut args = vec!["searchable", "--json", "--"];
    args.extend(paths.iter().map(String::as_str));
    // Each PDF is read to see whether it has any text.
    let out = run(folder, &args, Duration::from_secs(90)).ok()?;
    let value: Value = serde_json::from_str(&out).ok()?;
    let files = value["files"].as_array().cloned()?;
    (files.len() == paths.len()).then_some(files)
}
