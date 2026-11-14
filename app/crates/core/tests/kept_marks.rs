//! A lesson keeps, beside each turn, which of the record's marked answers
//! appeared for its message, so a reopened session shows each mark on the
//! answer it was shown on, also when several answers fall in one minute.
//! It is a test file of its own because it points the window at a stand-in
//! runtime through the environment, which no other test may share.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mg_core::agent::{AgentEvent, Intent};
use mg_core::claude::ClaudeCode;
use mg_core::{runtime, Manager, Store};
use serde_json::json;

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The record's rows, as the stand-in runtime prints them: all in one minute.
fn record(folder: &Path, ids: &[&str]) {
    let rows: Vec<_> = ids
        .iter()
        .map(|id| json!({"attempt_id": id, "answered_at": "2026-11-13 13:15", "mark": "right", "help": "none", "cold": false, "label": "sign of b", "paper": null}))
        .collect();
    // Moved into place whole, so the runtime never prints half a file.
    let temp = folder.join("attempts.json.tmp");
    fs::write(&temp, json!({ "attempts": rows }).to_string()).unwrap();
    fs::rename(temp, folder.join("attempts.json")).unwrap();
}

struct Events(Receiver<AgentEvent>);

impl Events {
    /// The events up to the first that matches.
    fn until(&self, what: &str, matches: impl Fn(&AgentEvent) -> bool) -> Vec<AgentEvent> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .0
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {what} within 30 s; saw {seen:#?}"));
            let done = matches(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }

    /// The ids of the marks shown live for the message, once the record has
    /// been read for it.
    fn marked(&self) -> Vec<String> {
        self.until("the record read", |e| matches!(e, AgentEvent::Settled))
            .into_iter()
            .filter_map(|event| match event {
                AgentEvent::Marked { mark } => Some(mark.id),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn each_turn_keeps_the_marks_shown_for_its_message() {
    let dir = tempfile::tempdir().unwrap();
    let folder: PathBuf = dir.path().join("study");
    let home = dir.path().join("home");
    let state = dir.path().join("agent-state");
    for path in [&folder, &home, &state] {
        fs::create_dir_all(path).unwrap();
    }
    // A runtime whose record is the file the test writes; it runs in the folder.
    let runtime_program = dir.path().join("magnesium");
    executable(
        &runtime_program,
        "#!/bin/sh\n[ \"$1\" = attempts ] || exit 2\ncat attempts.json 2>/dev/null || echo '{\"attempts\": []}'\n",
    );
    std::env::set_var(runtime::PROGRAM_ENV, &runtime_program);
    let agent = dir.path().join("claude");
    executable(
        &agent,
        &format!(
            "#!/bin/sh\nexport STAND_IN_LOG='{}'\nexport STAND_IN_STATE='{}'\nexec python3 '{}' \"$@\"\n",
            dir.path().join("agent.log").display(),
            state.display(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/stand_in_agent.py")
                .display()
        ),
    );
    let (sender, receiver) = channel();
    let sender = Mutex::new(sender);
    let manager = Manager::new(
        Arc::new(Store::at(home.clone())),
        Arc::new(ClaudeCode::with_program(agent)),
        Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        }),
    );
    manager.open_folder(&folder).unwrap();
    manager.open_session(true).unwrap();
    let events = Events(receiver);

    // Three answers in one minute. The first is marked; the second has two
    // rows (a task in two parts); the third has none.
    manager
        .send_with("x = 2", Some(Intent::Answer))
        .expect("sent");
    record(&folder, &["a-1"]);
    let ready = events.until("ready", |e| matches!(e, AgentEvent::Ready { .. }));
    let session = ready
        .iter()
        .find_map(|event| match event {
            AgentEvent::Ready { session_id, .. } => Some(session_id.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(events.marked(), ["a-1"]);

    manager
        .send_with("x = 2 and x = 3", Some(Intent::Answer))
        .expect("sent");
    record(&folder, &["a-1", "a-2", "a-3"]);
    assert_eq!(events.marked(), ["a-2", "a-3"]);

    // The third message's record is still being read when the learner writes
    // again, so nothing is kept for it; the fourth's row is.
    manager
        .send_with("x = 3", Some(Intent::Answer))
        .expect("sent");
    events.until("end of turn", |e| matches!(e, AgentEvent::TurnEnded { .. }));
    // After the record's first reading for it, and before its next.
    std::thread::sleep(Duration::from_millis(300));
    manager
        .send_with("x = -3", Some(Intent::Answer))
        .expect("sent");
    record(&folder, &["a-1", "a-2", "a-3", "a-4"]);
    assert_eq!(events.marked(), ["a-4"]);

    let kept: Vec<Option<Vec<String>>> = manager
        .lesson_turns(None, &session)
        .unwrap()
        .into_iter()
        .map(|turn| turn.mark_ids)
        .collect();
    let ids = |ids: &[&str]| Some(ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>());
    assert_eq!(
        kept,
        [ids(&["a-1"]), ids(&["a-2", "a-3"]), None, ids(&["a-4"])]
    );
}
