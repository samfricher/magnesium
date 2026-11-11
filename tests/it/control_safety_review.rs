use crate::support::{TestEnvironment, v2};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;

fn control_output(
    environment: &TestEnvironment,
    thread: &str,
    action: &str,
) -> std::process::Output {
    v2::tool_output(
        environment,
        "claude",
        thread,
        "learning_step",
        json!({
            "schema":"learner-model/v2", "action":"control", "request_id":format!("control-{action}"),
            "control":action, "reported_event":{"external_event_key":format!("event-{action}"),
            "speaker":"learner", "kind":"message", "text":format!("Please {action} now.")}
        }),
    )
}

fn control(environment: &TestEnvironment, thread: &str, action: &str) -> Value {
    serde_json::from_slice(&control_output(environment, thread, action).stdout).unwrap()
}

/// A started lesson whose admission lock another process holds, as when a
/// second control request is in progress.
fn busy_lesson(thread: &str) -> (TestEnvironment, fs::File) {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    v2::start(&environment, "claude", thread);
    let key = Sha256::digest(
        serde_json::to_vec(&["claude", &format!("magnesium-h1:claude:{thread}")]).unwrap(),
    )
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            environment
                .workspace
                .join(crate::support::CONTROL_DIR)
                .join(format!("control-safety/{key}.admission.lock")),
        )
        .unwrap();
    lock.lock().unwrap();
    (environment, lock)
}

// Each refused control waits for the lock before it gives up, so each control
// has a test of its own and the waits overlap.
fn busy_admission_refuses(action: &str) {
    let thread = "busy-headless-control";
    let (environment, lock) = busy_lesson(thread);
    let response = control(&environment, thread, action);
    assert!(
        response.to_string().contains("admission lock busy"),
        "{response}"
    );
    assert_eq!(response["durable"], false, "{response}");
    assert_eq!(response["cross_process_protection"], "unavailable");
    assert_eq!(response["error"]["code"], "control_safety_unavailable");
    assert_eq!(v2::count(&environment, "operation_receipts"), 1);
    drop(lock);
}

#[test]
fn headless_stop_does_not_bypass_busy_admission() {
    busy_admission_refuses("stop");
}

#[test]
fn headless_recording_off_does_not_bypass_busy_admission() {
    busy_admission_refuses("recording_off");
}

#[test]
fn headless_recording_on_does_not_bypass_busy_admission_and_stop_works_once_free() {
    let thread = "busy-headless-control";
    let (environment, lock) = busy_lesson(thread);
    let denied_on = control_output(&environment, thread, "recording_on");
    assert!(!denied_on.status.success(), "{denied_on:?}");
    assert!(
        String::from_utf8_lossy(&denied_on.stderr).contains("admission lock busy"),
        "{denied_on:?}"
    );
    assert_eq!(v2::count(&environment, "operation_receipts"), 1);
    drop(lock);
    let stopped = control(&environment, thread, "stop");
    assert_eq!(
        stopped["cross_process_protection"], "canonical_control",
        "{stopped}"
    );
    assert_eq!(v2::count(&environment, "operation_receipts"), 2);
}

#[test]
fn headless_binding_read_failure_discloses_the_persisted_deny() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let thread = "headless-binding-failure";
    v2::start(&environment, "claude", thread);
    let conn = rusqlite::Connection::open(
        environment
            .workspace
            .join(crate::support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    conn.execute_batch("DROP TABLE host_bindings").unwrap();
    let response = control(&environment, thread, "recording_off");
    assert_eq!(
        response["cross_process_protection"], "persisted_deny",
        "{response}"
    );
    assert_eq!(response["restart_durability"], "safety_latch");
    assert_eq!(response["local_stop"], true);
    assert!(response.get("error").is_some());
    let marker = fs::read_dir(
        environment
            .workspace
            .join(crate::support::CONTROL_DIR)
            .join("control-safety"),
    )
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .find(|path| {
        path.extension()
            .is_some_and(|extension| extension == "deny")
    })
    .unwrap();
    assert_eq!(fs::read(marker).unwrap(), [1, 1]);
    assert_eq!(v2::count(&environment, "operation_receipts"), 1);
}
