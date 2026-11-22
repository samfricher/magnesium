#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{Days, Local, NaiveDate};
use serde_json::{json, Value};

use mg_core::study;
use mg_core::Store;

const THREAD: &str = "window-readers";
const GAP_LOG: &str = "Study/midterm-1/gap-log.md";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

fn runtime_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("MAGNESIUM_TEST_BINARY").filter(|v| !v.is_empty()) {
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
}

struct Scratch {
    _dir: tempfile::TempDir,
    root: PathBuf,
    folder: PathBuf,
    binary: PathBuf,
}

impl Scratch {
    fn new(binary: PathBuf) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for name in ["home", "data", "config", "tmp", "window", "Course"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let folder = root.join("Course");
        Self {
            _dir: dir,
            root,
            folder,
            binary,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.env_clear();
        for (name, value) in self.environment() {
            command.env(name, value);
        }
        command.current_dir(&self.folder);
        command
    }

    fn environment(&self) -> Vec<(String, String)> {
        let path = |name: &str| self.root.join(name).to_string_lossy().into_owned();
        let mut vars = vec![
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), path("home")),
            ("XDG_DATA_HOME".to_owned(), path("data")),
            ("XDG_CONFIG_HOME".to_owned(), path("config")),
            ("TMPDIR".to_owned(), path("tmp")),
            ("CLAUDE_CODE_SESSION_ID".to_owned(), THREAD.to_owned()),
        ];
        if let Ok(zone) = std::env::var("TZ") {
            vars.push(("TZ".to_owned(), zone));
        }
        vars
    }

    fn wrapper(&self) -> PathBuf {
        let wrapper = self.root.join("magnesium");
        let mut script = String::from("#!/bin/sh\nexec env -i");
        for (name, value) in self.environment() {
            script.push_str(&format!(" {name}='{value}'"));
        }
        script.push_str(&format!(" '{}' \"$@\"\n", self.binary.display()));
        std::fs::write(&wrapper, script).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        wrapper
    }

    fn run(&self, args: &[&str], input: Option<&Value>) -> Value {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the runtime starts");
        let mut stdin = child.stdin.take().unwrap();
        if let Some(input) = input {
            stdin.write_all(input.to_string().as_bytes()).unwrap();
        }
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "magnesium {args:?}: {output:?}");
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("magnesium {args:?} printed no JSON ({e}): {output:?}"))
    }

    fn tool(&self, name: &str, request: Value) -> Value {
        let value = self.run(
            &[
                "adapter",
                "call",
                name,
                "--protocol-version",
                "2",
                "--json-input",
                "-",
            ],
            Some(&request),
        );
        assert!(value.get("error").is_none(), "{name}: {value}");
        value
    }

    fn step(&self, request: Value) -> Value {
        let value = self.tool("learning_step", request);
        assert_eq!(value["update_status"], "committed", "{value}");
        value
    }

    fn prompt(&self, id: &str, text: &str) -> String {
        let input = json!({"session_id": THREAD, "cwd": self.folder, "hook_event_name": "UserPromptSubmit",
            "prompt_id": id, "prompt": text});
        let output = self.run(
            &[
                "_internal",
                "hook",
                "user-prompt-submit",
                "--plugin-version",
                "0.2.0",
                "--adapter-protocol-version",
                "2",
                "--host-kind",
                "claude",
            ],
            Some(&input),
        );
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_default();
        context
            .split_once("current event ")
            .and_then(|(_, rest)| rest.split(['.', ' ']).next())
            .unwrap_or_else(|| panic!("the hook named no event: {output}"))
            .to_owned()
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.folder.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.folder.join(relative)).unwrap()
    }
}

fn day(today: NaiveDate, offset: i64) -> String {
    let shifted = if offset >= 0 {
        today.checked_add_days(Days::new(offset as u64))
    } else {
        today.checked_sub_days(Days::new(offset.unsigned_abs()))
    };
    shifted.unwrap().to_string()
}

fn written_rows(gap_log: &str) -> Vec<(String, String)> {
    let start = gap_log.find("## Gaps").expect("a Gaps section");
    let end = gap_log[start + 3..]
        .find("\n## ")
        .map_or(gap_log.len(), |at| start + 3 + at);
    let mut lines = gap_log[start..end]
        .lines()
        .filter(|line| line.starts_with('|'));
    let header: Vec<String> = cells(lines.next().expect("a header"));
    let tag = header
        .iter()
        .position(|c| c == "Tag")
        .expect("a Tag column");
    let status = header
        .iter()
        .position(|c| c == "Status")
        .expect("a Status column");
    lines
        .map(cells)
        .filter(|row| {
            !row.iter()
                .all(|c| c.chars().all(|ch| matches!(ch, '-' | ':')))
        })
        .map(|row| (row[tag].clone(), row[status].clone()))
        .collect()
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

fn passes_of(status: &str) -> usize {
    match status {
        "closed" => 3,
        "2 of 3" => 2,
        "1 of 3" => 1,
        "open" => 0,
        other => panic!("the runtime wrote a status the window does not know: {other:?}"),
    }
}

fn counted(dots: &[u8; 3]) -> usize {
    dots.iter().filter(|dot| **dot == 2).count()
}

#[test]
fn the_readers_follow_what_the_runtime_in_this_checkout_writes() {
    let scratch = Scratch::new(runtime_binary());
    let today = Local::now().date_naive();
    let d = |offset| day(today, offset);

    scratch.run(&["init", "--json"], None);

    scratch.write(
        "Study/midterm-1/plan.md",
        &format!(
            "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | {} |\n| Length | 50 minutes |\n",
            d(14)
        ),
    );
    scratch.write(
        GAP_LOG,
        &format!(
            "# Gap log: midterm-1\n\n## Gaps\n\n\
             | # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n\
             |---|---|---|---|---|---|---|---|---|---|---|---|\n\
             | 1 | {d21} | Diagnostic | 1 | km | read Km as the intercept | K | S | taught | {d20} pass, {d17} pass, {d10} pass |  | closed |\n\
             | 2 | {d21} | Diagnostic | 2 | vmax | took the slope for Vmax | K | U | repaired | {d4} pass | {d1} | 1 of 3 |\n\
             | 3 | {d21} | Diagnostic | 3 | slope | lost the sign of the slope | N | G | repaired | {d9} pass, {d6} pass, {d2} fail | {n1} | open |\n\n\
             ## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
             | Diagnostic | {d21} | 17/30 | 48/50 min | Conf right: S 4/6, U 2/3 |\n",
            d21 = d(-21), d20 = d(-20), d17 = d(-17), d10 = d(-10), d9 = d(-9), d6 = d(-6),
            d4 = d(-4), d2 = d(-2), d1 = d(-1), n1 = d(1),
        ),
    );

    let loaded = scratch.tool(
        "context",
        json!({"action": "instructions", "host": "claude", "stage": "papers_loop"}),
    );
    assert_eq!(loaded["gap_logs"]["imported"], json!([GAP_LOG]), "{loaded}");

    let started = scratch.step(json!({"schema": "learner-model/v2", "action": "update", "request_id": "start",
        "session": {"operation": "start", "client_ref": "s", "goal": "Read Vmax from a Lineweaver-Burk plot"}}));
    let session = started["references"]["s"]
        .as_str()
        .expect("a session id")
        .to_owned();
    scratch.step(json!({"schema": "learner-model/v2", "action": "update", "request_id": "steps",
        "session": {"operation": "use", "session_id": session, "expected_revision": 1},
        "checkpoint": {"current_problem": "Vmax from the intercept", "working_diagnosis": "Reads the slope for Vmax",
            "next_action": "Ask for Vmax from intercept 0.25", "pending_task": null,
            "goal_steps": [
                {"step": "Find the y-intercept", "status": "done"},
                {"step": "Take its reciprocal", "status": "doing"},
                {"step": "Check the units", "status": "todo"}]}}));
    let event = scratch.prompt("p-1", "Vmax is 0.25");
    let answered = scratch.step(json!({"schema": "learner-model/v2", "action": "update", "request_id": "answers",
        "observations": [
            {"client_ref": "wrong", "kind": "learner_work", "description": "Took the intercept itself as Vmax.",
             "events": [{"event_id": event, "role": "learner_response"}],
             "attempt": {"label": "reciprocal of the intercept", "question": "Intercept 0.25: find Vmax",
                 "first_try": "wrong", "help": "none", "cold": true}},
            {"client_ref": "helped", "kind": "learner_work", "description": "Gave the units after a hint.",
             "events": [{"event_id": event, "role": "learner_response"}],
             "attempt": {"label": "units of a rate", "question": "Give the units of Vmax",
                 "first_try": "right", "help": "hint", "cold": false}}]}));
    assert_eq!(
        answered["practice"]["items"][0]["status"], "open",
        "a quick check was made: {answered}"
    );

    std::env::set_var(mg_core::runtime::PROGRAM_ENV, scratch.wrapper());
    let store = Store::at(scratch.root.join("window"));
    let digest = scratch.run(&["digest"], None);
    let plan = &digest["plans"][0];
    assert_eq!(plan["gap_log_from"], "record", "{plan}");

    let written = written_rows(&scratch.read(GAP_LOG));
    assert_eq!(written.len(), 3, "{written:?}");
    let progress = study::progress(&scratch.folder, None);
    assert_eq!(progress.message, None, "{progress:#?}");
    let counts = progress.today.as_ref().expect("today's counts");
    assert_eq!(counts.day, study::today().to_string(), "{counts:?}");
    assert!(mg_core::runtime::fits_window(&scratch.wrapper()));
    let exam = progress.exam.as_ref().expect("the plan's exam");
    assert_eq!(
        (exam.target.as_str(), exam.days_left),
        ("midterm-1", Some(14))
    );

    let names = |rows: &[study::Row]| {
        rows.iter()
            .map(|row| row.name.clone())
            .collect::<BTreeSet<_>>()
    };
    let closed: BTreeSet<String> = written
        .iter()
        .filter(|(_, s)| s == "closed")
        .map(|(t, _)| t.clone())
        .collect();
    let open: BTreeSet<String> = written
        .iter()
        .filter(|(_, s)| s != "closed")
        .map(|(t, _)| t.clone())
        .collect();
    assert_eq!(
        names(&progress.confirmed),
        closed,
        "confirmed must be exactly the closed rows"
    );
    let chat = BTreeSet::from([
        "reciprocal of the intercept".to_owned(),
        "units of a rate".to_owned(),
    ]);
    assert_eq!(
        names(&progress.in_progress),
        open.union(&chat).cloned().collect::<BTreeSet<_>>(),
        "{progress:#?}"
    );
    assert_eq!(
        Some(open.len() as u64),
        plan["unfixed_gaps"].as_u64(),
        "the record agrees: {plan}"
    );
    for row in progress.confirmed.iter().chain(&progress.in_progress) {
        let Some((_, status)) = written.iter().find(|(tag, _)| *tag == row.name) else {
            assert!(chat.contains(&row.name), "a row from nowhere: {row:?}");
            assert_eq!(counted(&row.dots), 0, "{row:?}");
            continue;
        };
        assert_eq!(
            counted(&row.dots),
            passes_of(status),
            "{}: {status} drawn as {:?}",
            row.name,
            row.dots
        );
    }
    assert!(
        progress.untested.is_empty(),
        "an item never answered is listed on the course map, not as a row: {progress:#?}"
    );
    let score = progress.scores.first().expect("the Diagnostic's score");
    assert_eq!(
        (score.score.as_str(), score.sure_right, score.sure_total),
        ("17/30", Some(4), Some(6))
    );

    assert!(!progress.checks_unknown);
    let check = progress
        .in_progress
        .iter()
        .find(|row| row.name == "reciprocal of the intercept")
        .expect("the quick check");
    assert!(!check.due, "a check made today is due tomorrow: {check:?}");
    assert_eq!(check.next_day, Some(d(1)));

    assert_eq!(progress.helped_count, 1, "{progress:#?}");
    assert_eq!(progress.helped, vec!["units of a rate".to_owned()]);

    let marks = mg_core::runtime::marks(&scratch.folder).expect("the runtime has attempts");
    assert_eq!(marks.len(), 2, "{marks:#?}");
    let wrong = marks
        .iter()
        .find(|m| m.label == "reciprocal of the intercept")
        .unwrap();
    assert_eq!(
        (
            wrong.mark.as_str(),
            wrong.help.as_str(),
            wrong.cold,
            wrong.paper.as_deref()
        ),
        ("wrong", "none", true, None)
    );
    let helped = marks.iter().find(|m| m.label == "units of a rate").unwrap();
    assert_eq!(
        (helped.mark.as_str(), helped.help.as_str(), helped.cold),
        ("right", "hint", false)
    );
    assert_ne!(wrong.id, helped.id);
    assert!(!wrong.id.is_empty() && wrong.at.starts_with(&today.format("%Y-%m-%d").to_string()));

    let earned: usize = written.iter().map(|(_, status)| passes_of(status)).sum();
    assert!(!progress.level_unknown);
    assert!(progress.proofs.levelled);
    assert_eq!(progress.proofs.earned as usize, earned, "{progress:#?}");
    assert_eq!(progress.proofs.skills as usize, written.len());
    assert_eq!(progress.proofs.proven as usize, closed.len());
    assert_eq!(progress.proofs.total, progress.proofs.skills * 3);
    assert_eq!(
        progress.proofs.level as usize,
        1 + earned * 9 / (progress.proofs.total as usize)
    );
    assert_eq!(progress.proofs.level, 5);

    assert_eq!(
        study::answered_days(&[scratch.folder.to_string_lossy().into_owned()]),
        Some(BTreeSet::from([today]))
    );
    assert!(progress.confirmed.iter().all(|row| !row.due));

    let card = study::card(&scratch.folder, &store);
    assert_eq!(card.proofs, progress.proofs, "{card:#?}");
    assert_eq!(card.due, 1);
    assert_eq!(
        card.exam.as_ref().map(|exam| exam.target.as_str()),
        Some("midterm-1")
    );

    let home = study::home(&scratch.folder, &store);
    assert_eq!(
        home.checks_due,
        Some(0),
        "the runtime counts the checks, and the one made today is due tomorrow"
    );
    assert_eq!(home.redos_due, 1);
    assert_eq!(home.first_redo.as_deref(), Some("Diagnostic 2"));
    let last = home.last.as_ref().expect("the last lesson");
    assert_eq!(last.goal, "Read Vmax from a Lineweaver-Burk plot");
    assert_eq!(
        last.next_action.as_deref(),
        Some("Ask for Vmax from intercept 0.25")
    );

    let rail = study::rail(&scratch.folder);
    assert_eq!(
        rail.goal.as_deref(),
        Some("Read Vmax from a Lineweaver-Burk plot")
    );
    let steps: Vec<(String, String)> = rail
        .steps
        .iter()
        .map(|s| (s.step.clone(), s.status.clone()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("Find the y-intercept".to_owned(), "done".to_owned()),
            ("Take its reciprocal".to_owned(), "doing".to_owned()),
            ("Check the units".to_owned(), "todo".to_owned()),
        ]
    );
    let lesson = mg_core::runtime::lesson(&scratch.folder, Some(THREAD))
        .expect("the runtime says")
        .expect("the conversation has a lesson");
    assert_eq!(lesson.goal, "Read Vmax from a Lineweaver-Burk plot");
    assert_eq!(lesson.steps, rail.steps);
    assert_eq!(lesson.open_task, None);
    assert_eq!(
        mg_core::runtime::lesson(&scratch.folder, Some("another-conversation")),
        Some(None)
    );

    let text = scratch.read(GAP_LOG);
    let vmax = text
        .lines()
        .find(|line| line.contains("| vmax |"))
        .unwrap()
        .to_owned();
    let edited = vmax.replace(&format!("| {} | 1 of 3 |", d(-1)), "|  | closed |");
    assert_ne!(edited, vmax, "the row was edited");
    scratch.write(GAP_LOG, &text.replace(&vmax, &edited));
    let progress = study::progress(&scratch.folder, None);
    assert_eq!(
        names(&progress.confirmed),
        closed,
        "a hand-closed row was shown as confirmed: {progress:#?}"
    );
    assert_eq!(progress.message, None);

    let reloaded = scratch.tool(
        "context",
        json!({"action": "instructions", "host": "claude", "stage": "papers"}),
    );
    assert_eq!(
        reloaded["gap_logs"]["edited"][0]["file"], GAP_LOG,
        "{reloaded}"
    );
    let progress = study::progress(&scratch.folder, None);
    assert_eq!(progress.message, None);
    assert_eq!(names(&progress.confirmed), closed);

    for (id, question, cold) in [
        ("p-2", "Units of Km", true),
        ("p-3", "Units of Vmax", false),
    ] {
        let event = scratch.prompt(id, "per second");
        scratch.step(json!({"schema": "learner-model/v2", "action": "update", "request_id": id,
            "observations": [{"client_ref": "alone", "kind": "learner_work", "description": "Gave the units alone.",
                "events": [{"event_id": event, "role": "learner_response"}],
                "attempt": {"label": "units of a concentration", "question": question,
                    "first_try": "right", "help": "none", "cold": cold}}]}));
    }
    let mut marks = mg_core::runtime::marks(&scratch.folder).expect("the runtime has attempts");
    mg_core::runtime::note_passes(&mut marks, &scratch.folder);
    let alone: Vec<Option<u8>> = marks
        .iter()
        .filter(|m| m.label == "units of a concentration")
        .map(|m| m.pass)
        .collect();
    assert_eq!(alone, [Some(1), None], "{marks:#?}");
    let tomorrow = chrono::Local::now()
        .date_naive()
        .succ_opt()
        .unwrap()
        .to_string();
    let first = marks
        .iter()
        .find(|m| m.label == "units of a concentration" && m.pass == Some(1))
        .expect("the pass");
    assert_eq!(
        first.next_day.as_deref(),
        Some(tomorrow.as_str()),
        "{marks:#?}"
    );
    assert!(marks
        .iter()
        .filter(|m| m.pass.is_none())
        .all(|m| m.next_day.is_none()));
    assert!(marks
        .iter()
        .filter(|m| m.label != "units of a concentration")
        .all(|m| m.pass.is_none()));
    let progress = study::progress(&scratch.folder, None);
    let row = progress
        .in_progress
        .iter()
        .find(|row| row.name == "units of a concentration")
        .expect("the skill's row");
    assert_eq!(row.dots, [2, 0, 0]);
    assert_eq!(
        row.last_answer.as_deref(),
        marks.last().map(|m| m.id.as_str())
    );
}
