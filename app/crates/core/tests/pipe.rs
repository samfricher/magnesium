//! The pipe, with no window: one agent process driven through the manager.
//!
//! These tests run against `stand_in_agent.py`, which speaks the lines the
//! window expects from Claude Code and starts no model. They show the window
//! is consistent with the documented protocol. Only a run against the real
//! program shows that Magnesium's hooks fire as they do in a terminal; the
//! README gives that run as one command.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mg_core::agent::{AgentEvent, Answer, Decision, NoticeKind};
use mg_core::claude::ClaudeCode;
use mg_core::{Manager, Store};
use serde_json::Value;

struct Bench {
    _dir: tempfile::TempDir,
    folder: PathBuf,
    home: PathBuf,
    program: PathBuf,
    log: PathBuf,
}

impl Bench {
    fn new(extra_env: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("study");
        let home = dir.path().join("home");
        let state = dir.path().join("agent-state");
        for path in [&folder, &home, &state] {
            fs::create_dir_all(path).unwrap();
        }
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
        Self {
            _dir: dir,
            folder,
            home,
            program,
            log,
        }
    }

    fn manager(&self) -> (Manager, Events) {
        let (sender, receiver) = channel();
        let sender = Mutex::new(sender);
        let manager = Manager::new(
            Arc::new(Store::at(self.home.clone())),
            Arc::new(ClaudeCode::with_program(self.program.clone())),
            Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }),
        );
        manager.open_folder(&self.folder).unwrap();
        (
            manager,
            Events {
                receiver,
                seen: Vec::new(),
            },
        )
    }

    fn logged(&self, kind: &str) -> Vec<Value> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|entry| entry["kind"] == kind)
            .map(|entry| entry["value"].clone())
            .collect()
    }
}

struct Events {
    receiver: Receiver<AgentEvent>,
    seen: Vec<AgentEvent>,
}

impl Events {
    /// Wait for the first event that matches, keeping everything seen.
    fn until(&mut self, what: &str, matches: impl Fn(&AgentEvent) -> bool) -> AgentEvent {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .receiver
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {what} within 20 s; saw {:#?}", self.seen));
            self.seen.push(event.clone());
            if matches(&event) {
                return event;
            }
        }
    }

    fn turn_end(&mut self) -> AgentEvent {
        self.until("end of turn", |e| matches!(e, AgentEvent::TurnEnded { .. }))
    }

    /// The tutor's finished text since the last call.
    fn take_text(&mut self) -> String {
        let text = self
            .seen
            .iter()
            .filter_map(|event| match event {
                AgentEvent::TextDone { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        self.seen.clear();
        text
    }
}

fn ended_well(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::TurnEnded {
            ok: true,
            interrupted: false,
            error: None
        }
    )
}

#[test]
fn one_process_holds_a_lesson_across_several_turns() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    let opened = manager.open_session(true).unwrap();
    assert!(!opened.resumed, "a new folder has nothing to resume");

    manager.send("hello tutor").unwrap();
    let ready = events.until("ready", |e| matches!(e, AgentEvent::Ready { .. }));
    assert!(matches!(ready, AgentEvent::Ready { ref problems, .. } if problems.is_empty()));
    assert!(ended_well(&events.turn_end()));
    let streamed: String = events
        .seen
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        streamed, "You said: hello tutor",
        "the text arrives as it is written"
    );
    assert_eq!(events.take_text(), "You said: hello tutor");

    manager.send("HISTORY").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert!(events.take_text().starts_with("This is turn 2 of "));

    manager.send("HISTORY").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert!(events.take_text().starts_with("This is turn 3 of "));

    let starts = bench.logged("start");
    assert_eq!(starts.len(), 1, "three turns, one process");
    let args: Vec<&str> = starts[0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        !args.contains(&"--bare"),
        "bare mode would skip Magnesium's hooks"
    );
    assert!(args.contains(&"--include-partial-messages"));
    assert_eq!(
        Path::new(starts[0]["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        bench.folder.canonicalize().unwrap(),
        "the agent runs in the study folder, so it loads that folder's settings"
    );
}

#[test]
fn a_permission_request_is_allowed_once_always_and_refused() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();

    for (decision, expected) in [
        (Decision::AllowOnce, "The command ran."),
        (
            Decision::AllowAlways,
            "The command ran and it is remembered.",
        ),
        (
            Decision::Refuse,
            "The command was refused: The learner refused this.",
        ),
    ] {
        manager.send("PERMISSION please").unwrap();
        let request = events.until("permission request", |e| {
            matches!(e, AgentEvent::Permission { .. })
        });
        let AgentEvent::Permission {
            id, title, detail, ..
        } = request
        else {
            unreachable!()
        };
        assert_eq!(title, "Run a command");
        assert_eq!(detail, "tectonic paper.tex");
        manager.decide(&id, decision).unwrap();
        assert!(ended_well(&events.turn_end()));
        let text = events.take_text();
        assert!(text.starts_with(expected), "{decision:?} gave {text:?}");
    }
    assert!(
        manager.decide("gone", Decision::AllowOnce).is_err(),
        "a request can be answered once"
    );
}

#[test]
fn a_question_from_the_tutor_is_answered() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("QUESTION").unwrap();
    let asked = events.until("question", |e| matches!(e, AgentEvent::Question { .. }));
    let AgentEvent::Question { id, questions } = asked else {
        unreachable!()
    };
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].options.len(), 2);
    manager
        .answer(
            &id,
            &[Answer {
                question: questions[0].question.clone(),
                chosen: vec![questions[0].options[1].label.clone()],
            }],
        )
        .unwrap();
    assert!(ended_well(&events.turn_end()));
    assert_eq!(
        events.take_text(),
        r#"You chose: {"Which did you have in mind?": "the intercept is 1/Vmax"}"#
    );
}

#[test]
fn a_reply_is_interrupted_and_the_lesson_goes_on() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("SLOW").unwrap();
    events.until("first words", |e| matches!(e, AgentEvent::TextDelta { .. }));
    manager.interrupt().unwrap();
    let end = events.turn_end();
    assert!(
        matches!(
            end,
            AgentEvent::TurnEnded {
                ok: true,
                interrupted: true,
                error: None
            }
        ),
        "{end:?}"
    );
    events.take_text();

    manager.send("still there?").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert_eq!(events.take_text(), "You said: still there?");
    assert_eq!(
        bench.logged("start").len(),
        1,
        "an interrupt does not end the process"
    );
}

#[test]
fn a_closed_lesson_is_resumed_with_its_earlier_turns() {
    let bench = Bench::new(&[]);
    let session = {
        let (manager, mut events) = bench.manager();
        manager.open_session(true).unwrap();
        manager.send("first").unwrap();
        let ready = events.until("ready", |e| matches!(e, AgentEvent::Ready { .. }));
        let AgentEvent::Ready { session_id, .. } = ready else {
            unreachable!()
        };
        events.turn_end();
        manager.send("second").unwrap();
        events.turn_end();
        manager.stop(Some("step 3: what result collects")).unwrap();
        // A stopped conversation says nothing more, so the end is read from
        // the agent's own log.
        let deadline = Instant::now() + Duration::from_secs(20);
        while bench.logged("end").is_empty() {
            assert!(Instant::now() < deadline, "the process never ended");
            std::thread::sleep(Duration::from_millis(50));
        }
        session_id
    };

    // A new manager is the window opened again the next day.
    let (manager, mut events) = bench.manager();
    let home = manager.home().unwrap();
    assert!(home.can_resume);
    let opened = manager.open_session(true).unwrap();
    assert!(opened.resumed);
    assert_eq!(opened.turns.len(), 2);
    assert_eq!(opened.turns[0].learner, "first");
    assert_eq!(opened.turns[1].tutor, "You said: second");

    manager.send("HISTORY").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert_eq!(events.take_text(), format!("This is turn 3 of {session}."));

    let starts = bench.logged("start");
    assert_eq!(starts.len(), 2);
    let resume = format!("--resume={session}");
    assert!(starts[1]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == resume.as_str()));
    assert_eq!(
        manager.store().folder(&bench.folder).stopped_at.as_deref(),
        Some("step 3: what result collects")
    );

    // "New lesson" forgets the old conversation and starts without --resume.
    let opened = manager.open_session(false).unwrap();
    assert!(!opened.resumed && opened.turns.is_empty());
    manager.send_new_lesson("teach me enzyme kinetics").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert_eq!(
        events.take_text(),
        "You said: /magnesium:learn teach me enzyme kinetics"
    );
    let starts = bench.logged("start");
    assert!(!starts[2]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a.as_str().unwrap().starts_with("--resume")));
}

#[test]
fn a_replaced_conversation_says_nothing_more() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("SLOW").unwrap();
    events.until("first words", |e| matches!(e, AgentEvent::TextDelta { .. }));

    // A new lesson while the old reply is still being written.
    manager.open_session(false).unwrap();
    events.seen.clear();
    manager.send("hello new lesson").unwrap();
    let ready = events.until("ready", |e| matches!(e, AgentEvent::Ready { .. }));
    let AgentEvent::Ready { session_id, .. } = ready else {
        unreachable!()
    };
    assert!(ended_well(&events.turn_end()));
    assert_eq!(events.take_text(), "You said: hello new lesson");

    // Wait for the old process to end, then for anything it left behind.
    let deadline = Instant::now() + Duration::from_secs(20);
    while bench.logged("end").is_empty() {
        assert!(Instant::now() < deadline, "the old process never ended");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(800));
    let late: Vec<AgentEvent> = events.receiver.try_iter().collect();
    assert!(
        late.is_empty(),
        "the old conversation still spoke: {late:#?}"
    );

    let folder = bench.folder.canonicalize().unwrap();
    assert_eq!(
        manager.store().folder(&folder).session_id,
        Some(session_id),
        "the new conversation's id is kept"
    );
}

#[test]
fn a_stopped_reply_says_nothing_more() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("SLOW").unwrap();
    events.until("first words", |e| matches!(e, AgentEvent::TextDelta { .. }));
    manager.stop(None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while bench.logged("end").is_empty() {
        assert!(Instant::now() < deadline, "the process never ended");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(800));
    let late: Vec<AgentEvent> = events.receiver.try_iter().collect();
    assert!(
        late.is_empty(),
        "the stopped conversation still spoke: {late:#?}"
    );
    assert!(
        bench
            .logged("received")
            .iter()
            .any(|m| m["request"]["subtype"] == "interrupt"),
        "a reply under way is interrupted before the input closes"
    );
}

#[test]
fn a_missing_plugin_is_reported_with_the_first_reply() {
    let bench = Bench::new(&[("STAND_IN_NO_PLUGIN", "1")]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("hello").unwrap();
    let ready = events.until("ready", |e| matches!(e, AgentEvent::Ready { .. }));
    let AgentEvent::Ready {
        problems,
        plugin_missing,
        ..
    } = ready
    else {
        unreachable!()
    };
    assert!(plugin_missing);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("plugin did not load"));
    // Nothing judged the agent before it reported what it loaded.
    assert!(!events
        .seen
        .iter()
        .any(|e| matches!(e, AgentEvent::Notice { .. })));
}

#[test]
fn a_reply_is_interrupted_before_its_first_word() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("WAIT a long time").unwrap();
    // No word has arrived, and none will for five seconds.
    manager.interrupt().unwrap();
    let end = events.turn_end();
    assert!(
        matches!(
            end,
            AgentEvent::TurnEnded {
                ok: true,
                interrupted: true,
                error: None
            }
        ),
        "{end:?}"
    );
    assert!(events.take_text().is_empty());

    manager.send("and now?").unwrap();
    assert!(ended_well(&events.turn_end()));
    assert_eq!(events.take_text(), "You said: and now?");
    assert_eq!(bench.logged("start").len(), 1);
}

#[test]
fn words_written_before_an_interrupt_are_kept_for_the_next_sitting() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("SLOW").unwrap();
    events.until("first words", |e| matches!(e, AgentEvent::TextDelta { .. }));
    manager.interrupt().unwrap();
    events.turn_end();
    let folder = bench.folder.canonicalize().unwrap();
    let session = manager.store().folder(&folder).session_id.unwrap();
    let turns = manager.store().turns(&folder, &session);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].interrupted);
    assert!(turns[0].tutor.starts_with("word0"), "{:?}", turns[0].tutor);
}

#[test]
fn a_helpers_text_is_never_shown_as_the_tutors() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("HELPER at work").unwrap();
    assert!(ended_well(&events.turn_end()));
    let shown = format!("{:?}", events.seen);
    assert!(!shown.contains("SECRET HELPER TEXT"));
    assert_eq!(events.take_text(), "You said: HELPER at work");
}

#[test]
fn a_usage_limit_is_one_plain_line() {
    let bench = Bench::new(&[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(true).unwrap();
    manager.send("LIMIT").unwrap();
    let notice = events.until("notice", |e| matches!(e, AgentEvent::Notice { .. }));
    assert!(
        matches!(notice, AgentEvent::Notice { kind: NoticeKind::UsageLimit, ref text } if text.contains("usage limit"))
    );
    let end = events.turn_end();
    assert!(
        matches!(
            end,
            AgentEvent::TurnEnded {
                ok: false,
                error: None,
                ..
            }
        ),
        "{end:?}"
    );
}

#[test]
fn a_program_that_is_missing_gives_a_plain_message() {
    let bench = Bench::new(&[]);
    let (sender, _receiver) = channel::<AgentEvent>();
    let sender = Mutex::new(sender);
    let manager = Manager::new(
        Arc::new(Store::at(bench.home.clone())),
        Arc::new(ClaudeCode::with_program(
            bench.folder.join("no-such-program"),
        )),
        Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        }),
    );
    manager.open_folder(&bench.folder).unwrap();
    let error = manager
        .open_session(true)
        .expect_err("no program, no session");
    assert!(error.to_string().contains("Claude Code is not installed"));
}

#[test]
fn a_conversation_the_agent_no_longer_has_is_forgotten_not_asked_for_twice() {
    let bench = Bench::new(&[]);
    Store::at(bench.home.clone()).update(&bench.folder.canonicalize().unwrap(), |state| {
        state.session_id = Some("0b9f1c1e-7a52-4f6e-9a57-3f1b8d2c4e10".into());
    });
    let (manager, mut events) = bench.manager();
    assert!(manager.open_session(true).unwrap().resumed);
    let exit = events.until("exit", |e| matches!(e, AgentEvent::Exited { .. }));
    let AgentEvent::Exited { expected, detail } = exit else {
        unreachable!()
    };
    assert!(!expected);
    assert!(detail.unwrap().contains("No conversation found"));
    assert_eq!(manager.store().folder(&bench.folder).session_id, None);

    // The next try starts a new conversation and works.
    let opened = manager.open_session(true).unwrap();
    assert!(!opened.resumed);
    manager.send("hello again").unwrap();
    assert!(ended_well(&events.turn_end()));
}
