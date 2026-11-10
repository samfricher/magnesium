//! How lessons are paid for, and signing in, against the stand-in agent.
//!
//! The learner chooses their plan or an API key. The window starts the agent
//! so the choice holds, shows what the agent itself reports, and says so when
//! a lesson is paid another way. It never reads a credential.
//!
//! This file sets an API key in its own process environment, as a learner's
//! shell can, so it runs apart from the other pipe tests.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mg_core::agent::{AgentEvent, NoticeKind, Payment};
use mg_core::claude::ClaudeCode;
use mg_core::{Harness, Manager, Store};
use serde_json::Value;

struct Bench {
    dir: tempfile::TempDir,
    program: PathBuf,
    log: PathBuf,
}

impl Bench {
    fn new(extra_env: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("agent-state");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(dir.path().join("study")).unwrap();
        let log = dir.path().join("agent.log");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stand_in_agent.py");
        let mut wrapper = format!(
            "#!/bin/sh\nexport STAND_IN_LOG='{}'\nexport STAND_IN_STATE='{}'\n",
            log.display(),
            state.display()
        );
        for (name, value) in extra_env {
            wrapper.push_str(&format!("export {name}='{value}'\n"));
        }
        wrapper.push_str(&format!("exec python3 '{}' \"$@\"\n", script.display()));
        let program = dir.path().join("claude");
        fs::write(&program, wrapper).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, program, log }
    }

    fn agent(&self) -> ClaudeCode {
        ClaudeCode::with_program(self.program.clone())
    }

    /// Start a lesson paid as chosen, and return what the agent reported
    /// before its first reply.
    fn lesson(&self, payment: Payment) -> Vec<AgentEvent> {
        let (sender, events) = channel();
        let sender = Mutex::new(sender);
        let store = Arc::new(Store::at(self.dir.path().join("window")));
        store.set_payment(payment);
        let manager = Manager::new(
            store,
            Arc::new(self.agent()),
            Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }),
        );
        manager.open_folder(&self.dir.path().join("study")).unwrap();
        manager.open_session(false).unwrap();
        manager.send("hello").unwrap();
        let seen = until_turn_ends(&events);
        manager.shut_down();
        seen
    }

    /// Whether the agent's last start saw an API key in its environment.
    fn last_start_had_a_key(&self) -> bool {
        let text = fs::read_to_string(&self.log).unwrap();
        let starts: Vec<Value> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|entry| entry["kind"] == "start" && entry["value"]["args"][0] == "-p")
            .collect();
        starts.last().unwrap()["value"]["api_key"]
            .as_bool()
            .unwrap()
    }
}

fn until_turn_ends(events: &Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut seen = Vec::new();
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("the turn did not end; so far: {seen:#?}"));
        let ended = matches!(event, AgentEvent::TurnEnded { .. });
        seen.push(event);
        if ended {
            return seen;
        }
    }
}

fn payment_notices(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text,
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_plan_is_used_even_with_an_api_key_on_the_mac_and_the_window_shows_which() {
    // A key in the learner's environment, such as one their shell exports.
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-not-a-real-key");
    let bench = Bench::new(&[]);

    let plan = bench.agent().account(Payment::Plan);
    assert!(plan.installed && plan.signed_in, "{plan:?}");
    assert_eq!(plan.paid_with, Some(Payment::Plan));
    assert_eq!(plan.detail.as_deref(), Some("your Claude Max plan"));
    assert_eq!(plan.problem, None);
    let seen = bench.lesson(Payment::Plan);
    assert!(!bench.last_start_had_a_key(), "the key reached the agent");
    assert!(payment_notices(&seen).is_empty(), "{seen:#?}");

    let key = bench.agent().account(Payment::ApiKey);
    assert_eq!(key.paid_with, Some(Payment::ApiKey));
    assert!(key.detail.unwrap().contains("ANTHROPIC_API_KEY"));
    assert_eq!(key.problem, None);
    let seen = bench.lesson(Payment::ApiKey);
    assert!(bench.last_start_had_a_key());
    assert!(payment_notices(&seen).is_empty(), "{seen:#?}");

    // A key the agent keeps in its own settings is not in the environment,
    // so the window cannot hold it back. It says so, before and during.
    let helper = Bench::new(&[("STAND_IN_KEY_HELPER", "1")]);
    let account = helper.agent().account(Payment::Plan);
    assert_eq!(account.paid_with, Some(Payment::ApiKey));
    assert!(
        account.problem.unwrap().contains("not your Claude plan"),
        "the start screen says the plan is not in use"
    );
    let notices = payment_notices(&helper.lesson(Payment::Plan));
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("not your Claude plan"), "{notices:?}");
    std::env::remove_var("ANTHROPIC_API_KEY");
}

#[test]
fn signed_out_the_window_starts_the_agents_own_sign_in_and_sees_the_result() {
    let bench = Bench::new(&[("STAND_IN_SIGNED_OUT", "1")]);
    let agent = bench.agent();
    let before = agent.account(Payment::Plan);
    assert!(before.installed && !before.signed_in, "{before:?}");
    assert!(before.problem.unwrap().contains("not signed in"));

    agent.sign_in(Payment::Plan).unwrap();
    let started = Instant::now();
    loop {
        let account = agent.account(Payment::Plan);
        if account.signed_in {
            assert_eq!(account.paid_with, Some(Payment::Plan));
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "never signed in"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let logins: Vec<Value> = fs::read_to_string(&bench.log)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["value"]["args"][1] == "login")
        .collect();
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0]["value"]["args"][2], "--claudeai");
}

#[test]
fn without_the_agent_the_window_names_its_install_page() {
    let bench = Bench::new(&[]);
    let missing = ClaudeCode::with_program(bench.dir.path().join("not-installed"));
    let account = missing.account(Payment::Plan);
    assert!(!account.installed && !account.signed_in);
    assert!(account
        .install_page
        .unwrap()
        .starts_with("https://code.claude.com/"));
    assert!(missing.sign_in(Payment::Plan).is_err());
}
