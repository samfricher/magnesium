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

fn record_says(folder: &Path, lesson: Value) {
    let temp = folder.join("lesson.json.tmp");
    fs::write(&temp, json!({ "lesson": lesson }).to_string()).unwrap();
    fs::rename(temp, folder.join("lesson.json")).unwrap();
}

struct Events(Receiver<AgentEvent>);

impl Events {
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

    manager.send_with(task, Some(Intent::Talk)).expect("sent");
    assert!(events.says_nothing_of_the_task());

    record_says(
        &folder,
        json!({"goal": "Mental sums", "steps": [], "open_task": null}),
    );
    manager.send_with(task, Some(Intent::Talk)).expect("sent");
    assert_eq!(events.open_task(), (None, true));

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
