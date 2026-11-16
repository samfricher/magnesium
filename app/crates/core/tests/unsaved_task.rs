//! On Claude Code's engine the tutor keeps the record, and can set a task
//! and end its turn without saving it. The record then names no open task.
//! The window says so with `fallback`, so the page still takes a task the
//! reply opens with a bold word, and the learner's answer is sent as one.
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
use serde_json::{json, Value};

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// What the stand-in runtime prints for `magnesium lesson --json`.
fn record_says(folder: &Path, lesson: Value) {
    // Moved into place whole, so the runtime never prints half a file.
    let temp = folder.join("lesson.json.tmp");
    fs::write(&temp, json!({ "lesson": lesson }).to_string()).unwrap();
    fs::rename(temp, folder.join("lesson.json")).unwrap();
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

    /// What the window says of the open task once the turn has ended. It
    /// reads the record for that at once, before it looks for marks.
    fn open_task(&self) -> (Option<String>, bool) {
        self.until("word of the open task", |e| {
            matches!(e, AgentEvent::OpenTask { .. })
        })
        .into_iter()
        .rev()
        .find_map(|event| match event {
            AgentEvent::OpenTask { task, fallback, .. } => Some((task, fallback)),
            _ => None,
        })
        .unwrap()
    }

    /// Whether the window says anything of the open task in the two seconds
    /// after the turn ends.
    fn says_nothing_of_the_task(&self) -> bool {
        let mut seen = self.until("end of turn", |e| matches!(e, AgentEvent::TurnEnded { .. }));
        let deadline = Instant::now() + Duration::from_secs(2);
        while let Ok(event) = self
            .0
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            seen.push(event);
        }
        !seen
            .iter()
            .any(|e| matches!(e, AgentEvent::OpenTask { .. }))
    }
}

#[test]
fn a_task_the_record_does_not_name_may_still_be_found_by_its_bold_word() {
    let dir = tempfile::tempdir().unwrap();
    let folder: PathBuf = dir.path().join("study");
    let home = dir.path().join("home");
    let state = dir.path().join("agent-state");
    for path in [&folder, &home, &state] {
        fs::create_dir_all(path).unwrap();
    }
    // A runtime whose lesson is the file the test writes; it runs in the folder.
    let runtime_program = dir.path().join("magnesium");
    executable(
        &runtime_program,
        "#!/bin/sh\ncase \"$1\" in\n  lesson) cat lesson.json 2>/dev/null || echo '{\"lesson\": null}' ;;\n  attempts) echo '{\"attempts\": []}' ;;\n  *) exit 2 ;;\nesac\n",
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
    let task = "A task, please.\n\n**Task.** What is 2 + 2?";

    // The record has no lesson for this conversation: it says nothing of its
    // tasks, so the window says nothing, and the page finds the task itself.
    manager.send_with(task, Some(Intent::Talk)).expect("sent");
    assert!(events.says_nothing_of_the_task());

    // The record has the lesson and names no task: the tutor set one and did
    // not save it. The window says no task is named, and that the page may
    // take the one the reply opens with a bold word.
    record_says(
        &folder,
        json!({"goal": "Mental sums", "steps": [], "open_task": null}),
    );
    manager.send_with(task, Some(Intent::Talk)).expect("sent");
    assert_eq!(events.open_task(), (None, true));

    // The record names the task: its words are the task, with no fallback.
    record_says(
        &folder,
        json!({"goal": "Mental sums", "steps": [], "open_task":
            {"kind": "task", "text": "What is 2 + 2?", "posed_at_ms": i64::MAX, "whole_reply": false}}),
    );
    manager.send_with(task, Some(Intent::Talk)).expect("sent");
    assert_eq!(
        events.open_task(),
        (Some("What is 2 + 2?".to_owned()), false)
    );
}
