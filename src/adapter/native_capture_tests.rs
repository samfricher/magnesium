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
                host_kind: "claude".into(),
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
fn a_ticket_outlasts_a_slow_first_reply_and_then_expires() {
    let f = Fixture::new();
    let text = "Teach me coefficient roles";
    let ticket = native_capture::issue(&f.workspace, &f.host, "turn", text, 1000).unwrap();
    let twenty_minutes = 20 * 60 * 1000;
    assert!(
        native_capture::verify(&f.workspace, &f.host, &ticket, text, 1000 + twenty_minutes).is_ok()
    );
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
fn complete_then_new_native_hook_can_start_a_new_session_in_same_thread() {
    let f = Fixture::new();
    let native_host = super::HostIdentity::claude("restart-same-thread").unwrap();
    let host = super::host_context(&native_host);
    let initial=f.store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":"initial-session","session":{"operation":"start","client_ref":"first","goal":"First lesson"}})).unwrap();
    f.store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":"complete-first","session":{"operation":"complete","session_id":initial.references["first"],"expected_revision":1}})).unwrap();
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
