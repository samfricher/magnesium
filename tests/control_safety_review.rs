use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use support::{TestEnvironment, v2};

fn control_output(
    environment: &TestEnvironment,
    thread: &str,
    action: &str,
) -> std::process::Output {
    v2::tool_output(
        environment,
        "codex",
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

#[test]
fn headless_controls_do_not_bypass_busy_admission() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let thread = "busy-headless-control";
    v2::start(&environment, "codex", thread);
    let key = Sha256::digest(serde_json::to_vec(&["codex", thread]).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join(format!("control-safety/{key}.admission.lock")),
        )
        .unwrap();
    lock.lock().unwrap();
    for action in ["stop", "recording_off"] {
        let response = control(&environment, thread, action);
        assert!(
            response.to_string().contains("admission lock busy"),
            "{response}"
        );
        assert_eq!(response["durable"], false, "{response}");
        assert_eq!(response["cross_process_protection"], "unavailable");
        assert_eq!(response["error"]["code"], "control_safety_unavailable");
    }
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
    v2::start(&environment, "codex", thread);
    let conn = rusqlite::Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
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
            .join(support::CONTROL_DIR)
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
