//! Independent safety-boundary regressions. No host model or UI is involved.
use super::control_safety::Guard;
use crate::{
    learner::{HostContext, LearnerStore, SCHEMA},
    workspace::Workspace,
};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    store: LearnerStore,
    host: HostContext,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .join(crate::learner::store::new_id("control-boundary-review").unwrap());
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"control-review\"\n",
        )
        .unwrap();
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let store = LearnerStore::initialize(&workspace.store_path(), "control-review").unwrap();
        let host = HostContext {
            host_kind: "codex".into(),
            host_thread_id: "control-review-thread".into(),
        };
        Self {
            root,
            workspace,
            store,
            host,
        }
    }
    fn marker(&self) -> PathBuf {
        self.workspace
            .control()
            .join("control-safety")
            .join(format!(
                "{}.deny",
                super::sha256_hex(
                    &serde_json::to_vec(&[&self.host.host_kind, &self.host.host_thread_id])
                        .unwrap()
                )
            ))
    }
    fn start(&self) {
        self.store.update(&self.host, json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Control safety"}})).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn recording_and_learning_denies_recover_independently() {
    let f = Fixture::new();
    {
        let mut guard = Guard::acquire(&f.workspace, &f.host).unwrap();
        guard.before_control("recording_off").unwrap();
        guard.before_control("stop").unwrap();
        guard.after_control("recording_on").unwrap();
        assert!(!guard.recording_blocked());
        assert!(guard.learning_blocked());
    }
    {
        let mut guard = Guard::acquire(&f.workspace, &f.host).unwrap();
        assert!(!guard.recording_blocked());
        assert!(guard.learning_blocked());
        guard.before_control("recording_off").unwrap();
        guard.after_resume().unwrap();
        assert!(guard.recording_blocked());
        assert!(!guard.learning_blocked());
    }
    let guard = Guard::acquire(&f.workspace, &f.host).unwrap();
    assert!(guard.recording_blocked());
    assert!(!guard.learning_blocked());
}

#[test]
fn malformed_markers_deny_semantic_and_trace_writes() {
    let f = Fixture::new();
    f.start();
    drop(Guard::acquire(&f.workspace, &f.host).unwrap());
    for bytes in [vec![], vec![1], vec![2, 1], vec![1, 4], vec![1, 1, 0]] {
        crate::workspace::write_private_atomic(&f.marker(), &bytes).unwrap();
        assert!(Guard::acquire(&f.workspace, &f.host).is_err());
        let update = json!({"schema":SCHEMA,"action":"update","request_id":"denied-update","reported_events":[{"external_event_key":"private","speaker":"learner","kind":"message","text":"Do not retain this"}]});
        assert!(
            super::dispatch_with_host(&f.root, "learning_step", update, Some(f.host.clone()))
                .is_err()
        );
        let status = super::dispatch_with_host(
            &f.root,
            "context",
            json!({"schema":SCHEMA,"action":"status"}),
            Some(f.host.clone()),
        )
        .unwrap();
        assert_eq!(status["trace_status"], "disabled");
    }
    for table in ["learning_events", "context_receipts"] {
        let count: i64 = f
            .store
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[cfg(unix)]
#[test]
fn nonprivate_and_symlink_markers_fail_closed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = Fixture::new();
    {
        let mut guard = Guard::acquire(&f.workspace, &f.host).unwrap();
        guard.before_control("recording_off").unwrap();
    }
    fs::set_permissions(f.marker(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Guard::acquire(&f.workspace, &f.host).is_err());
    fs::remove_file(f.marker()).unwrap();
    let outside = f.root.join("outside-marker");
    fs::write(&outside, [1, 0]).unwrap();
    symlink(&outside, f.marker()).unwrap();
    assert!(Guard::acquire(&f.workspace, &f.host).is_err());
    assert_eq!(fs::read(outside).unwrap(), [1, 0]);
}

#[test]
fn busy_admission_cannot_commit_control_outside_the_guard() {
    let f = Fixture::new();
    f.start();
    let guard = Guard::acquire(&f.workspace, &f.host).unwrap();
    let response = super::dispatch_with_host(&f.root, "learning_step", json!({"schema":SCHEMA,"action":"control","request_id":"busy-stop","control":"stop","reported_event":{"external_event_key":"busy-stop-turn","speaker":"learner","kind":"message","text":"Stop learning"}}), Some(f.host.clone())).unwrap();
    assert_eq!(response["local_stop"], true);
    assert_eq!(response["durable"], false);
    assert_eq!(response["cross_process_protection"], "unavailable");
    assert!(
        f.store.binding(&f.host).unwrap().1,
        "No unguarded canonical mutation may overtake an admitted operation"
    );
    let receipts: i64 = f
        .store
        .conn
        .query_row(
            "SELECT count(*) FROM operation_receipts WHERE request_id='busy-stop'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 0);
    drop(guard);
    let next = super::dispatch_with_host(
        &f.root,
        "learning_step",
        json!({"schema":SCHEMA,"action":"update","request_id":"later-update"}),
        Some(f.host.clone()),
    );
    assert!(
        next.is_err(),
        "The failed control still denies subsequent calls in the current process"
    );
}

#[test]
fn overlapping_recovery_cannot_clear_a_newer_local_failure() {
    use std::sync::{Mutex, mpsc};
    for preexisting in [false, true] {
        let state = Mutex::new(super::LocalSuppression::default());
        if preexisting {
            state.lock().unwrap().insert("host".into());
        }
        let (admitted_tx, admitted_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let local_state = &state;
            let recovery = scope.spawn(move || {
                let observed = local_state.lock().unwrap().token("host");
                admitted_tx.send(()).unwrap();
                finish_rx.recv().unwrap();
                local_state
                    .lock()
                    .unwrap()
                    .clear_if("host", observed.as_ref());
            });
            admitted_rx.recv().unwrap();
            let newer = state.lock().unwrap().insert("host".into());
            finish_tx.send(()).unwrap();
            recovery.join().unwrap();
            let mut local = state.lock().unwrap();
            assert!(local.contains("host"));
            assert!(std::sync::Arc::ptr_eq(
                &local.token("host").unwrap(),
                &newer
            ));
            local.clear_if("host", Some(&newer));
            assert!(!local.contains("host"));
        });
    }
}

#[test]
fn host_scoped_gate_does_not_serialize_an_unrelated_control() {
    let f = Fixture::new();
    f.start();
    let other = HostContext {
        host_thread_id: "other-conversation".into(),
        ..f.host.clone()
    };
    f.store.update(&other, json!({"schema":SCHEMA,"action":"update","request_id":"other-start","session":{"operation":"start","client_ref":"other","goal":"Other conversation"}})).unwrap();
    let _guard = Guard::acquire(&f.workspace, &f.host).unwrap();
    let off = super::dispatch_with_host(&f.root, "learning_step", json!({"schema":SCHEMA,"action":"control","request_id":"other-off","control":"recording_off","reported_event":{"external_event_key":"other-off-turn","speaker":"learner","kind":"message","text":"Stop recording"}}), Some(other.clone())).unwrap();
    assert_eq!(off["durable"], true, "{off}");
    assert!(!f.store.binding(&other).unwrap().2);
    assert!(f.store.binding(&f.host).unwrap().2);
}

#[test]
fn paused_snapshot_in_conversation_a_does_not_delay_conversation_b_controls() {
    use std::sync::mpsc;
    use std::time::Duration;
    let f = Fixture::new();
    f.start();
    // Isolate the admission boundary from SQLite's legitimate rollback-journal
    // reader/writer serialization. This reader owns a real WAL snapshot.
    f.store
        .conn
        .execute_batch("PRAGMA journal_mode=WAL")
        .unwrap();
    let other = HostContext {
        host_thread_id: "snapshot-other-conversation".into(),
        ..f.host.clone()
    };
    f.store.update(&other, json!({"schema":SCHEMA,"action":"update","request_id":"snapshot-other-start","session":{"operation":"start","client_ref":"other","goal":"Other conversation"}})).unwrap();
    let root = f.root.clone();
    let host = f.host.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let read = std::thread::spawn(move || {
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let store = super::open_store(&workspace).unwrap();
        let release_rx = std::sync::Mutex::new(release_rx);
        let cancellation = crate::request::OperationCancellation::none().with_test_hook(
            std::sync::Arc::new(move |phase| {
                if phase == "learner_snapshot_open" {
                    entered_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
            }),
        );
        let _scope = crate::request::enter_request_cancellation(&cancellation);
        // This is the same split used by dispatch: no gate exists during SQL.
        let prepared = crate::learner::read::prepare_snapshot(
            &store.conn,
            &host.host_kind,
            &host.host_thread_id,
            json!({"schema":SCHEMA,"action":"status"}),
        )
        .unwrap();
        super::control_admission::Admission::new(&workspace, &host)
            .trace(&store, |allowed| {
                crate::learner::read::finish_trace(&store.conn, prepared, allowed)
            })
            .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let stop = super::dispatch_with_host(
        &f.root,
        "learning_step",
        json!({"schema":SCHEMA,"action":"control","request_id":"snapshot-other-stop","control":"stop","reported_event":{"external_event_key":"snapshot-other-stop-turn","speaker":"learner","kind":"message","text":"Stop learning"}}),
        Some(other.clone()),
    );
    let off = super::dispatch_with_host(
        &f.root,
        "learning_step",
        json!({"schema":SCHEMA,"action":"control","request_id":"snapshot-other-off","control":"recording_off","reported_event":{"external_event_key":"snapshot-other-off-turn","speaker":"learner","kind":"message","text":"Stop recording"}}),
        Some(other.clone()),
    );
    release_tx.send(()).unwrap();
    read.join().unwrap();
    assert_eq!(stop.unwrap()["durable"], true);
    assert_eq!(off.unwrap()["durable"], true);
    assert_eq!(
        f.store.binding(&other).unwrap(),
        (f.store.binding(&other).unwrap().0, false, false)
    );
}

#[test]
fn recording_off_between_snapshot_and_trace_prevents_persistence() {
    let f = Fixture::new();
    f.start();
    let prepared = crate::learner::read::prepare_snapshot(
        &f.store.conn,
        &f.host.host_kind,
        &f.host.host_thread_id,
        json!({"schema":SCHEMA,"action":"status"}),
    )
    .unwrap();
    let off = super::dispatch_with_host(&f.root, "learning_step", json!({"schema":SCHEMA,"action":"control","request_id":"between-off","control":"recording_off","reported_event":{"external_event_key":"between-off-turn","speaker":"learner","kind":"message","text":"Stop recording"}}), Some(f.host.clone())).unwrap();
    assert_eq!(off["durable"], true);
    let packet = super::control_admission::Admission::new(&f.workspace, &f.host)
        .trace(&f.store, |allowed| {
            crate::learner::read::finish_trace(&f.store.conn, prepared, allowed)
        })
        .unwrap();
    assert_eq!(packet["trace_status"], "disabled");
    assert!(packet["trace_id"].is_null());
    assert!(
        packet["identity"].is_object(),
        "The useful prepared packet survives"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn failed_trace_returns_the_bounded_useful_snapshot() {
    let f = Fixture::new();
    f.start();
    let packet = super::dispatch_with_host(
        &f.root,
        "context",
        json!({"schema":SCHEMA,"action":"status"}),
        Some(f.host.clone()),
    )
    .unwrap();
    let max = serde_json::to_vec(&packet).unwrap().len();
    f.store.conn.execute_batch("CREATE TRIGGER reject_context_trace BEFORE INSERT ON context_receipts BEGIN SELECT RAISE(ABORT,'trace storage failed'); END;").unwrap();
    let result = super::dispatch_with_host(
        &f.root,
        "context",
        json!({"schema":SCHEMA,"action":"status","max_bytes":max}),
        Some(f.host.clone()),
    )
    .unwrap();
    assert_eq!(result["trace_status"], "unavailable");
    assert!(result["trace_id"].is_null());
    assert!(result["identity"].is_object());
    assert!(serde_json::to_vec(&result).unwrap().len() <= max);
}

#[test]
fn default_journal_read_snapshot_does_not_delay_the_other_host_deny_boundary() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    f.start();
    assert_ne!(
        f.store
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    let other = HostContext {
        host_thread_id: "default-journal-other".into(),
        ..f.host.clone()
    };
    f.store.update(&other,json!({"schema":SCHEMA,"action":"update","request_id":"default-other-start","session":{"operation":"start","client_ref":"other","goal":"Other conversation"}})).unwrap();
    let marker = f.workspace.control().join("control-safety").join(format!(
        "{}.deny",
        super::sha256_hex(&serde_json::to_vec(&[&other.host_kind, &other.host_thread_id]).unwrap())
    ));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let root = f.root.clone();
    let host = f.host.clone();
    let read = std::thread::spawn(move || {
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let store = super::open_store(&workspace).unwrap();
        let release_rx = std::sync::Mutex::new(release_rx);
        let cancellation = crate::request::OperationCancellation::none().with_test_hook(
            std::sync::Arc::new(move |phase| {
                if phase == "learner_snapshot_open" {
                    entered_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
            }),
        );
        let _scope = crate::request::enter_request_cancellation(&cancellation);
        let prepared = crate::learner::read::prepare_snapshot(
            &store.conn,
            &host.host_kind,
            &host.host_thread_id,
            json!({"schema":SCHEMA,"action":"status"}),
        )
        .unwrap();
        crate::learner::read::finish_trace(&store.conn, prepared, false).unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let root = f.root.clone();
    let (control_tx, control_rx) = mpsc::channel();
    let control = std::thread::spawn(move || {
        let result = super::dispatch_with_host(
            &root,
            "learning_step",
            json!({"schema":SCHEMA,"action":"control","request_id":"default-other-off","control":"recording_off","reported_event":{"external_event_key":"default-other-off-turn","speaker":"learner","kind":"message","text":"Stop recording"}}),
            Some(other),
        );
        control_tx.send(result).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    let deny_arrived = loop {
        if fs::read(&marker).is_ok_and(|bytes| bytes == [1, 1]) {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    // SQLite may serialize canonical COMMIT against this reader, but the deny
    // boundary has already been reached and protects capture fail-closed.
    let early_result = control_rx.try_recv();
    let canonical_waited = matches!(early_result, Err(mpsc::TryRecvError::Empty));
    release_tx.send(()).unwrap();
    read.join().unwrap();
    let result = match early_result {
        Ok(result) => result,
        Err(_) => control_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
    }
    .unwrap();
    control.join().unwrap();
    assert!(
        deny_arrived,
        "The unrelated control's durable deny must not wait for the read query"
    );
    assert!(
        canonical_waited,
        "This test preserves the product's rollback-journal contention"
    );
    assert_eq!(result["durable"], true);
}

#[test]
fn adapter_routes_pending_work_directory_from_both_read_tools_and_fresh_hooks() {
    let f = Fixture::new();
    f.start();
    f.store.conn.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('foreign-unbound','codex','earlier-thread','earlier-turn','learner','message','host_observed','Saved learner answer, still uninterpreted.','{}',123,'pending')",[]).unwrap();
    let fresh = HostContext {
        host_thread_id: "fresh-recovery-thread".into(),
        ..f.host.clone()
    };
    for tool in ["context", "memory"] {
        let packet = super::dispatch_with_host(
            &f.root,
            tool,
            json!({"schema":SCHEMA,"action":"pending_work","unbound_only":true}),
            Some(fresh.clone()),
        )
        .unwrap();
        assert_eq!(packet["pending_work"][0]["event_id"], "foreign-unbound");
        assert_eq!(
            packet["pending_work"][0]["host_thread_id"],
            "earlier-thread"
        );
        assert!(packet["pending_work"][0]["session_id"].is_null());
        assert_eq!(packet["pending_work"][0]["read_ref"]["kind"], "event");
    }
    let native = super::HostIdentity::codex("fresh-native-recovery").unwrap();
    let notice = super::host_session_start(&f.root, &native)
        .unwrap()
        .unwrap();
    assert!(notice.contains("context.pending_work"));
    assert!(!notice.contains("Saved learner answer"));
    let ticket_notice = super::host_user_prompt_submit(
        &f.root,
        &native,
        Some("current-native-turn"),
        None,
        "A new request before starting",
    )
    .unwrap()
    .unwrap();
    assert!(ticket_notice.contains("context.pending_work unbound_only:true"));
    assert!(ticket_notice.contains("unbound:1"));
    assert!(ticket_notice.len() <= 512);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn actual_control_decoder_accepts_closed_actions_and_reports_nested_field_paths() {
    let f = Fixture::new();
    for action in ["pause", "stop", "recording_off", "recording_on"] {
        let request = json!({"schema":SCHEMA,"action":"control","request_id":"typed-action","control":action,"reported_event":{"external_event_key":"current","speaker":"learner","kind":"message","text":"Control requested"}});
        let decoded: super::ControlRequest =
            crate::learner::update::decode_at(&request, "", &f.host).unwrap();
        assert_eq!(decoded.control.as_str(), action);
    }
    for (request, path) in [
        (
            json!({"schema":SCHEMA,"action":"control","request_id":"bad-action","control":"restart","reported_event":{"external_event_key":"current","speaker":"learner","kind":"message","text":"Control requested"}}),
            "control",
        ),
        (
            json!({"schema":SCHEMA,"action":"control","request_id":"bad-speaker","control":"stop","reported_event":{"external_event_key":"current","speaker":"teacher","kind":"message","text":"Control requested"}}),
            "reported_event.speaker",
        ),
    ] {
        let result =
            super::dispatch_with_host(&f.root, "learning_step", request, Some(f.host.clone()));
        assert!(result.unwrap_err().contains(path));
    }
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn native_ticket_notice_keeps_identity_compaction_recovery_and_full_ticket_in_512_bytes() {
    let ticket = "a".repeat(32);
    for count in [0, 1, 9999, 10000, i64::MAX] {
        let notice = super::native_ticket_notice(&ticket, count);
        assert!(notice.len() <= 512);
        assert!(notice.contains(&ticket));
        assert!(notice.contains("learner-model/v2"));
        assert!(notice.contains("compaction:context.prepare"));
        let operations = crate::learner::SessionEdit::CAPTURE_OPERATIONS.join("/");
        assert!(notice.contains(&format!("{operations}:exact text")));
        assert!(notice.contains(&format!("native_capture.ticket={ticket};")));
        if count > 0 {
            assert!(notice.contains("context.pending_work unbound_only:true"));
        }
        if count > 9999 {
            assert!(notice.contains("unbound:9999+"));
        }
    }
}

#[test]
fn invalid_control_input_is_field_specific_and_does_not_enter_storage_recovery() {
    for (variant, field, message) in [
        ("missing", "reported_event", "exactly one"),
        ("both", "turn_ref", "exactly one"),
        ("speaker", "reported_event.speaker", "learner"),
        ("stale", "turn_ref", "current native learner event"),
    ] {
        let f = Fixture::new();
        f.start();
        let before = f.store.binding(&f.host).unwrap();
        let receipt_count: i64 = f
            .store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |r| r.get(0))
            .unwrap();
        let event = |key: &str| crate::learner::RawEvent {
            external_event_key: key.into(),
            speaker: crate::learner::Speaker::Learner,
            kind: "message".into(),
            text: "Stop".into(),
            metadata: None,
            occurred_at_ms: None,
        };
        let captured = f
            .store
            .capture(
                &f.host,
                &[event("old"), event("current")],
                "host_observed",
                false,
            )
            .unwrap();
        let mut request = json!({"schema":SCHEMA,"action":"control","request_id":"bad-control","control":"recording_off"});
        match variant {
            "missing" => {}
            "both" => {
                request["turn_ref"] = json!(captured[1]);
                request["reported_event"] = json!(event("reported"));
            }
            "speaker" => {
                request["reported_event"] = json!(event("reported"));
                request["reported_event"]["speaker"] = json!("source");
            }
            "stale" => request["turn_ref"] = json!(captured[0]),
            _ => unreachable!(),
        }
        let error = super::control_admission::Admission::new(&f.workspace, &f.host)
            .control(&f.store, request)
            .unwrap_err();
        assert!(error.contains(field), "{variant}: {error}");
        assert!(error.contains(message), "{variant}: {error}");
        assert!(!error.contains("storage"), "{variant}: {error}");
        assert!(error.len() <= 2200);
        assert_eq!(f.store.binding(&f.host).unwrap(), before);
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            receipt_count
        );
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM learning_events", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert!(!f.marker().exists());
        assert!(
            !super::control_admission::local_capture_suppressed(&f.workspace, &f.host).unwrap()
        );
    }
}

#[test]
fn actual_store_failure_with_valid_reported_control_keeps_truthful_deny_fallback() {
    let f = Fixture::new();
    f.start();
    let before = f.store.binding(&f.host).unwrap();
    let receipt_count: i64 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM operation_receipts", [], |r| r.get(0))
        .unwrap();
    f.store
        .conn
        .execute_batch("ALTER TABLE workspace_meta RENAME TO unavailable_workspace_meta")
        .unwrap();
    let request = json!({"schema":SCHEMA,"action":"control","request_id":"storage-off","control":"recording_off",
        "reported_event":{"external_event_key":"current-off","speaker":"learner","kind":"message","text":"Stop recording now"}});
    let result =
        super::dispatch_with_host(&f.root, "learning_step", request, Some(f.host.clone())).unwrap();
    assert_eq!(result["error"]["code"], "storage_unavailable");
    assert_eq!(result["local_stop"], true);
    assert_eq!(result["durable"], false);
    assert_eq!(result["cross_process_protection"], "persisted_deny");
    assert_eq!(result["restart_durability"], "safety_latch");
    assert_eq!(result["capture_origin"], "model_reported");
    assert_eq!(f.store.binding(&f.host).unwrap(), before);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        receipt_count
    );
    assert!(
        Guard::acquire(&f.workspace, &f.host)
            .unwrap()
            .recording_blocked()
    );
}

#[test]
fn native_stop_control_cannot_be_validated_while_the_event_store_is_unreadable() {
    for control in ["pause", "stop", "recording_off"] {
        let f = Fixture::new();
        f.start();
        let before = f.store.binding(&f.host).unwrap();
        let receipt_count: i64 = f
            .store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |r| r.get(0))
            .unwrap();
        f.store
            .conn
            .execute_batch("DROP TABLE learning_events")
            .unwrap();
        let request = json!({"schema":SCHEMA,"action":"control","request_id":"native-stop","control":control,"turn_ref":"current-native"});
        let error =
            super::dispatch_with_host(&f.root, "learning_step", request, Some(f.host.clone()))
                .unwrap_err();
        assert!(
            error.starts_with("control_unavailable: turn_ref cannot be validated"),
            "{control}: {error}"
        );
        assert!(!error.contains("unknown_reference"), "{control}: {error}");
        assert_eq!(f.store.binding(&f.host).unwrap(), before, "{control}");
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            receipt_count,
            "{control}"
        );
        assert!(!f.marker().exists(), "{control}");
        assert!(
            !super::control_admission::local_capture_suppressed(&f.workspace, &f.host).unwrap(),
            "{control}"
        );
    }
}
