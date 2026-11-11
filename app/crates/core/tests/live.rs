//! The pipe against the real Claude Code. It is ignored by default, because
//! it starts a model on your own subscription and takes a few minutes.
//!
//!   scripts/live-check.sh
//!
//! It answers the one question the stand-in cannot: does a lesson started by
//! the window load Magnesium's plugin, hooks and tool server as a terminal
//! session does, and leave the same kind of record?
//!
//! It works in a new scratch study folder (or the one named by
//! MAGNESIUM_LIVE_FOLDER) and prints the folder at the end so you can look.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mg_core::agent::{AgentEvent, Answer, Decision};
use mg_core::claude::ClaudeCode;
use mg_core::{runtime, Manager, Store};

struct Live {
    manager: Manager,
    events: Receiver<AgentEvent>,
}

/// What one turn gave back.
#[derive(Default, Debug)]
struct TurnReport {
    text: String,
    deltas: usize,
    permissions: Vec<String>,
    questions: usize,
    notices: Vec<String>,
    problems: Option<Vec<String>>,
    ok: bool,
    interrupted: bool,
    error: Option<String>,
    /// Milliseconds from the start of the wait to the first piece of the
    /// reply, and to its first finished block.
    first_piece_ms: Option<u128>,
    first_block_ms: Option<u128>,
    /// The lines that named the work while the tutor worked.
    doing: Vec<String>,
}

impl Live {
    fn new(folder: &Path, home: &Path) -> Self {
        let (sender, events) = channel();
        let sender = Mutex::new(sender);
        let manager = Manager::new(
            Arc::new(Store::at(home.to_path_buf())),
            Arc::new(ClaudeCode::new()),
            Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }),
        );
        manager.open_folder(folder).expect("the folder opens");
        Self { manager, events }
    }

    /// Wait for the turn to end. Every permission request is allowed once and
    /// printed, and every question gets its first option, so the turn can
    /// finish with nobody at the keyboard. `interrupt_after` stops the reply
    /// once that many pieces of text have arrived.
    fn turn(&self, limit: Duration, interrupt_after: Option<usize>) -> TurnReport {
        let started = Instant::now();
        let deadline = started + limit;
        let mut report = TurnReport::default();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self.events.recv_timeout(left).unwrap_or_else(|_| {
                panic!("the turn did not end within {limit:?}; so far: {report:#?}")
            });
            match event {
                AgentEvent::Ready { problems, .. } => report.problems = Some(problems),
                AgentEvent::TextDelta { .. } => {
                    report.deltas += 1;
                    report
                        .first_piece_ms
                        .get_or_insert(started.elapsed().as_millis());
                    if interrupt_after == Some(report.deltas) {
                        self.manager.interrupt().expect("the interrupt is sent");
                    }
                }
                AgentEvent::Working { doing } => {
                    if let Some(doing) = doing {
                        if report.doing.last() != Some(&doing) {
                            report.doing.push(doing);
                        }
                    }
                }
                AgentEvent::TextDone { text } => {
                    report
                        .first_block_ms
                        .get_or_insert(started.elapsed().as_millis());
                    if !report.text.is_empty() {
                        report.text.push_str("\n\n");
                    }
                    report.text.push_str(&text);
                }
                AgentEvent::Permission {
                    id, title, detail, ..
                } => {
                    report.permissions.push(format!("{title}: {detail}"));
                    self.manager
                        .decide(&id, Decision::AllowOnce)
                        .expect("the answer is sent");
                }
                AgentEvent::Question { id, questions } => {
                    report.questions += questions.len();
                    let answers: Vec<Answer> = questions
                        .iter()
                        .map(|q| Answer {
                            question: q.question.clone(),
                            chosen: q
                                .options
                                .first()
                                .map(|o| vec![o.label.clone()])
                                .unwrap_or_default(),
                        })
                        .collect();
                    self.manager
                        .answer(&id, &answers)
                        .expect("the answer is sent");
                }
                AgentEvent::Notice { text, .. } => report.notices.push(text),
                AgentEvent::TurnEnded {
                    ok,
                    interrupted,
                    error,
                } => {
                    report.ok = ok;
                    report.interrupted = interrupted;
                    report.error = error;
                    return report;
                }
                AgentEvent::Exited { expected, detail } => {
                    panic!(
                        "the agent ended (expected: {expected}): {detail:?}; so far: {report:#?}"
                    )
                }
                _ => {}
            }
        }
    }
}

impl Live {
    /// Press Stop once that many pieces of the reply have arrived, as the
    /// Stop button does while the tutor writes. Returns what arrived before.
    fn stop_during_reply(&self, limit: Duration, after: usize, line: &str) -> TurnReport {
        let deadline = Instant::now() + limit;
        let mut report = TurnReport::default();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(AgentEvent::TextDelta { .. }) => {
                    report.deltas += 1;
                    if report.deltas == after {
                        self.manager
                            .stop(Some(line))
                            .expect("Stop closes the lesson");
                        return report;
                    }
                }
                Ok(AgentEvent::TurnEnded { ok, .. }) => {
                    panic!(
                        "the reply ended (ok: {ok}) before Stop was pressed; so far: {report:#?}"
                    )
                }
                Ok(_) => {}
                Err(_) => panic!("no reply within {limit:?}; so far: {report:#?}"),
            }
        }
    }
}

fn show(title: &str, report: &TurnReport) {
    println!("\n== {title} ==");
    println!(
        "ended well: {}   interrupted: {}   pieces of text: {}",
        report.ok, report.interrupted, report.deltas
    );
    if let Some(error) = &report.error {
        println!("error: {error}");
    }
    if let (Some(piece), Some(block)) = (report.first_piece_ms, report.first_block_ms) {
        println!("first words after {piece} ms, first finished block after {block} ms");
    }
    if !report.doing.is_empty() {
        println!("working lines: {:?}", report.doing);
    }
    for line in &report.permissions {
        println!("asked permission: {line}");
    }
    if report.questions > 0 {
        println!("asked {} question(s)", report.questions);
    }
    for line in &report.notices {
        println!("notice: {line}");
    }
    let shown: String = report.text.chars().take(700).collect();
    println!("tutor said:\n{shown}");
}

#[test]
#[ignore = "starts Claude Code on your own subscription; run scripts/live-check.sh yourself"]
fn the_real_agent_holds_a_lesson_through_the_pipe() {
    let scratch =
        std::env::temp_dir().join(format!("magnesium-window-live-{}", std::process::id()));
    let folder = std::env::var_os("MAGNESIUM_LIVE_FOLDER")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.join("Live-Check"));
    let home = scratch.join("window");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    println!("study folder: {}", folder.display());
    if let Some(trace) = std::env::var_os(mg_core::claude::TRACE_ENV) {
        println!(
            "every line to and from the agent is kept in: {}",
            PathBuf::from(trace).display()
        );
    }
    println!(
        "runtime: {:?}, version {:?}",
        runtime::program(),
        runtime::version()
    );

    // What `magnesium init` does in any study folder.
    runtime::init(&folder).expect("magnesium sets the folder up");

    let minutes = |n: u64| Duration::from_secs(60 * n);
    let live = Live::new(&folder, &home);
    live.manager
        .open_session(false)
        .expect("Claude Code starts");

    live.manager
        .send_new_lesson(
            "Teach me what a prime number is. One short step, then one task. No plan files.",
        )
        .unwrap();
    let first = live.turn(minutes(10), None);
    show("1. a new lesson", &first);
    let problems = first
        .problems
        .clone()
        .expect("the agent reported what it loaded");
    assert!(
        problems.is_empty(),
        "the plugin or its tool server is missing: {problems:#?}"
    );
    assert!(
        first.ok && !first.text.trim().is_empty(),
        "the first reply is empty or failed"
    );
    assert!(first.deltas > 1, "the reply did not arrive piece by piece");

    live.manager.send("2, 3, 5 and 7").unwrap();
    let second = live.turn(minutes(10), None);
    show("2. the same process, a second turn", &second);
    assert!(second.ok && !second.text.trim().is_empty());

    live.manager
        .send("Explain in about 300 words why there are infinitely many primes.")
        .unwrap();
    let third = live.turn(minutes(10), Some(3));
    show("3. a reply interrupted after its first words", &third);
    assert!(third.interrupted, "the interrupt did not end the reply");

    live.manager
        .send("One more time, in about 300 words: why are there infinitely many primes?")
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    live.manager.interrupt().expect("the interrupt is sent");
    let early = live.turn(minutes(2), None);
    show("3b. a reply interrupted before its first word", &early);
    assert!(
        early.interrupted,
        "an interrupt sent before the first word did not end the turn"
    );

    // Stop pressed while the tutor writes. The reply is interrupted first,
    // then the conversation closes, and nothing more reaches the page.
    live.manager
        .send("In about 400 words, explain how the sieve of Eratosthenes finds primes.")
        .unwrap();
    let stopped = live.stop_during_reply(
        minutes(10),
        3,
        "primes: the proof that there are infinitely many",
    );
    show("3c. Stop pressed during a reply", &stopped);
    // The agent sends text in bursts, so pieces that reached the page before
    // Stop can still wait in this test's queue. Once stop() returns, the
    // window passes nothing more on, so all that waits arrived before Stop.
    let before_stop = live.events.try_iter().count();
    println!("events that reached the page before Stop, still queued: {before_stop}");
    let after_stop = live.events.recv_timeout(Duration::from_secs(15));
    assert!(
        after_stop.is_err(),
        "the stopped conversation still reached the page: {after_stop:?}"
    );
    drop(live);

    // The window, opened again.
    let again = Live::new(&folder, &home);
    let opened = again
        .manager
        .open_session(true)
        .expect("the lesson resumes");
    assert!(
        opened.resumed,
        "the window did not remember the conversation"
    );
    println!("\nearlier turns kept by the window: {}", opened.turns.len());
    again
        .manager
        .send("I'm back. In one line: which four numbers did I give you earlier?")
        .unwrap();
    let fourth = again.turn(minutes(10), None);
    show("4. the same lesson, resumed", &fourth);
    assert!(fourth.ok);
    assert!(
        fourth.text.contains('7') && fourth.text.contains('5'),
        "the resumed conversation did not remember the earlier answer"
    );
    again.manager.stop(None).unwrap();

    // The record a terminal lesson would leave.
    let digest = runtime::digest(&folder).expect("the digest reads");
    let sessions = digest["record"]["sessions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    println!("\n== the record ==");
    println!(
        "{}",
        serde_json::to_string_pretty(&digest["record"]).unwrap()
    );
    match runtime::attempts(&folder) {
        Some(attempts) => println!("attempts recorded: {}", attempts.len()),
        None => println!("attempts: this runtime has no `attempts` command"),
    }
    assert!(
        !sessions.is_empty(),
        "no lesson was recorded, so Magnesium's hooks or tool server did not run as they do in a terminal"
    );
    println!("\nAll five parts passed. Look in: {}", folder.display());
}

/// The tutor's delivered replies, as the runtime recorded them.
fn delivered(folder: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(folder.join(".magnesium/state.sqlite")).unwrap();
    let mut statement = conn
        .prepare("SELECT text FROM learning_events WHERE speaker='tutor' AND kind='delivery' ORDER BY event_no")
        .unwrap();
    statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// Words that would give a tool, a command or a path away.
fn gives_away(line: &str) -> bool {
    let lower = line.to_lowercase();
    [
        "mcp",
        "bash",
        "read(",
        "write(",
        "learning_step",
        "search_sources",
        "latexmk",
        "tectonic",
        "/",
        "\\",
        "--",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

#[test]
#[ignore = "starts Claude Code on your own subscription, for half an hour; run it yourself"]
fn a_window_lesson_streams_its_reply_names_its_work_and_builds_a_paper() {
    let scratch =
        std::env::temp_dir().join(format!("magnesium-window-live-48-{}", std::process::id()));
    let folder = scratch.join("Calculus");
    let home = scratch.join("window");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    println!("study folder: {}", folder.display());
    runtime::init(&folder).expect("magnesium sets the folder up");
    let minutes = |n: u64| Duration::from_secs(60 * n);

    let live = Live::new(&folder, &home);
    live.manager
        .open_session(false)
        .expect("Claude Code starts");
    live.manager
        .send_new_lesson("derivatives with the power rule, starting from where I am")
        .unwrap();
    let first = live.turn(minutes(10), None);
    show("1. a new lesson", &first);
    assert_eq!(
        first.problems.as_deref(),
        Some(&[][..]),
        "what the agent loaded"
    );
    assert!(first.ok && first.deltas > 1, "{first:#?}");

    // A reply of 100 words or more shows its first words before it is whole.
    live.manager
        .send("Before the next question: explain in about 200 words what a derivative measures, then give me one task.")
        .unwrap();
    let long = live.turn(minutes(10), None);
    show("2. a long reply", &long);
    let words = long.text.split_whitespace().count();
    let (piece, block) = (long.first_piece_ms.unwrap(), long.first_block_ms.unwrap());
    assert!(words >= 100, "{words} words");
    assert!(
        piece + 1000 < block,
        "first words at {piece} ms, the whole reply at {block} ms"
    );

    // What the runtime recorded as delivered is what the window showed.
    let kept = delivered(&folder);
    assert!(
        kept.iter().any(|reply| reply.trim() == long.text.trim()),
        "shown: {:?}\nkept: {kept:#?}",
        long.text
    );

    // Answer its task, then close the window in the middle of the lesson.
    live.manager
        .send("I think it's 2x, because the power comes down and drops by one.")
        .unwrap();
    let third = live.turn(minutes(10), None);
    show("3. an answer", &third);
    let steps = mg_core::peek::goal_steps(&folder);
    println!("goal steps: {steps:?}");
    assert!(!steps.is_empty(), "the tutor saved no steps for the goal");
    drop(live);
    let home_now = mg_core::study::home(&folder, &mg_core::Store::at(home.clone()));
    let up_next = home_now
        .last
        .as_ref()
        .and_then(|last| last.next_action.clone());
    println!(
        "Up next after the window closed: {up_next:?}\nthe last reply: {}",
        third.text
    );
    assert!(up_next.is_some(), "the course page has no Up next");

    // A paper, built and marked from the window.
    let papers = Live::new(&folder, &home);
    papers
        .manager
        .open_session(false)
        .expect("Claude Code starts");
    let request = "I have a short quiz on the power rule in two days. Build me one 10-minute practice paper of three questions, with its key, as Markdown. Keep it short and do not ask me questions first.";
    papers
        .manager
        .send_shown(request, &format!("/magnesium:papers {request}"))
        .unwrap();
    let built = papers.turn(minutes(40), None);
    show("4. a paper built", &built);
    assert!(built.ok, "{built:#?}");
    assert!(
        built
            .doing
            .iter()
            .any(|line| line.contains("paper") || line.contains("writing")),
        "no line named the paper being written: {:?}",
        built.doing
    );
    for line in &built.doing {
        assert!(
            !gives_away(line),
            "a working line gives a tool away: {line}"
        );
    }
    let work = folder.join("my-answers.md");
    std::fs::write(&work, "Q1: 3x^2\nQ2: 10x^4 - 2\nQ3: ?\n").unwrap();
    let mark = format!(
        "Mark my practice paper. I used 9 of 10 minutes. My work: {}",
        work.display()
    );
    papers
        .manager
        .send_shown(&mark, &format!("/magnesium:papers {mark}"))
        .unwrap();
    let marked = papers.turn(minutes(30), None);
    show("5. the paper marked", &marked);
    assert!(marked.ok);
    for line in &marked.doing {
        assert!(
            !gives_away(line),
            "a working line gives a tool away: {line}"
        );
    }
    papers.manager.stop(None).unwrap();
    match runtime::attempts(&folder) {
        Some(attempts) => println!("attempts recorded: {}", attempts.len()),
        None => println!("attempts: this runtime has no `attempts` command"),
    }
    println!("\nLook in: {}", folder.display());
}
