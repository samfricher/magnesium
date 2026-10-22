//! Independent adversarial review of native byte provenance, not learner authority.
use super::native_capture;
use crate::{
    learner::{HostContext, LearnerStore, SCHEMA},
    workspace::Workspace,
};
use serde_json::json;
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    host: HostContext,
    store: LearnerStore,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .join(crate::learner::store::new_id("native-proof-review").unwrap());
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"native-review-workspace\"\n",
        )
        .unwrap();
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let store =
            LearnerStore::initialize(&workspace.store_path(), "native-review-workspace").unwrap();
        native_capture::initialize(&workspace).unwrap();
        Self {
            root,
            workspace,
            store,
            host: HostContext {
                host_kind: "codex".into(),
                host_thread_id: "native-thread".into(),
            },
        }
    }
    fn directory(&self) -> PathBuf {
        self.root
            .join(crate::workspace::CONTROL_DIR)
            .join("cache/native-capture")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn later_native_turn_revokes_old_ticket_and_identical_text_is_not_same_event() {
    let f = Fixture::new();
    let old = native_capture::issue(&f.workspace, &f.host, "turn-1", "same text", 1000).unwrap();
    let retry = native_capture::issue(&f.workspace, &f.host, "turn-1", "same text", 1001).unwrap();
    assert_eq!(old, retry);
    let next = native_capture::issue(&f.workspace, &f.host, "turn-2", "same text", 1002).unwrap();
    assert!(native_capture::verify(&f.workspace, &f.host, &old, "same text", 1003).is_err());
    let event = native_capture::verify(&f.workspace, &f.host, &next, "same text", 1003).unwrap();
    assert_ne!(
        event.external_event_key,
        native_capture::event_key("turn-1")
    );
    native_capture::invalidate(&f.workspace, &f.host).unwrap();
    assert!(native_capture::verify(&f.workspace, &f.host, &next, "same text", 1004).is_err());
}
#[test]
fn copied_signer_cannot_move_capture_to_another_workspace_root() {
    let first = Fixture::new();
    let second = Fixture::new();
    let ticket =
        native_capture::issue(&first.workspace, &first.host, "turn", "private text", 1000).unwrap();
    for entry in fs::read_dir(first.directory()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "metadata.lock" {
            fs::copy(entry.path(), second.directory().join(entry.file_name())).unwrap();
        }
    }
    assert!(
        native_capture::verify(
            &second.workspace,
            &second.host,
            &ticket,
            "private text",
            1001
        )
        .is_err()
    );
}
#[test]
fn pending_proof_metadata_stores_neither_learner_text_nor_its_digest() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new();
    let text = "PRIVATE learner message with enough unique bytes to inspect persistence";
    native_capture::issue(&f.workspace, &f.host, "turn", text, 1000).unwrap();
    let text_digest = Sha256::digest(text.as_bytes());
    for entry in fs::read_dir(f.directory()).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(text.len())
                .any(|part| part == text.as_bytes())
        );
        assert!(!bytes.windows(32).any(|part| part == text_digest.as_slice()));
    }
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn successful_start_retry_survives_expiry_and_signer_rotation_without_recapture() {
    let f = Fixture::new();
    let text = "Teach me coefficient roles";
    let ticket = native_capture::issue(&f.workspace, &f.host, "turn", text, 1000).unwrap();
    let raw = native_capture::verify(&f.workspace, &f.host, &ticket, text, 1001).unwrap();
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"native-start","session":{"operation":"start","client_ref":"lesson","goal":"coefficient roles"},"native_capture":{"ticket":ticket,"text":text}});
    let first = f
        .store
        .update_with_native_capture(&f.host, request.clone(), Some(&raw))
        .unwrap();
    fs::remove_file(f.directory().join("signing-key")).unwrap();
    native_capture::initialize(&f.workspace).unwrap();
    assert!(
        native_capture::verify(
            &f.workspace,
            &f.host,
            &ticket,
            text,
            1001 + native_capture::TTL_MS
        )
        .is_err()
    );
    let retry = f.store.replay(&f.host, &request).unwrap().unwrap();
    assert_eq!(first.captured_event_ids, retry.captured_event_ids);
    assert_eq!(first.references, retry.references);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[cfg(unix)]
#[test]
fn init_is_idempotent_private_and_rejects_symlink_signer() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = Fixture::new();
    let key = f.directory().join("signing-key");
    let original = fs::read(&key).unwrap();
    native_capture::initialize(&f.workspace).unwrap();
    assert_eq!(fs::read(&key).unwrap(), original);
    assert_eq!(
        fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(native_capture::initialize(&f.workspace).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&key).unwrap();
    let outside = f.root.join("outside-key");
    fs::write(&outside, &original).unwrap();
    symlink(&outside, &key).unwrap();
    assert!(native_capture::initialize(&f.workspace).is_err());
    assert!(native_capture::issue(&f.workspace, &f.host, "turn", "text", 1000).is_err());
}
#[test]
fn failed_recording_off_requires_recording_on_after_storage_recovers() {
    let f = Fixture::new();
    f.store
        .conn
        .execute_batch("DROP TABLE host_bindings")
        .unwrap();
    let control = |key: &str, action: &str| json!({"schema":SCHEMA,"action":"control","request_id":key,"control":action,"reported_event":{"external_event_key":key,"speaker":"learner","kind":"message","text":action}});
    let off = super::dispatch(
        &f.root,
        "learning_step",
        control("failed-off", "recording_off"),
    )
    .unwrap();
    assert_eq!(off["local_stop"], true);
    f.store.conn.execute_batch("CREATE TABLE host_bindings(host_kind TEXT NOT NULL,host_thread_id TEXT NOT NULL,session_id TEXT REFERENCES learning_sessions(session_id),learning_active INTEGER NOT NULL DEFAULT 0 CHECK(learning_active IN(0,1)),recording_enabled INTEGER NOT NULL DEFAULT 1 CHECK(recording_enabled IN(0,1)),PRIMARY KEY(host_kind,host_thread_id));").unwrap();
    let start = json!({"schema":SCHEMA,"action":"update","request_id":"after-recovery-start","session":{"operation":"start","client_ref":"session","goal":"Resume learning while recording stays disabled"}});
    assert!(
        super::dispatch(&f.root, "learning_step", start.clone()).is_err(),
        "Starting learning must not clear a failed recording-off control"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    super::dispatch(
        &f.root,
        "learning_step",
        control("explicit-on", "recording_on"),
    )
    .unwrap();
    assert_eq!(
        super::dispatch(&f.root, "learning_step", start).unwrap()["update_status"],
        "committed"
    );
}
#[test]
fn same_process_hooks_honor_failed_control_suppression() {
    let f = Fixture::new();
    let native_host = super::HostIdentity::codex("failed-control-hook-review").unwrap();
    let host = super::host_context(&native_host);
    f.store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":"hook-start","session":{"operation":"start","client_ref":"session","goal":"algebra"}})).unwrap();
    f.store.conn.execute_batch("CREATE TRIGGER reject_recording_control BEFORE UPDATE OF recording_enabled ON host_bindings BEGIN SELECT RAISE(ABORT,'injected durable control failure'); END;").unwrap();
    let event = crate::learner::RawEvent {
        external_event_key: "off-turn".into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: "Stop recording".into(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    };
    let error = f
        .store
        .control(&host, "hook-off", "recording_off", &event)
        .unwrap_err();
    let key = format!(
        "{}:{}:{}",
        super::sha256_hex(&crate::workspace::native_path_key(f.workspace.root())),
        host.host_kind,
        host.host_thread_id
    );
    let request = json!({"schema":SCHEMA,"action":"control","request_id":"hook-off","control":"recording_off","reported_event":event});
    let failed =
        super::control_storage_failure("learning_step", &request, &key, error.to_string()).unwrap();
    assert_eq!(failed["local_stop"], true);
    f.store
        .conn
        .execute_batch("DROP TRIGGER reject_recording_control")
        .unwrap();
    assert!(f.store.binding(&host).unwrap().2);
    let notice = super::host_user_prompt_submit(
        &f.root,
        &native_host,
        Some("next-turn"),
        None,
        "Ordinary coding after failed stop",
    )
    .unwrap()
    .unwrap();
    super::host_response_stop_with_delivery(
        &f.root,
        &native_host,
        "tutor-turn",
        Some("A tutor message after failed stop"),
    )
    .unwrap();
    let events: i64 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM learning_events", [], |row| row.get(0))
        .unwrap();
    super::recording_suppressed().lock().unwrap().remove(&key);
    assert_eq!(
        events, 0,
        "Native learner and tutor hooks must honor the same process-local privacy latch"
    );
    assert!(notice.contains("locally suppressed"));
}
#[test]
fn stop_then_new_native_hook_can_start_a_new_session_in_same_thread() {
    let f = Fixture::new();
    let native_host = super::HostIdentity::codex("restart-same-thread").unwrap();
    let host = super::host_context(&native_host);
    let initial=f.store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":"initial-session","session":{"operation":"start","client_ref":"first","goal":"First lesson"}})).unwrap();
    let stop = crate::learner::RawEvent {
        external_event_key: "stop".into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: "Stop learning".into(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    };
    f.store.control(&host, "stop-first", "stop", &stop).unwrap();
    assert_eq!(
        f.store.binding(&host).unwrap().0,
        Some(initial.references["first"].clone())
    );
    assert!(!f.store.binding(&host).unwrap().1);
    let text = "Start a new lesson on coefficient roles";
    let notice =
        super::host_user_prompt_submit(&f.root, &native_host, Some("new-native-turn"), None, text)
            .unwrap()
            .unwrap();
    let ticket = notice
        .split_once("native_capture.ticket=")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    let raw = native_capture::verify(
        &f.workspace,
        &host,
        ticket,
        text,
        crate::learner::store::now_ms(),
    )
    .unwrap();
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"new-native-session","session":{"operation":"start","client_ref":"second","goal":"New lesson"},"native_capture":{"ticket":ticket,"text":text}});
    let next = f
        .store
        .update_with_native_capture(&host, request, Some(&raw))
        .unwrap();
    let next_id = &next.references["second"];
    assert_ne!(next_id, &initial.references["first"]);
    assert_eq!(f.store.binding(&host).unwrap().0.as_ref(), Some(next_id));
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT session_id FROM learning_events WHERE event_id=?1",
                [&next.captured_event_ids[0]],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        *next_id
    );
}

#[test]
fn old_control_receipts_cannot_clear_later_failed_control_denies() {
    for old_action in ["recording_off", "recording_on", "pause", "stop"] {
        let f = Fixture::new();
        let host = super::tool_host();
        let control = |key: &str, action: &str| json!({"schema":SCHEMA,"action":"control","request_id":key,"control":action,"reported_event":{"external_event_key":key,"speaker":"learner","kind":"message","text":action}});
        let start = |key: &str| json!({"schema":SCHEMA,"action":"update","request_id":key,"session":{"operation":"start","client_ref":"lesson","goal":"Control replay regression"}});
        super::dispatch(&f.root, "learning_step", start("initial")).unwrap();
        let old = control("old-control", old_action);
        assert_eq!(
            super::dispatch(&f.root, "learning_step", old.clone()).unwrap()["durable"],
            true
        );
        super::dispatch(&f.root, "learning_step", control("new-on", "recording_on")).unwrap();
        let active = super::dispatch(&f.root, "learning_step", start("new-start")).unwrap();
        let privacy = old_action.starts_with("recording_");
        let (column, failed_action) = if privacy {
            ("recording_enabled", "recording_off")
        } else {
            ("learning_active", "stop")
        };
        f.store.conn.execute_batch(&format!("CREATE TRIGGER deny_control BEFORE UPDATE OF {column} ON host_bindings BEGIN SELECT RAISE(ABORT,'control failure'); END;")).unwrap();
        let failed = super::dispatch(
            &f.root,
            "learning_step",
            control("failed-later", failed_action),
        )
        .unwrap();
        assert_eq!(failed["cross_process_protection"], "persisted_deny");
        f.store
            .conn
            .execute_batch("DROP TRIGGER deny_control")
            .unwrap();
        let replay = super::dispatch(&f.root, "learning_step", old).unwrap();
        assert_eq!(replay["replayed"], true);
        assert_eq!(replay["capture_admission"], "unchanged");
        {
            let guard = super::control_safety::Guard::acquire(&f.workspace, &host).unwrap();
            assert_eq!(guard.recording_blocked(), privacy, "{old_action}");
            assert_eq!(guard.learning_blocked(), !privacy, "{old_action}");
        }
        let (_, learning, recording) = f.store.binding(&host).unwrap();
        assert!(
            learning && recording,
            "failed write leaves canonical state on"
        );
        let update = json!({"schema":SCHEMA,"action":"update","request_id":"must-stay-blocked","session":{"operation":"use","session_id":active["references"]["lesson"],"expected_revision":1}});
        assert!(
            super::dispatch(&f.root, "learning_step", update).is_err(),
            "{old_action}"
        );
        if privacy {
            super::dispatch(
                &f.root,
                "learning_step",
                control("explicit-recovery", "recording_on"),
            )
            .unwrap();
        } else {
            super::dispatch(&f.root, "learning_step", start("explicit-recovery")).unwrap();
        }
        assert!(
            !super::control_safety::Guard::acquire(&f.workspace, &host)
                .unwrap()
                .capture_blocked()
        );
    }
}

#[test]
fn replay_does_not_add_deny_or_clear_process_local_deny() {
    let f = Fixture::new();
    let host = super::tool_host();
    let control = |key: &str, action: &str| json!({"schema":SCHEMA,"action":"control","request_id":key,"control":action,"reported_event":{"external_event_key":key,"speaker":"learner","kind":"message","text":action}});
    let old = control("old-off", "recording_off");
    super::dispatch(&f.root, "learning_step", old.clone()).unwrap();
    super::dispatch(&f.root, "learning_step", control("new-on", "recording_on")).unwrap();
    super::dispatch(&f.root, "learning_step", old.clone()).unwrap();
    assert!(
        !super::control_safety::Guard::acquire(&f.workspace, &host)
            .unwrap()
            .capture_blocked()
    );
    let key = format!(
        "{}:{}:{}",
        super::sha256_hex(&crate::workspace::native_path_key(f.workspace.root())),
        host.host_kind,
        host.host_thread_id
    );
    super::recording_suppressed()
        .lock()
        .unwrap()
        .insert(key.clone());
    super::dispatch(&f.root, "learning_step", old).unwrap();
    assert!(super::recording_suppressed().lock().unwrap().remove(&key));
}

#[test]
fn old_session_start_receipt_does_not_recover_a_later_failed_stop() {
    let f = Fixture::new();
    let host = super::tool_host();
    let start = json!({"schema":SCHEMA,"action":"update","request_id":"original-start","session":{"operation":"start","client_ref":"lesson","goal":"Receipt replay"}});
    let original = super::dispatch(&f.root, "learning_step", start.clone()).unwrap();
    f.store.conn.execute_batch("CREATE TRIGGER fail_stop BEFORE UPDATE OF learning_active ON host_bindings BEGIN SELECT RAISE(ABORT,'control failure'); END;").unwrap();
    let failed = super::dispatch(&f.root, "learning_step", json!({"schema":SCHEMA,"action":"control","request_id":"later-stop","control":"stop","reported_event":{"external_event_key":"later-stop","speaker":"learner","kind":"message","text":"Stop learning"}})).unwrap();
    assert_eq!(failed["cross_process_protection"], "persisted_deny");
    f.store
        .conn
        .execute_batch("DROP TRIGGER fail_stop")
        .unwrap();
    assert_eq!(
        super::dispatch(&f.root, "learning_step", start).unwrap(),
        original
    );
    assert!(
        super::control_safety::Guard::acquire(&f.workspace, &host)
            .unwrap()
            .learning_blocked()
    );
}
