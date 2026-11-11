//! A sitting opens with what is due: quick checks and paper redos, which the
//! runtime schedules from recorded attempts.
use crate::support::{TestEnvironment, run_hook_with_host_flag, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const LABEL: &str = "sign of b in the quadratic formula";
const MISS: &str = "Took b as 7 in 2x² − 7x + 3, dropping its sign.";
const HOUR_MS: i64 = 60 * 60 * 1000;

fn claude_prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ))
}

fn claude_stop(e: &TestEnvironment, prompt_id: &str, reply: &str) {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"Stop","prompt_id":prompt_id,"stop_hook_active":false,"last_assistant_message":reply});
    let output = run_hook_with_host_flag(e, &e.workspace, "stop", input, "claude");
    assert!(output.status.success(), "{output:?}");
}

fn current_event(notice: &str) -> String {
    notice
        .split_once("current event ")
        .unwrap()
        .1
        .split(['.', ' '])
        .next()
        .unwrap()
        .to_owned()
}

fn step(e: &TestEnvironment, request: Value) -> Value {
    v2::tool(e, "claude", "conv", "learning_step", request)
}

fn attempt(
    e: &TestEnvironment,
    id: &str,
    event: &str,
    first_try: &str,
    description: &str,
) -> Value {
    let kind = if first_try == "declined" {
        "self_report"
    } else {
        "learner_work"
    };
    step(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":id,
            "observations":[{"client_ref":"work","kind":kind,"description":description,
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempt":{"label":LABEL,"question":"Solve x² − 7x + 10 = 0","first_try":first_try,"help":"none","cold":false}}],
            "event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    )
}

/// A context call from a caller that asks for the quick checks as data, as
/// the study window's direct engine does.
fn tool_with_checks(e: &TestEnvironment, request: Value) -> Value {
    use std::io::Write;
    let mut child = e
        .command()
        .env("CLAUDE_CODE_SESSION_ID", "conv")
        .env("MAGNESIUM_SITTING_CHECKS", "1")
        .args([
            "adapter",
            "call",
            "context",
            "--protocol-version",
            "2",
            "--json-input",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn sittings(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/sittings.sqlite")).unwrap()
}

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

/// What a later day looks like to the runtime: every recorded answer and
/// change moves a day and two hours back, so a check made today is due, and
/// the last learner message was more than three hours ago.
fn next_day(e: &TestEnvironment) {
    let back = 26 * HOUR_MS;
    let conn = store(e);
    conn.execute(
        "UPDATE attempts SET answered_at_ms=answered_at_ms-?1",
        [back],
    )
    .unwrap();
    conn.execute("UPDATE practice_events SET at_ms=at_ms-?1", [back])
        .unwrap();
    sittings(e)
        .execute(
            "UPDATE meta SET last_learner_ms=last_learner_ms-?1",
            [4 * HOUR_MS],
        )
        .unwrap();
}

/// The quick checks in the learner store.
fn checks(e: &TestEnvironment) -> i64 {
    store(e)
        .query_row(
            "SELECT count(*) FROM practice_items WHERE origin='chat'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn chrono_today() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// A plan with one lost mark in a gap log from the template before #13,
/// which has a Cold-redo date column and no Next redo.
fn old_paper_plan(e: &TestEnvironment) {
    let dir = e.workspace.join("Study/midterm-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plan.md"),
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("gap-log.md"),
        "# Gap log: midterm-1\n\n## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Cold-redo date | Status |\n|---|---|---|---|---|---|---|---|---|---|\n| 1 | 2026-10-20 | Diagnostic | 3 | km | slope read as intercept | K | taught the slope | | open |\n",
    )
    .unwrap();
}

#[test]
fn a_sitting_opens_with_its_quick_check_and_redo_and_a_decline_lets_the_lesson_go_on() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    old_paper_plan(&e);

    // Day 0: a lesson, and a wrong first try with a label.
    claude_prompt(&e, "p-1", "teach me the quadratic formula");
    v2::start(&e, "claude", "conv");
    let notice = claude_prompt(&e, "p-2", "b is 7");
    assert!(!notice.contains("Sitting quick checks"), "{notice}");
    let missed = attempt(&e, "miss", &current_event(&notice), "wrong", MISS);
    assert_eq!(missed["update_status"], "committed", "{missed}");
    assert_eq!(
        missed["quick_checks"]["open_labels"],
        json!([LABEL]),
        "{missed}"
    );
    let check = &missed["practice"]["items"][0];
    assert_eq!(check["item"], LABEL, "{missed}");
    assert_eq!(check["status"], "open", "{missed}");
    assert!(
        check["next_redo"].as_str().unwrap() > chrono_today().as_str(),
        "a miss is due the next day, not today: {missed}"
    );

    // A later day: the first learner message begins a sitting.
    next_day(&e);
    let notice = claude_prompt(&e, "p-3", "hi, let's carry on");
    assert!(
        notice.contains(" Sitting quick checks: due 1, posed 0."),
        "{notice}"
    );
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());

    // The first tool result carries the check's text and names the redo, once.
    let status = v2::tool(
        &e,
        "claude",
        "conv",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    let opener = status["sitting_opener"].as_str().unwrap();
    for text in [
        &format!("1. Label \"{LABEL}\". Earlier misses: {MISS}"),
        "Your first reply of this sitting begins with the words \"Quick check 1:\", with no greeting or recap before them",
        "the question never names or points at the skill, the label, the step or the earlier mistake",
        "lettered options",
        "first_try declined",
        "Study/midterm-1/gap-log.md: 1 due, first Diagnostic Q3.",
        "load stage papers_loop",
        "a skip, no-quiz or stop request wins",
    ] {
        assert!(opener.contains(text), "{text}\n{opener}");
    }
    // A tutor's result is as it was: the opener's text, and nothing as data.
    assert!(status.get("sitting_checks").is_none(), "{status}");
    assert!(status.get("sitting_redos").is_none(), "{status}");
    let again = v2::tool(
        &e,
        "claude",
        "conv",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert!(again.get("sitting_opener").is_none(), "{again}");
    assert!(again.get("sitting_checks").is_none(), "{again}");

    // The older gap log was brought in, so its row could fall due, and the
    // runtime wrote its table.
    let gap_log = std::fs::read_to_string(e.workspace.join("Study/midterm-1/gap-log.md")).unwrap();
    assert!(
        gap_log.contains("| Repair done | Passes | Next redo | Status |"),
        "{gap_log}"
    );
    assert!(
        gap_log.contains(&format!(
            "| 1 | 2026-10-20 | Diagnostic | 3 | km | slope read as intercept | K |  | taught the slope |  | {} | open |",
            chrono_today()
        )),
        "{gap_log}"
    );
    let backups = std::fs::read_dir(e.workspace.join(".magnesium/backups/gap-logs"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);

    // The tutor poses it; the learner declines. Both are recorded, and the
    // decline cites the learner's own message.
    claude_stop(
        &e,
        "p-3",
        "Welcome back. Quick check 1: in 3x² − 5x + 1 = 0, what is b?",
    );
    let notice = claude_prompt(&e, "p-4", "I'd rather skip that and keep going");
    assert!(!notice.contains("Sitting quick checks"), "{notice}");
    let event = current_event(&notice);
    let declined = attempt(
        &e,
        "decline",
        &event,
        "declined",
        "Declined the quick check.",
    );
    assert_eq!(declined["update_status"], "committed", "{declined}");
    let (outcome, posed): (String, bool) = sittings(&e)
        .query_row("SELECT outcome,posed FROM sitting_items", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((outcome.as_str(), posed), ("declined", true));
    let role: String = Connection::open(e.workspace.join(".magnesium/state.sqlite"))
        .unwrap()
        .query_row(
            "SELECT role FROM observation_events WHERE event_id=?1",
            [&event],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(role, "learner_response");

    // The lesson goes on, and a decline is free: the check stays due.
    let notice = claude_prompt(&e, "p-5", "what's next?");
    assert!(notice.contains("learning true"), "{notice}");
    let load = v2::tool(
        &e,
        "claude",
        "conv",
        "context",
        json!({"action":"instructions","host":"claude","stage":"teach_and_practice"}),
    );
    assert_eq!(load["status"], "served", "{load}");
    assert_eq!(
        declined["practice"]["items"][0]["status"], "open",
        "{declined}"
    );
    assert_eq!(
        declined["practice"]["attempts"][0]["items"][0]["effect"], "declined",
        "{declined}"
    );

    // One line per sitting in the local log.
    let log = std::fs::read_to_string(e.workspace.join(".magnesium/sittings.log")).unwrap();
    let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    assert_eq!(last["due"], json!([LABEL]));
    assert_eq!(last["posed"], json!([LABEL]));
    assert_eq!(last["declined"], json!([LABEL]));
    assert_eq!(last["passed"], json!([]));
    assert_eq!(last["due_not_taken"], 0);
    assert_eq!(last["paper_redos_due"][0]["first"], "Diagnostic Q3");
}

#[test]
fn a_right_unaided_answer_to_a_due_check_passes_it_when_the_stop_hook_carries_no_reply_text() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    claude_prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let notice = claude_prompt(&e, "p-2", "b is 7");
    attempt(&e, "miss", &current_event(&notice), "wrong", MISS);
    next_day(&e);
    // A new sitting begins with the learner's next message. The Stop hook of
    // that turn carries no reply text, so nothing says the check was posed.
    claude_prompt(&e, "p-3", "hello again");
    let no_reply = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"Stop","prompt_id":"p-3","stop_hook_active":false});
    let output = run_hook_with_host_flag(&e, &e.workspace, "stop", no_reply, "claude");
    assert!(output.status.success(), "{output:?}");
    let conn = sittings(&e);
    let sittings_count: i64 = conn
        .query_row("SELECT count(*) FROM sittings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sittings_count, 2);
    // The first tool result of the sitting carries its opener, once. A
    // caller that runs the lesson itself asks for the checks as data too.
    let load = tool_with_checks(
        &e,
        json!({"action":"instructions","host":"claude","stage":"teach_and_practice"}),
    );
    assert!(
        load["sitting_opener"].as_str().unwrap().contains(LABEL),
        "{load}"
    );
    let checks = load["sitting_checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1, "{load}");
    assert_eq!(checks[0]["number"], 1);
    assert_eq!(checks[0]["label"], LABEL);
    assert_eq!(checks[0]["misses"], json!([MISS]));
    assert_eq!(load["sitting_redos"], json!([]), "{load}");
    // The learner answers anyway; a right unaided answer is recorded and passes it.
    let notice = claude_prompt(&e, "p-4", "x is 2 or 5");
    let passed = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"pass",
            "observations":[{"client_ref":"work","kind":"learner_work","description":"Answer to b.",
                "events":[{"event_id":current_event(&notice),"role":"learner_response"}],
                "attempt":{"label":LABEL,"question":"Solve x² − 7x + 10 = 0","first_try":"right","help":"none","cold":true}}]}),
    );
    assert!(passed.get("sitting_opener").is_none(), "{passed}");
    let (outcome,): (String,) = conn
        .query_row("SELECT outcome FROM sitting_items", [], |r| {
            Ok((r.get(0)?,))
        })
        .unwrap();
    assert_eq!(outcome, "passed");
    assert_eq!(
        passed["practice"]["items"][0]["status"], "1 of 3",
        "{passed}"
    );
}

#[test]
fn a_caller_that_runs_the_lesson_gets_the_due_paper_redo_as_data_and_its_answer_passes_the_row() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    old_paper_plan(&e);
    // The first sitting's opener goes to the lesson's start, which brings
    // the older gap log in. A later sitting opens with the row still due.
    claude_prompt(&e, "p-0", "teach me enzyme kinetics");
    v2::start(&e, "claude", "conv");
    next_day(&e);
    claude_prompt(&e, "p-1", "I'm back");
    // The first tool result of the sitting, for a caller that asks for data.
    let status = tool_with_checks(&e, json!({"schema":"learner-model/v2","action":"status"}));
    assert_eq!(status["sitting_checks"], json!([]), "{status}");
    assert_eq!(
        status["sitting_redos"],
        json!([{"item": "midterm-1#1", "plan": "midterm-1", "tag": "km", "type": "K",
            "what_went_wrong": "slope read as intercept", "paper": "Diagnostic", "paper_question": "3",
            "question": null, "key": null}]),
        "{status}"
    );
    // A cold, unaided, right answer that names the row passes it, and its
    // next question is kept for the next redo.
    let notice = claude_prompt(&e, "p-2", "Km is 2 mM");
    let passed = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"redo",
            "observations":[{"client_ref":"work","kind":"learner_work","description":"Read Km off the plot.",
                "events":[{"event_id":current_event(&notice),"role":"learner_response"}],
                "attempt":{"label":"km","question":"The plot crosses the x axis at −0.5: what is Km?",
                    "first_try":"right","help":"none","cold":true,"counts_for":["midterm-1#1"],
                    "next":{"question":"A second plot crosses at −0.25: what is Km?","answer_key":"4 mM"}}}]}),
    );
    assert_eq!(passed["update_status"], "committed", "{passed}");
    assert_eq!(
        passed["practice"]["attempts"][0]["items"][0]["effect"], "passed",
        "{passed}"
    );
    assert_eq!(
        passed["practice"]["items"][0]["status"], "1 of 3",
        "{passed}"
    );
    assert_eq!(checks(&e), 0, "a redo makes no quick check");
    let stored: String = store(&e)
        .query_row("SELECT next_question FROM practice_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, "A second plot crosses at −0.25: what is Km?");
}

#[test]
fn an_attempt_needs_the_learners_own_message_and_a_bounded_label() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", "cx");
    let captured = v2::tool(
        &e,
        "claude",
        "cx",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"tutor",
            "reported_events":[{"external_event_key":"tutor-1","speaker":"tutor","kind":"message","text":"b is the coefficient of x"}]}),
    );
    let tutor_event = captured["captured_event_ids"][0].as_str().unwrap();
    for (index, (label, role, path)) in [
        (LABEL, "teaching_context", "observations[0].events"),
        ("", "teaching_context", "observations[0].attempt.label"),
        (
            "Q2: 2x² − 3x − 2 = 0",
            "teaching_context",
            "observations[0].attempt.label",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let output = v2::tool_output(
            &e,
            "claude",
            "cx",
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":format!("bad-{index}"),
                "observations":[{"client_ref":"work","kind":"teaching_response","description":"Explained b.",
                    "events":[{"event_id":tutor_event,"role":role}],
                    "attempt":{"label":label,"question":"Solve x² − 7x + 10 = 0","first_try":"wrong","help":"none","cold":false}}]}),
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["recovery"]["field_path"], path, "{value}");
    }
    assert_eq!(checks(&e), 0);
}

#[test]
fn one_command_turns_the_opener_off_or_adds_a_block_on_new_material() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let switch = |state: &str| {
        let output = e.command().args(["opener", state]).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    claude_prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let notice = claude_prompt(&e, "p-2", "b is 7");
    attempt(&e, "miss", &current_event(&notice), "wrong", MISS);
    let load = |stage: &str| {
        v2::tool_output(
            &e,
            "claude",
            "conv",
            "context",
            json!({"action":"instructions","host":"claude","stage":stage}),
        )
    };
    let load = |stage: &str| -> Value { serde_json::from_slice(&load(stage).stdout).unwrap() };

    assert!(switch("off").starts_with("Opener off"));
    next_day(&e);
    let notice = claude_prompt(&e, "p-3", "back again");
    assert!(!notice.contains("Sitting quick checks"), "{notice}");
    assert!(load("teach_and_practice").get("sitting_opener").is_none());

    assert!(switch("block").starts_with("Opener on, with a block"));
    next_day(&e);
    let notice = claude_prompt(&e, "p-4", "and again");
    assert!(notice.contains("due 1, posed 0"), "{notice}");
    let refused = load("teach_and_practice");
    assert_eq!(refused["error"]["code"], "due_checks_first", "{refused}");
    assert!(refused["sitting_opener"].as_str().unwrap().contains(LABEL));
    // The opener came with the refusal, and review stays open.
    assert_eq!(load("review_and_transfer")["status"], "served");
    let refused = load("orient_and_probe");
    assert_eq!(refused["error"]["code"], "due_checks_first");
    assert!(
        refused["sitting_opener"].as_str().unwrap().contains(LABEL),
        "restated while blocked"
    );
    claude_stop(&e, "p-4", "Quick check 1: what is b in x² + 4x − 1?");
    assert_eq!(load("teach_and_practice")["status"], "served");

    assert!(switch("on").starts_with("Opener on: each sitting"));
    let config = std::fs::read_to_string(e.workspace.join(".magnesium/config.toml")).unwrap();
    assert!(!config.contains("opener"), "{config}");
    let elsewhere = e.root.join("not-a-study-folder");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let refused = e
        .command_in(&elsewhere)
        .args(["opener", "off"])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "{refused:?}");
}

#[test]
fn loading_a_paper_stage_brings_in_an_older_gap_log_even_with_the_opener_off() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    old_paper_plan(&e);
    let output = e.command().args(["opener", "off"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let path = e.workspace.join("Study/midterm-1/gap-log.md");
    let before = std::fs::read_to_string(&path).unwrap();
    let load = v2::tool(
        &e,
        "claude",
        "cx",
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(load["status"], "served", "{load}");
    assert!(load.get("sitting_opener").is_none(), "{load}");
    assert_eq!(
        load["gap_logs"]["imported"],
        json!(["Study/midterm-1/gap-log.md"]),
        "{load}"
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert_ne!(before, after);
    assert!(after.contains("| Passes | Next redo | Status |"), "{after}");
    let digest = e.command().arg("digest").output().unwrap();
    let digest: Value = serde_json::from_slice(&digest.stdout).unwrap();
    assert_eq!(digest["plans"][0]["redos_due"], 1, "{digest}");
    assert_eq!(digest["plans"][0]["unreadable"], json!([]), "{digest}");
}

#[test]
fn served_instructions_ask_for_an_attempt_and_open_each_sitting_with_what_is_due() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["claude"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "served",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let review = served("review_and_transfer");
        for text in [
            "with an attempt carrying the mark of every first try at a task you set, \"I don't know\" (declined, not wrong) and a right answer after a hint included (a guess gets none)",
            "Most answers are ordinary: send only the observation and its attempt.",
            "Each sitting opens with what is due: the runtime's sitting opener lists the quick checks and paper redos.",
            "A decline is free and is recorded as the learner's; a no-quiz preference, refusal, skip or pause wins.",
        ] {
            assert!(review.contains(text), "{host}: {text}");
        }
        assert!(!review.contains("An overdue item is a suggestion, not an obligation."));
    }
}
