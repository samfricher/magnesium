//! A folder an earlier Magnesium set up, opened with the runtime built from
//! this checkout. The window asks once before it upgrades the folder, and
//! until then neither it nor the agent it would start changes the record.
//!
//! It never touches the learner's own home or study folders: the runtime runs
//! with a scratch home and a scratch study folder.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mg_core::claude::ClaudeCode;
use mg_core::study::HomeState;
use mg_core::{runtime, Manager, Store};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

/// Build the runtime from this checkout. `MAGNESIUM_TEST_BINARY` names one
/// already built instead.
fn runtime_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("MAGNESIUM_TEST_BINARY").filter(|v| !v.is_empty()) {
        return PathBuf::from(binary);
    }
    let root = repo_root();
    let status = Command::new("cargo")
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .args(["build", "--release", "--locked", "--bin", "magnesium"])
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the runtime did not build");
    root.join("target/release/magnesium")
}

fn record_copies(folder: &Path) -> usize {
    fs::read_dir(folder.join(".magnesium/backups"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("state-schema-"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn a_folder_an_earlier_version_set_up_is_upgraded_only_after_a_yes() {
    let binary = runtime_binary();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for name in ["home", "data", "config", "tmp", "window", "Course"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let folder = root.join("Course");
    let at = |name: &str| root.join(name).display().to_string();
    let wrapper = root.join("magnesium");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME='{}' XDG_DATA_HOME='{}' XDG_CONFIG_HOME='{}' TMPDIR='{}' '{}' \"$@\"\n",
            at("home"),
            at("data"),
            at("config"),
            at("tmp"),
            binary.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(runtime::PROGRAM_ENV, &wrapper);

    // Set up as today, then given the record the release of 21 October made.
    runtime::init(&folder).unwrap();
    let store_path = folder.join(".magnesium/state.sqlite");
    let id: String = rusqlite::Connection::open(&store_path)
        .unwrap()
        .query_row("SELECT workspace_id FROM workspace_meta", [], |r| r.get(0))
        .unwrap();
    fs::remove_file(&store_path).unwrap();
    let old = rusqlite::Connection::open(&store_path).unwrap();
    old.execute_batch(
        &fs::read_to_string(repo_root().join("tests/fixtures/learner_model/schema-2.sql")).unwrap(),
    )
    .unwrap();
    old.execute(
        "INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms)
           VALUES(1,?1,'learner-model/v2',2,1000)",
        [&id],
    )
    .unwrap();
    drop(old);
    let before = fs::read(&store_path).unwrap();

    let manager = Manager::new(
        Arc::new(Store::at(root.join("window"))),
        Arc::new(ClaudeCode::with_program(root.join("no-agent-needed"))),
        Arc::new(|_| {}),
    );
    // Opening it, and the start screen of every folder opened before, asks.
    let home = manager.open_folder(&folder).unwrap();
    assert_eq!(home.state, HomeState::NeedsUpgrade, "{home:?}");
    assert!(home.message.unwrap().contains("keeps a copy"));
    assert!(manager.progress(None).unwrap().message.is_some());
    let refused = manager.open_session(false).unwrap_err().to_string();
    assert!(refused.contains("Upgrade it"), "{refused}");
    let _ = manager.folder_to_open();
    assert_eq!(
        fs::read(&store_path).unwrap(),
        before,
        "the record is as it was"
    );
    assert_eq!(record_copies(&folder), 0);

    // After the yes: upgraded, with a copy of the record as it was.
    let home = manager.upgrade_folder().unwrap();
    assert_eq!(home.state, HomeState::Empty, "{home:?}");
    assert_eq!(record_copies(&folder), 1);
}
