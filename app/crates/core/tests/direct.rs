//! The direct engine with no window: the manager drives a stand-in model
//! (`stand_in_model.py`), and the record is kept by the real runtime built
//! from this checkout, in a scratch study folder with a scratch home.
//!
//! They show that code keeps the record from the model's record block: the
//! mark, the help given across turns, the goal's steps, the reply as shown,
//! and what a resumed or switched conversation carries on. Only a run
//! against the real program shows how a real model writes the block; see
//! `live.rs`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use mg_core::agent::{AgentEvent, NoticeKind};
use mg_core::claude::ClaudeCode;
use mg_core::direct::DirectTutor;
use mg_core::store::Engine;
use mg_core::study::Paper;
use mg_core::{Manager, Store};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

/// The runtime from this checkout, built once for all the tests here.
fn runtime_binary() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            if let Some(binary) =
                std::env::var_os("MAGNESIUM_TEST_BINARY").filter(|v| !v.is_empty())
            {
                return PathBuf::from(binary);
            }
            let root = repo_root();
            let status = Command::new("cargo")
                .current_dir(&root)
                .env("CARGO_TARGET_DIR", root.join("target"))
                .args(["build", "--release", "--locked", "--bin", "magnesium"])
                .status()
                .expect("cargo runs");
            assert!(status.success(), "the runtime did not build");
            root.join("target/release/magnesium")
        })
        .clone()
}

/// The window's own readers, such as the check that a folder needs no
/// upgrade, find the runtime by this variable. Every test here sets the same
/// value.
fn point_readers_at_the_runtime() {
    static SET: OnceLock<()> = OnceLock::new();
    SET.get_or_init(|| {
        std::env::set_var(mg_core::runtime::PROGRAM_ENV, runtime_binary());
        // A tool call takes milliseconds; a short limit lets a test of a hung
        // server finish soon.
        std::env::set_var(mg_core::host_client::CALL_LIMIT_ENV, "2000");
    });
}

fn script(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Bench {
    _dir: tempfile::TempDir,
    root: PathBuf,
    folder: PathBuf,
    window: PathBuf,
    runtime: PathBuf,
    /// Files whose presence makes the runtime misbehave, for a test.
    flags: PathBuf,
    model: PathBuf,
    plugin: PathBuf,
    log: PathBuf,
}

impl Bench {
    /// A study root set up by the runtime, and the folder lessons open in:
    /// the root itself, or `nested` below it.
    fn new(nested: Option<&str>, extra_env: &[(&str, &str)]) -> Self {
        point_readers_at_the_runtime();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("Course");
        let home = base.join("home");
        let window = base.join("window");
        for path in [&root, &home, &window] {
            fs::create_dir_all(path).unwrap();
        }
        let runtime = base.join("magnesium");
        let flags = base.join("flags");
        fs::create_dir_all(&flags).unwrap();
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        script(
            &runtime,
            &format!(
                "#!/bin/sh\nexport HOME='{home}'\nexport XDG_CONFIG_HOME='{home}/.config'\nexport XDG_DATA_HOME='{home}/.local/share'\nexport XDG_CACHE_HOME='{home}/.cache'\n\
                 if [ \"$1 $2 $3\" = \"_internal hook user-prompt-submit\" ] && [ -e '{flags}/fail-prompt-hook' ]; then cat >/dev/null; exit 0; fi\n\
                 if [ \"$1 $2\" = \"_internal mcp-server\" ] && [ -e '{flags}/hang' ]; then HANG_FLAG='{flags}/hang' exec python3 '{proxy}' '{bin}' \"$@\"; fi\n\
                 if [ \"$1 $2\" = \"_internal mcp-server\" ] && [ -e '{flags}/slow' ]; then DELAY_EACH=\"$(cat '{flags}/slow')\" exec python3 '{proxy}' '{bin}' \"$@\"; fi\n\
                 exec '{bin}' \"$@\"\n",
                bin = runtime_binary().display(),
                home = home.display(),
                flags = flags.display(),
                proxy = tests.join("hang_proxy.py").display(),
            ),
        );
        let init = Command::new(&runtime)
            .current_dir(&root)
            .args(["init", "--json"])
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );
        let folder = match nested {
            Some(path) => {
                let folder = root.join(path);
                fs::create_dir_all(&folder).unwrap();
                folder
            }
            None => root.clone(),
        };
        let log = base.join("model.log");
        let mut exports = format!("export STAND_IN_LOG='{}'\n", log.display());
        for (name, value) in extra_env {
            exports.push_str(&format!("export {name}='{value}'\n"));
        }
        let model = base.join("claude");
        script(
            &model,
            &format!(
                "#!/bin/sh\n{exports}exec python3 '{}' \"$@\"\n",
                tests.join("stand_in_model.py").display()
            ),
        );
        let plugin = base.join("claude-plugin-engine");
        let state = base.join("agent-state");
        fs::create_dir_all(&state).unwrap();
        script(
            &plugin,
            &format!(
                "#!/bin/sh\nexport STAND_IN_LOG='{}'\nexport STAND_IN_STATE='{}'\nexec python3 '{}' \"$@\"\n",
                base.join("plugin.log").display(),
                state.display(),
                tests.join("stand_in_agent.py").display()
            ),
        );
        Self {
            _dir: dir,
            root,
            folder,
            window,
            runtime,
            flags,
            model,
            plugin,
            log,
        }
    }

    fn manager(&self) -> (Manager, Events) {
        let (sender, receiver) = channel();
        let sender = Mutex::new(sender);
        let store = Arc::new(Store::at(self.window.clone()));
        let manager = Manager::new(
            Arc::clone(&store),
            Arc::new(ClaudeCode::with_program(self.plugin.clone())),
            Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }),
        )
        .with_direct(Arc::new(
            DirectTutor::with_programs(self.model.clone(), self.runtime.clone()).with_store(store),
        ));
        manager.set_engine(Engine::Direct);
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

    fn cli(&self, args: &[&str]) -> Value {
        let out = Command::new(&self.runtime)
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("{args:?}: {}", String::from_utf8_lossy(&out.stdout)))
    }

    /// The answers on record, once the last turn's record is kept.
    fn attempts(&self) -> Vec<Value> {
        self.attempts_now()
    }

    fn attempts_now(&self) -> Vec<Value> {
        self.cli(&["attempts", "--json"])["attempts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn store(&self) -> Connection {
        Connection::open_with_flags(
            self.root.join(".magnesium/state.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }

    /// The tutor's replies the runtime kept, oldest first.
    fn deliveries(&self) -> Vec<String> {
        let conn = self.store();
        let mut statement = conn
            .prepare("SELECT text FROM learning_events WHERE speaker='tutor' AND kind='delivery' ORDER BY event_no")
            .unwrap();
        statement
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn size(&self) -> (i64, usize) {
        let events: i64 = self
            .store()
            .query_row("SELECT count(*) FROM learning_events", [], |r| r.get(0))
            .unwrap();
        (events, self.attempts_now().len())
    }

    fn sessions(&self) -> i64 {
        self.store()
            .query_row("SELECT count(*) FROM learning_sessions", [], |r| r.get(0))
            .unwrap()
    }
}

struct Events {
    receiver: Receiver<AgentEvent>,
    seen: Vec<AgentEvent>,
}

impl Events {
    fn until(&mut self, what: &str, matches: impl Fn(&AgentEvent) -> bool) -> AgentEvent {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .receiver
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no {what} within 30 s; saw {:#?}", self.seen));
            self.seen.push(event.clone());
            if matches(&event) {
                return event;
            }
        }
    }

    fn turn_end(&mut self) -> AgentEvent {
        self.until("end of turn", |e| matches!(e, AgentEvent::TurnEnded { .. }))
    }

    /// The reply shown in the last turn, and every piece shown while it
    /// streamed, joined.
    fn take(&mut self) -> (String, String) {
        let done = self
            .seen
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDone { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let pieces: String = self
            .seen
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        self.seen.clear();
        (done, pieces)
    }
}

/// Wait for the end of a turn, then for its record: the direct engine
/// ends a turn when the reply is whole and keeps its record just after,
/// and says when it has.
fn done(manager: &Manager, events: &mut Events) -> AgentEvent {
    let end = events.turn_end();
    assert!(
        manager.wait_saved(Duration::from_secs(30)),
        "the record was not kept within 30 s"
    );
    end
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
fn a_direct_lesson_streams_its_reply_and_code_keeps_the_record() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    let opened = manager.open_session(false).unwrap();
    assert_eq!(opened.engine, Engine::Direct);
    manager.send_new_lesson("TEACH me quadratics").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    // Ready, and the id remembered, once Claude Code showed it has the
    // conversation: with the first message.
    assert!(events
        .seen
        .iter()
        .any(|e| matches!(e, AgentEvent::Ready { .. })));
    let (reply, pieces) = events.take();
    assert_eq!(reply, "Let's start. What is b in x² − 5x + 6 = 0?");
    assert_eq!(
        pieces, reply,
        "the pieces shown are the reply, and no part of the block"
    );

    // Claude Code was started as the model alone.
    let start = &bench.logged("start")[0];
    let args: Vec<String> = serde_json::from_value(start["args"].clone()).unwrap();
    for flag in [
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--system-prompt",
    ] {
        assert!(args.contains(&flag.to_owned()), "{flag}: {args:?}");
    }
    assert!(!args.iter().any(|a| a == "--add-dir"), "{args:?}");
    assert_eq!(start["env"]["CLAUDE_CODE_DISABLE_CLAUDE_MDS"], "1");
    assert_eq!(start["env"]["ENABLE_CLAUDEAI_MCP_SERVERS"], "false");
    assert!(start["env"]["MAGNESIUM_SURFACE"].is_null());
    // The first message carried the whole state, and the learner's words
    // without the window's mark.
    let first = &bench.logged("message")[0];
    assert_eq!(first["learner"], "TEACH me quadratics");
    let state = first["state"].as_str().unwrap();
    assert!(state.contains("Goal: TEACH me quadratics"), "{state}");
    assert!(state.contains("Quick checks due: none"), "{state}");

    // A wrong answer is marked, with its question.
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    events.take();
    // The Hint button, then a right answer: code keeps the hint, though the
    // block of the answer says none.
    manager.send("Can I have a hint?").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    events.take();
    manager.send("b = -5\n\nConfidence: sure").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let (reply, _) = events.take();
    assert!(
        reply.starts_with("Right: the sign goes with the number."),
        "{reply}"
    );

    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    let wrong = attempts.iter().find(|a| a["mark"] == "wrong").unwrap();
    assert_eq!(wrong["question"], "What is b in x² − 5x + 6 = 0?");
    assert_eq!(wrong["label"], "sign of b");
    assert_eq!(wrong["help"], "none");
    assert_eq!(wrong["kind"], "reading coefficients");
    let right = attempts.iter().find(|a| a["mark"] == "right").unwrap();
    assert_eq!(right["question"], "What is b in x² + 3x − 4 = 0?");
    assert_eq!(right["help"], "hint");
    assert_eq!(right["confidence"], "sure");

    // The steps and the next action are where the window reads them.
    let digest = bench.cli(&["digest"]);
    let session = &digest["record"]["sessions"][0];
    assert_eq!(
        session["next_action"],
        "Solve x² − 5x + 6 = 0 with the formula."
    );
    let steps = mg_core::peek::goal_steps(&bench.root);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].status, "done");
    assert_eq!(steps[1].status, "doing");
    // Each reply is kept as it was shown, without the block.
    let kept = bench.deliveries();
    assert_eq!(kept.len(), 4, "{kept:#?}");
    assert_eq!(kept[0], "Let's start. What is b in x² − 5x + 6 = 0?");
    assert!(kept.iter().all(|text| !text.contains("<record>")));
    // The help a kind needs went to the model with the next message.
    let messages = bench.logged("message");
    assert!(
        messages[2]["state"]
            .as_str()
            .unwrap()
            .contains("\"reading coefficients\""),
        "{messages:#?}"
    );
    // The misunderstanding the block named was written to the learner's
    // memory of that skill.
    let memory: String = bench
        .store()
        .query_row("SELECT title FROM learner_memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(memory, "sign of b");
    manager.shut_down();
}

#[test]
fn a_missing_block_is_asked_for_again_and_a_message_meanwhile_waits() {
    let bench = Bench::new(None, &[("STAND_IN_REPAIR_DELAY", "1.5")]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("NOBLOCK b = 5").unwrap();
    // The reply is shown at once; its record is asked for in the background.
    events.until("the reply", |e| matches!(e, AgentEvent::TextDone { .. }));
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert!(ended_well(&done(&manager, &mut events)));
    let messages = bench.logged("message");
    let order: Vec<String> = messages
        .iter()
        .map(|m| {
            if m["state"]
                .as_str()
                .unwrap_or_default()
                .contains("could not be read")
            {
                "hidden".to_owned()
            } else {
                m["learner"].as_str().unwrap_or_default().to_owned()
            }
        })
        .collect();
    assert_eq!(order, ["TEACH me", "NOBLOCK b = 5", "hidden", "b = -5"]);
    // The hidden request is never shown and never kept as a reply.
    assert!(bench
        .deliveries()
        .iter()
        .all(|text| !text.contains("record")));
    manager.shut_down();
}

#[test]
fn an_answer_whose_block_cannot_be_read_twice_is_left_unmarked_with_a_notice() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("NEVERBLOCK b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    // The notice comes with the record, just after the turn's end.
    let notice = match events.until("a notice", |e| {
        matches!(
            e,
            AgentEvent::Notice {
                kind: NoticeKind::Other,
                ..
            }
        )
    }) {
        AgentEvent::Notice { text, .. } => text,
        _ => unreachable!(),
    };
    assert!(notice.contains("not marked"), "{notice}");
    assert!(bench.attempts().is_empty());
    // The reply was still shown and kept.
    assert_eq!(bench.deliveries()[1], "A reply with no notes.");
    manager.shut_down();
}

#[test]
fn an_interrupted_reply_gets_no_mark_and_the_task_stays_open() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("SLOW b = 5").unwrap();
    events.until("the first words", |e| {
        matches!(e, AgentEvent::TextDelta { .. })
    });
    manager.interrupt().unwrap();
    let end = done(&manager, &mut events);
    assert!(
        matches!(
            end,
            AgentEvent::TurnEnded {
                interrupted: true,
                ..
            }
        ),
        "{end:?}"
    );
    assert!(bench.attempts().is_empty());
    // The open task is still the first one: the next answer is marked on it.
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["question"], "What is b in x² − 5x + 6 = 0?");
    manager.shut_down();
}

#[test]
fn a_lesson_in_a_folder_below_the_study_root_reads_the_root_and_keeps_its_record() {
    let bench = Bench::new(Some("stats/week1"), &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let start = &bench.logged("start")[0];
    let args: Vec<String> = serde_json::from_value(start["args"].clone()).unwrap();
    let at = args
        .iter()
        .position(|a| a == "--add-dir")
        .expect("--add-dir");
    assert_eq!(Path::new(&args[at + 1]), bench.root);
    assert_eq!(Path::new(start["cwd"].as_str().unwrap()), bench.folder);
    assert_eq!(bench.attempts().len(), 1);
    manager.shut_down();
}

#[test]
fn a_closed_lesson_resumes_its_conversation_and_its_open_task() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    let session = match events
        .seen
        .iter()
        .find(|e| matches!(e, AgentEvent::Ready { .. }))
    {
        Some(AgentEvent::Ready { session_id, .. }) => session_id.clone(),
        _ => panic!("no ready"),
    };
    manager.stop(None).unwrap();
    drop(manager);

    let (manager, mut events) = bench.manager();
    let opened = manager.open_session(true).unwrap();
    assert!(opened.resumed);
    assert_eq!(opened.engine, Engine::Direct);
    assert_eq!(opened.turns.len(), 1);
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let starts = bench.logged("start");
    assert!(
        starts[1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a.as_str() == Some(&format!("--resume={session}"))),
        "{starts:#?}"
    );
    // The open task came back from the record, so the answer is marked on it.
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["question"], "What is b in x² − 5x + 6 = 0?");
    assert_eq!(bench.sessions(), 1);
    manager.shut_down();
}

#[test]
fn a_switch_of_engine_starts_a_new_conversation_on_the_same_lesson() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    assert_eq!(
        manager.engine(),
        mg_core::manager::EngineChoice {
            chosen: Engine::Direct,
            conversation: Some(Engine::Direct)
        }
    );

    // Switched to the plugin engine: the remembered conversation is the
    // direct engine's, so a new one starts.
    let choice = manager.set_engine(Engine::Plugin);
    assert_eq!(choice.chosen, Engine::Plugin);
    let opened = manager.open_session(true).unwrap();
    assert!(!opened.resumed);
    assert_eq!(opened.engine, Engine::Plugin);
    manager.send("hello").unwrap();
    done(&manager, &mut events);
    assert_eq!(manager.engine().conversation, Some(Engine::Plugin));

    // And back: the plugin conversation is not resumed by the direct engine.
    manager.set_engine(Engine::Direct);
    let opened = manager.open_session(true).unwrap();
    assert!(!opened.resumed);
    assert_eq!(opened.engine, Engine::Direct);
    // The window's opening line resumes the runtime's lesson: no new one.
    manager
        .send_new_lesson("I'm back. Let's continue.")
        .unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert_eq!(bench.sessions(), 1);
    let last = bench.logged("message").pop().unwrap();
    let state = last["state"].as_str().unwrap();
    assert!(state.contains("Goal: TEACH me"), "{state}");
    assert!(
        state.contains("Open task: What is b in x² − 5x + 6 = 0?"),
        "{state}"
    );
    manager.shut_down();
}

#[test]
fn handing_in_a_paper_opens_a_plugin_conversation_whatever_was_picked() {
    let bench = Bench::new(None, &[]);
    let (manager, _events) = bench.manager();
    let work = bench.root.join("work.png");
    fs::write(&work, b"png").unwrap();
    manager
        .paper_start(
            &Paper {
                target: "midterm-1".into(),
                name: "Diagnostic".into(),
                path: "Study/midterm-1/diagnostic.pdf".into(),
                minutes: Some(50),
                sat: false,
            },
            50,
        )
        .unwrap();
    let words = manager
        .paper_end(&[work])
        .unwrap()
        .expect("words to mark it");
    assert!(
        words.1.starts_with("/magnesium:papers Mark my Diagnostic"),
        "{words:?}"
    );
    let opened = manager.open_session(true).unwrap();
    assert_eq!(opened.engine, Engine::Plugin);
    // The next lesson goes back to the engine the learner picked.
    let opened = manager.open_session(true).unwrap();
    assert_eq!(opened.engine, Engine::Direct);
    manager.shut_down();
}

/// Every event the runtime holds, learner and tutor, and every answer.
fn record_size(bench: &Bench) -> (i64, usize) {
    bench.size()
}

#[test]
fn a_cold_answer_is_kept_cold_and_one_the_runtime_refuses_keeps_its_mark() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("b = -5 COLD").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["cold"], true, "{attempts:#?}");

    // A new process does not know the skill was answered today, so it asks
    // for cold again; the runtime refuses, and the mark is kept without it.
    manager.stop(None).unwrap();
    drop(manager);
    let (manager, mut events) = bench.manager();
    assert!(manager.open_session(true).unwrap().resumed);
    manager.send("b = -5 COLD").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    let second = attempts
        .iter()
        .find(|a| a["question"] == "Solve x² − 5x + 6 = 0 with the formula.")
        .unwrap();
    assert_eq!(second["mark"], "right");
    assert_eq!(second["cold"], false);
    manager.shut_down();
}

#[test]
fn after_recording_off_nothing_reaches_the_record_until_recording_on_in_any_conversation() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("b = 5").unwrap();
    done(&manager, &mut events);
    // The message that asks is captured, as in a plugin lesson; its reply
    // and everything after it are not.
    manager.send("RECORDING OFF please").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let off = record_size(&bench);
    let binding: bool = bench
        .store()
        .query_row(
            "SELECT min(recording_enabled) FROM host_bindings",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!binding, "the runtime turned recording off");
    events.seen.clear();
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert!(
        !events
            .seen
            .iter()
            .any(|e| matches!(e, AgentEvent::Notice { .. })),
        "no 'no mark' notice while recording is off: {:#?}",
        events.seen
    );
    assert_eq!(record_size(&bench), off, "no words, answer or reply kept");
    let state = bench.logged("message").pop().unwrap()["state"].clone();
    assert!(
        state.as_str().unwrap().contains("Recording is off"),
        "{state}"
    );

    // A new conversation, even one the window opens, keeps it off.
    manager.stop(None).unwrap();
    drop(manager);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager
        .send_new_lesson("I'm back. Let's continue.")
        .unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert_eq!(
        record_size(&bench),
        off,
        "nothing kept in a new conversation"
    );

    // Until the learner turns it on.
    manager.send("RECORDING ON please").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let (events_now, attempts_now) = record_size(&bench);
    assert!(events_now > off.0, "the words are kept again");
    let _ = attempts_now;
    manager.shut_down();
}

#[test]
fn a_paused_lesson_records_nothing_until_the_window_opens_it_again() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("PAUSE for now").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let paused = record_size(&bench);
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert_eq!(record_size(&bench), paused);
    let state = bench.logged("message").pop().unwrap()["state"].clone();
    assert!(
        state.as_str().unwrap().contains("paused the lesson"),
        "{state}"
    );
    // Continue from the course page resumes the lesson in the record.
    manager.open_session(true).unwrap();
    manager
        .send_new_lesson("I'm back. Let's continue.")
        .unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert!(record_size(&bench).0 > paused.0);
    assert_eq!(bench.sessions(), 1);
    manager.shut_down();
}

fn notices(events: &Events) -> Vec<String> {
    events
        .seen
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_interrupt_s_watchdog_never_stops_the_next_reply() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager.send("SLOW b = 5").unwrap();
    events.until("the first words", |e| {
        matches!(e, AgentEvent::TextDelta { .. })
    });
    manager.interrupt().unwrap();
    done(&manager, &mut events);
    // At once, a reply that takes longer than the watchdog's eight seconds.
    events.seen.clear();
    manager.send("VERYSLOW b = 5").unwrap();
    let end = events.until("the slow reply's end", |e| {
        matches!(e, AgentEvent::TurnEnded { .. } | AgentEvent::Exited { .. })
    });
    assert!(ended_well(&end), "{end:?}");
    let (reply, _) = events.take();
    assert!(
        reply.ends_with("so that it takes a while to finish."),
        "{reply}"
    );
    manager.shut_down();
}

#[test]
fn an_answer_whose_words_were_not_captured_says_it_was_not_marked() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    fs::write(bench.flags.join("fail-prompt-hook"), "").unwrap();
    events.seen.clear();
    manager.send("b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert!(bench.attempts().is_empty());
    // The notice comes with the record, just after the turn's end.
    let notice = events.until("a notice", |e| matches!(e, AgentEvent::Notice { .. }));
    assert!(format!("{notice:?}").contains("not marked"), "{notice:?}");
    manager.shut_down();
}

#[test]
fn an_answer_after_an_unreadable_record_is_never_marked_on_the_old_task() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    // This reply may have posed a new task; its record cannot be read.
    manager.send("NEVERBLOCK b = 5").unwrap();
    done(&manager, &mut events);
    events.seen.clear();
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert!(bench.attempts().is_empty(), "{:#?}", bench.attempts());
    let notice = events.until("a notice", |e| matches!(e, AgentEvent::Notice { .. }));
    assert!(format!("{notice:?}").contains("not marked"), "{notice:?}");
    manager.shut_down();
}

#[test]
fn a_hung_tool_server_is_replaced_and_a_save_whose_outcome_was_unknown_is_kept_once() {
    let bench = Bench::new(None, &[]);
    // The engine's tool server starts behind the proxy, which holds back
    // the answer to the first save of an answer.
    fs::write(bench.flags.join("hang"), "").unwrap();
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    events.seen.clear();
    let sent = Instant::now();
    manager.send("b = 5").unwrap();
    assert!(ended_well(&events.turn_end()));
    // The reply is not held back by the save.
    assert!(
        sent.elapsed() < Duration::from_secs(2),
        "{:?}",
        sent.elapsed()
    );
    assert!(manager.wait_saved(Duration::from_secs(30)));
    // The save timed out, its server was replaced, and the same request,
    // sent again, was answered with its first result: one answer, no notice.
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 1, "{attempts:#?}");
    assert!(!bench.flags.join("hang").exists(), "the hang happened");
    assert!(notices(&events).is_empty(), "{:#?}", events.seen);
    // The next turn goes on as usual.
    manager.send("b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    assert_eq!(bench.attempts().len(), 2);
    manager.shut_down();
}

#[test]
fn a_restarted_or_cut_short_stream_ends_as_the_finished_message() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    events.take();
    // The message starts twice in one turn.
    manager.send("RESTART b = 5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let (reply, _) = events.take();
    assert_eq!(
        reply,
        "Not quite: b keeps its minus sign, so b = −5. Try this one: What is b in x² + 3x − 4 = 0?"
    );
    // The stream stops short; the finished message has the rest and the block.
    manager.send("CUTSHORT b = -5").unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let (reply, _) = events.take();
    assert!(
        reply.starts_with("Right: the sign goes with the number."),
        "{reply}"
    );
    assert!(!reply.contains("<record"));
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 2, "both blocks were found: {attempts:#?}");
    let kept = bench.deliveries();
    assert_eq!(kept.last().unwrap(), &reply);
    manager.shut_down();
}

/// On a machine under load each tool call is slow. A message sent while the
/// last reply's record is being kept waits for it, and so reads the task
/// that record left open; ending the lesson waits for it too.
#[test]
fn a_slow_record_holds_the_next_message_and_the_end_of_the_lesson() {
    let bench = Bench::new(None, &[]);
    fs::write(bench.flags.join("slow"), "1.0").unwrap();
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    // A wrong answer, and at once, before its record is kept, the next one.
    manager.send("b = 5").unwrap();
    events.turn_end();
    manager.send("b = -5").unwrap();
    done(&manager, &mut events);
    let attempts = bench.attempts();
    assert_eq!(attempts.len(), 2, "{attempts:#?}");
    let wrong = attempts.iter().find(|a| a["mark"] == "wrong").unwrap();
    let right = attempts.iter().find(|a| a["mark"] == "right").unwrap();
    assert_eq!(wrong["question"], "What is b in x² − 5x + 6 = 0?");
    // The task the first record opened.
    assert_eq!(right["question"], "What is b in x² + 3x − 4 = 0?");
    assert_ne!(wrong["response_event_id"], right["response_event_id"]);
    // "I don't know", and the learner ends the lesson as the reply ends.
    manager.send("I don't know").unwrap();
    events.turn_end();
    manager.stop(None).unwrap();
    let attempts = bench.attempts();
    assert_eq!(
        attempts.len(),
        3,
        "the last answer kept before the end: {attempts:#?}"
    );
    assert!(attempts.iter().any(|a| a["mark"] == "declined"));
    manager.shut_down();
}

/// A notice that comes after the turn's end still reaches the page.
#[test]
fn a_notice_after_the_turns_end_reaches_the_page() {
    let bench = Bench::new(None, &[]);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    events.seen.clear();
    manager.send("NEVERBLOCK b = 5").unwrap();
    let end = events.turn_end();
    assert!(ended_well(&end));
    assert!(manager.wait_saved(Duration::from_secs(30)));
    // Everything the page was sent, after the end of the turn included.
    while let Ok(event) = events.receiver.try_recv() {
        events.seen.push(event);
    }
    let after_end: Vec<&AgentEvent> = events
        .seen
        .iter()
        .skip_while(|e| !matches!(e, AgentEvent::TurnEnded { .. }))
        .collect();
    assert!(
        after_end
            .iter()
            .any(|e| matches!(e, AgentEvent::Notice { text, .. } if text.contains("not marked"))),
        "{:#?}",
        events.seen
    );
    manager.shut_down();
}

/// Write a memory as a plugin lesson's tutor writes one, through the
/// runtime's own tool call, in a conversation of its own.
fn plugin_memory(bench: &Bench, state_text: &str) -> String {
    use std::io::Write;
    let call = |request: Value| -> Value {
        let mut child = Command::new(&bench.runtime)
            .current_dir(&bench.root)
            .env("CLAUDE_CODE_SESSION_ID", "plugin-lesson")
            .args([
                "adapter",
                "call",
                "learning_step",
                "--protocol-version",
                "2",
                "--json-input",
                "-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(value.get("error").is_none(), "{value}");
        value
    };
    let started = call(
        serde_json::json!({"schema": "learner-model/v2", "action": "update", "request_id": "plugin-start",
        "session": {"operation": "start", "client_ref": "lesson", "goal": "quadratics"},
        "reported_events": [{"external_event_key": "plugin-1", "speaker": "learner", "kind": "message", "text": "b is 5"}]}),
    );
    let event = started["captured_event_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let made = call(
        serde_json::json!({"schema": "learner-model/v2", "action": "update", "request_id": "plugin-memory",
        "observations": [{"client_ref": "w", "kind": "learner_work", "description": "Drops the sign of b.",
            "events": [{"event_id": event, "role": "learner_response"}]}],
        "memory_edits": [{"operation": "create", "client_ref": "memory",
            "record": {"title": "sign of b", "scope": "Quadratics", "summary": "Their summary.",
                "state_text": state_text, "current_basis": [{"new_observation": "w"}]},
            "reason": "Record work.", "change_basis": [{"new_observation": "w"}]}]}),
    );
    made["references"]["memory"].as_str().unwrap().to_owned()
}

fn memory_text(bench: &Bench, memory_id: &str) -> (String, i64) {
    bench
        .store()
        .query_row(
            "SELECT state_text, revision FROM learner_memories WHERE memory_id=?1",
            [memory_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn a_memory_edit_keeps_the_plugins_lines_and_rewrites_its_own_in_place() {
    let bench = Bench::new(None, &[]);
    // A memory the plugin wrote: an anchor, and a long description.
    let theirs = format!(
        "Reads coefficients in standard form.\nanchor: reads a from a bare x² (check question after the formula)\n{}",
        "Earlier work on signs. ".repeat(500)
    );
    let memory = plugin_memory(&bench, &theirs);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    let first = bench.logged("message").pop().unwrap();
    assert!(
        first["state"]
            .as_str()
            .unwrap()
            .contains("- m1: sign of b: Their summary."),
        "{first}"
    );
    // The tutor revises m1: the plugin's text stays byte for byte.
    manager
        .send("MEMORY found Drops the minus sign of b.")
        .unwrap();
    done(&manager, &mut events);
    let (text, revision) = memory_text(&bench, &memory);
    assert_eq!(revision, 2, "revised");
    assert!(
        text.starts_with(&theirs),
        "the plugin's lines are kept byte for byte"
    );
    assert_eq!(
        &text[theirs.len()..],
        "\nMisunderstanding found in a lesson: Drops the minus sign of b."
    );
    // A second edit of the same misunderstanding rewrites that line.
    manager.send("MEMORY found Writes b = 5 for −5x.").unwrap();
    done(&manager, &mut events);
    let (text, _) = memory_text(&bench, &memory);
    assert!(text.starts_with(&theirs));
    assert_eq!(
        text.matches("in a lesson:").count(),
        1,
        "one line, rewritten"
    );
    assert!(text.ends_with("Misunderstanding found in a lesson: Writes b = 5 for −5x."));
    // Cleared: said in that line, with no date.
    manager
        .send("MEMORY cleared Keeps the sign with its number.")
        .unwrap();
    done(&manager, &mut events);
    let (text, _) = memory_text(&bench, &memory);
    assert!(text.starts_with(&theirs));
    assert!(
        text.ends_with("\nMisunderstanding cleared in a lesson: Keeps the sign with its number.")
    );
    assert!(!text.contains("2026-"), "no date line");
    let memories: i64 = bench
        .store()
        .query_row("SELECT count(*) FROM learner_memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(memories, 1, "no copy was made");
    manager.shut_down();
}

#[test]
fn a_memory_at_the_runtimes_limit_is_left_as_it_was() {
    let bench = Bench::new(None, &[]);
    let theirs = format!(
        "anchor: reads a from a bare x²\n{}",
        "x".repeat(mg_core::record::STATE_LIMIT - 40)
    );
    let memory = plugin_memory(&bench, &theirs);
    let (manager, mut events) = bench.manager();
    manager.open_session(false).unwrap();
    manager.send_new_lesson("TEACH me").unwrap();
    done(&manager, &mut events);
    manager
        .send("MEMORY found Drops the minus sign of b.")
        .unwrap();
    assert!(ended_well(&done(&manager, &mut events)));
    let (text, revision) = memory_text(&bench, &memory);
    assert_eq!((text.as_str(), revision), (theirs.as_str(), 1), "unchanged");
    manager.shut_down();
}
