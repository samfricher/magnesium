use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use support::{TestEnvironment, v2};
fn info(e: &TestEnvironment) -> Value {
    let o = e.command().args(["info", "--json"]).output().unwrap();
    assert!(o.status.success(), "{o:?}");
    serde_json::from_slice(&o.stdout).unwrap()
}
#[test]
fn inactive_info_does_not_initialize() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let report = info(&e);
    assert_eq!(report["active"], false);
    assert_eq!(report["reason"], "workspace_not_initialized");
    assert!(!e.workspace.join(support::CONTROL_DIR).exists());
}
#[test]
fn info_reports_fresh_family_counts_and_instruction_identity_without_writes() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let path = e.workspace.join(support::CONTROL_DIR).join("state.sqlite");
    let before = fs::read(&path).unwrap();
    let report = info(&e);
    assert_eq!(report["schema"], "learner-model/v2");
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["runtime_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["current_memory_count"], 0);
    assert_eq!(report["pending_event_count"], 0);
    assert_eq!(report["index_current"], true);
    assert!(report.get("group_id").is_none());
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn pending_native_work_is_visible_and_observation_index_failure_is_distinct() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "codex", "monday");
    let o = v2::prompt(&e, "codex", "monday", Some("1"), "What does b mean?");
    assert!(o.status.success());
    let report = info(&e);
    assert_eq!(report["pending_event_count"], 1);
    let c = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    c.execute(
        "UPDATE workspace_meta SET observation_fts_generation=-1",
        [],
    )
    .unwrap();
    let report = info(&e);
    assert_eq!(report["index_current"], false);
    assert_eq!(report["memory_index_current"], true);
    assert_eq!(report["observation_index_current"], false);
}
#[test]
fn incompatible_store_is_rejected_without_migration_or_wipe() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let path = e.workspace.join(support::CONTROL_DIR).join("state.sqlite");
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE workspace_meta;")
        .unwrap();
    let before = fs::read(&path).unwrap();
    let o = e.command().args(["info", "--json"]).output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("incompatible_store"));
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn info_observes_live_wal_commits() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let c = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE workspace_meta SET memory_generation=9,memory_fts_generation=-1;").unwrap();
    let report = info(&e);
    assert_eq!(report["generations"]["memory"], 9);
    assert_eq!(report["index_current"], false);
}
#[test]
fn source_and_failure_text_cannot_impersonate_diagnostics() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let p = e
        .workspace
        .join(support::CONTROL_DIR)
        .join("diagnostics/latest-hook-error.json");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(
        p,
        r#"{"event":"hook","failure":"secret transcript body","at_ms":1}"#,
    )
    .unwrap();
    let report = info(&e);
    assert_eq!(report["last_hook_error"], Value::Null);
    assert!(!report.to_string().contains("secret transcript"));
}
