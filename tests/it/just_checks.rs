//! "Just the checks": a sitting that poses the quick checks due and nothing
//! else, then ends with the closing lines and no task. The prompt hook takes
//! the ask from the plugin's command, and the tutor's `just_checks` from any
//! other words; both make the same sitting.
use crate::support::{TestEnvironment, clock, run_hook_with_host_flag, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const HOUR_MS: i64 = 60 * 60 * 1000;
const LABELS: [&str; 3] = [
    "sign of b in the quadratic formula",
    "units of a rate",
    "slope read as intercept",
];

fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    prompt_in(e, "conv", prompt_id, text)
}

fn prompt_in(e: &TestEnvironment, conversation: &str, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":conversation,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ))
}

fn event_of(notice: &str) -> String {
    notice
        .split_once("current event ")
        .unwrap()
        .1
        .split(['.', ' '])
        .next()
        .unwrap()
        .to_owned()
}

fn stop(e: &TestEnvironment, prompt_id: &str, reply: &str) -> Value {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"Stop","prompt_id":prompt_id,"stop_hook_active":false,"last_assistant_message":reply});
    let output = run_hook_with_host_flag(e, &e.workspace, "stop", input, "claude");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn step(e: &TestEnvironment, request: Value) -> Value {
    v2::tool(e, "claude", "conv", "learning_step", request)
}

fn answer(e: &TestEnvironment, id: &str, event: &str, label: &str, first_try: &str) -> Value {
    let kind = if first_try == "declined" {
        "self_report"
    } else {
        "learner_work"
    };
    step(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":id,
            "observations":[{"client_ref":"work","kind":kind,"description":"An answer.",
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempt":{"label":label,"question":"A new problem","first_try":first_try,"help":"none","cold":first_try == "right"}}],
            "event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    )
}

fn load(e: &TestEnvironment, stage: &str) -> Value {
    load_in(e, "conv", stage)
}

fn load_in(e: &TestEnvironment, conversation: &str, stage: &str) -> Value {
    let output = v2::tool_output(
        e,
        "claude",
        conversation,
        "context",
        json!({"action":"instructions","host":"claude","stage":stage}),
    );
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Day 0: three wrong first tries make three quick checks. Then a later day,
/// with the last learner message more than three hours back.
fn three_checks_due(e: &TestEnvironment) {
    v2::init(e);
    prompt(e, "p-0", "teach me");
    v2::start(e, "claude", "conv");
    for (n, label) in LABELS.iter().enumerate() {
        let notice = prompt(e, &format!("p-0{n}"), "a wrong answer");
        let missed = answer(e, &format!("miss-{n}"), &event_of(&notice), label, "wrong");
        assert_eq!(missed["update_status"], "committed", "{missed}");
    }
    clock::days_pass(&e.workspace, 1);
    Connection::open(e.workspace.join(".magnesium/sittings.sqlite"))
        .unwrap()
        .execute(
            "UPDATE meta SET last_learner_ms=last_learner_ms-?1",
            [4 * HOUR_MS],
        )
        .unwrap();
}

#[test]
fn the_plugin_command_poses_the_due_checks_alone_and_the_last_answer_ends_the_sitting() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);

    // The command begins the sitting, and its notice says what it is.
    let notice = prompt(&e, "p-1", "/magnesium:checks");
    assert!(
        notice.contains(" Sitting of quick checks alone: 3 left."),
        "{notice}"
    );
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());

    // The first tool result carries the checks alone: no paper redo, no
    // new material, and how the sitting ends.
    let status = v2::tool(
        &e,
        "claude",
        "conv",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    let opener = status["sitting_opener"].as_str().unwrap();
    for text in [
        "Quick checks alone.",
        "no paper redo, no new material, no plan and no other task",
        &format!("1. Label \"{}\".", LABELS[0]),
        "Your next reply begins with the words \"Quick check 1:\"",
        "Magnesium ends the sitting and shows the learner",
    ] {
        assert!(opener.contains(text), "{text}\n{opener}");
    }

    // Marking and closing are part of the checks: they leave it as it is.
    for stage in ["interpret_and_repair", "review_and_transfer"] {
        let loaded = load(&e, stage);
        assert!(loaded.get("error").is_none(), "{loaded}");
        assert!(loaded.get("sitting_note").is_none(), "{loaded}");
    }

    // Right, wrong, declined: the last one ends the sitting, and the result
    // says so before the reply is written.
    stop(&e, "p-1", "Quick check 1: what is b in 3x² − 5x + 1 = 0?");
    let event = event_of(&prompt(&e, "p-2", "-5"));
    let right = answer(&e, "right", &event, LABELS[0], "right");
    assert!(right.get("sitting_ended").is_none(), "{right}");
    assert_eq!(stop(&e, "p-2", "Right. Quick check 2: ..."), json!({}));
    let event = event_of(&prompt(&e, "p-3", "metres"));
    let wrong = answer(&e, "wrong", &event, LABELS[1], "wrong");
    assert!(wrong.get("sitting_ended").is_none(), "{wrong}");
    stop(&e, "p-3", "Not quite. Quick check 3: ...");
    let event = event_of(&prompt(&e, "p-4", "skip this one"));
    let declined = answer(&e, "decline", &event, LABELS[2], "declined");
    assert_eq!(declined["sitting_ended"], true, "{declined}");
    let note = declined["sitting_note"].as_str().unwrap();
    assert!(note.contains("set no task"), "{note}");
    assert!(
        !note.contains(LABELS[0]),
        "the tutor is not given the lines: {note}"
    );

    // The closing lines show under that reply, from the record.
    let shown = stop(&e, "p-4", "That's all three. Well done for showing up.");
    let lines = shown["systemMessage"].as_str().unwrap();
    assert!(lines.starts_with("Earned today: a pass on "), "{lines}");
    // The same lines `magnesium closing` works out, which the window draws.
    let output = e.command().args(["closing", "--json"]).output().unwrap();
    let read: Value = serde_json::from_slice(&output.stdout).unwrap();
    let words: Vec<&str> = read["closing"]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(words.len(), 2, "{read}");
    assert_eq!(lines, words.join("\n"));
    assert!(read["sitting"]["ended_ms"].is_i64(), "{read}");

    // Asking for more afterwards is an ordinary lesson again.
    prompt(&e, "p-5", "actually, teach me the slope");
    assert!(load(&e, "teach_and_practice").get("error").is_none());
}

#[test]
fn other_words_reach_the_same_sitting_through_the_tutor() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);
    let notice = prompt(&e, "p-1", "can we only do my quick checks today?");
    assert!(!notice.contains("quick checks alone"), "{notice}");
    let asked = step(
        &e,
        json!({"schema":"learner-model/v2","action":"just_checks"}),
    );
    assert_eq!(asked["checks"], 3, "{asked}");
    let opener = asked["sitting_opener"].as_str().unwrap();
    assert!(opener.starts_with("Quick checks alone."), "{opener}");
    // A lesson is active here, so the note needs no start.
    assert_eq!(asked["note"], "Pose the checks as sitting_opener says.");
    // Asked twice, it is the same sitting with the same checks.
    let again = step(
        &e,
        json!({"schema":"learner-model/v2","action":"just_checks"}),
    );
    assert_eq!(again["checks"], 3, "{again}");
    assert_eq!(again["sitting_opener"], asked["sitting_opener"]);
    // In a conversation with no lesson active, an answer could not be
    // recorded: asking starts one, and the next answer is captured in it.
    let elsewhere = v2::tool(
        &e,
        "claude",
        "conv-2",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"just_checks"}),
    );
    assert_eq!(elsewhere["lesson_started"], "Quick checks", "{elsewhere}");
    assert_eq!(elsewhere["note"], "Pose the checks as sitting_opener says.");
    let input = json!({"session_id":"conv-2","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":"q-1","prompt":"-5"});
    let notice = v2::context(&run_hook_with_host_flag(
        &e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
    assert!(notice.contains("learning true"), "{notice}");
    assert!(!event_of(&notice).starts_with("unavailable"), "{notice}");
}

#[test]
fn with_no_check_due_the_tutor_says_when_the_next_is() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-0", "teach me");
    v2::start(&e, "claude", "conv");
    let notice = prompt(&e, "p-1", "b is 7");
    answer(&e, "miss", &event_of(&notice), LABELS[0], "wrong");
    // The check made today is due tomorrow, not now.
    let notice = prompt(&e, "p-2", "just the checks");
    assert!(!notice.contains("quick checks alone"), "{notice}");
    let asked = step(
        &e,
        json!({"schema":"learner-model/v2","action":"just_checks"}),
    );
    assert_eq!(asked["checks"], 0, "{asked}");
    assert!(asked["next_due"].is_string(), "{asked}");
    assert!(
        asked["note"]
            .as_str()
            .unwrap()
            .starts_with("No quick check is due now. The next is due "),
        "{asked}"
    );
    assert!(asked.get("sitting_opener").is_none(), "{asked}");
    // Nothing is held back.
    assert!(load(&e, "teach_and_practice").get("error").is_none());
}

/// The rows that make a sitting checks alone, for any conversation.
fn alone_rows(e: &TestEnvironment) -> i64 {
    Connection::open(e.workspace.join(".magnesium/sittings.sqlite"))
        .unwrap()
        .query_row("SELECT count(*) FROM checks_alone", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn asking_for_a_lesson_part_way_makes_it_an_ordinary_sitting() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);
    prompt(&e, "p-1", "/magnesium:checks");
    let event = event_of(&prompt(&e, "p-2", "-5"));
    answer(&e, "right", &event, LABELS[0], "right");
    stop(&e, "p-2", "Right. Quick check 2: what are its units?");
    // The learner asks for a lesson; the tutor loads its stage, which is
    // never refused.
    prompt(&e, "p-3", "actually, can you teach me the slope first?");
    let loaded = load(&e, "teach_and_practice");
    assert!(loaded.get("error").is_none(), "{loaded}");
    assert!(
        loaded["sitting_note"]
            .as_str()
            .unwrap()
            .contains("ordinary sitting"),
        "{loaded}"
    );
    assert_eq!(alone_rows(&e), 0);
    // The check posed before the request is no longer the open task.
    let lesson = e
        .command()
        .args(["lesson", "--json", "--conversation", "conv"])
        .output()
        .unwrap();
    let lesson: Value = serde_json::from_slice(&lesson.stdout).unwrap();
    assert_ne!(
        lesson["lesson"]["open_task"]["kind"], "quick_check",
        "{lesson}"
    );
    let notice = prompt(&e, "p-4", "ok, what next?");
    assert!(!notice.contains("quick checks alone"), "{notice}");
    // The checks not yet answered stay due, and answering them later does
    // not end the sitting under the lesson.
    let event = event_of(&prompt(&e, "p-5", "metres"));
    let wrong = answer(&e, "wrong", &event, LABELS[1], "wrong");
    assert!(wrong.get("sitting_ended").is_none(), "{wrong}");
    let event = event_of(&prompt(&e, "p-6", "skip"));
    let declined = answer(&e, "decline", &event, LABELS[2], "declined");
    assert!(declined.get("sitting_ended").is_none(), "{declined}");
}

#[test]
fn a_second_conversation_in_the_folder_goes_on_as_in_any_sitting() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);
    let notice = prompt(&e, "p-1", "/magnesium:checks");
    assert!(notice.contains("quick checks alone: 3 left"), "{notice}");
    // Another tab, mid-lesson, in the same folder and sitting.
    let notice = prompt_in(&e, "conv-2", "q-1", "teach me the slope");
    assert!(!notice.contains("quick checks alone"), "{notice}");
    let loaded = load_in(&e, "conv-2", "teach_and_practice");
    assert!(loaded.get("error").is_none(), "{loaded}");
    assert!(loaded.get("sitting_note").is_none(), "{loaded}");
    // The checks are still the first conversation's.
    assert_eq!(alone_rows(&e), 1);
    let notice = prompt(&e, "p-2", "-5");
    assert!(notice.contains("quick checks alone: 3 left"), "{notice}");
}

#[test]
fn recording_off_part_way_makes_it_an_ordinary_sitting() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);
    prompt(&e, "p-1", "/magnesium:checks");
    let event = event_of(&prompt(&e, "p-2", "turn recording off"));
    let off = step(
        &e,
        json!({"schema":"learner-model/v2","action":"control","request_id":"off-1","control":"recording_off","turn_ref":event}),
    );
    assert_eq!(off["durable"], true, "{off}");
    // No answer here could be recorded, so nothing waits on the checks.
    assert_eq!(alone_rows(&e), 0);
    let loaded = load(&e, "teach_and_practice");
    assert!(loaded.get("error").is_none(), "{loaded}");
}

/// A learning_step from a caller that runs the lesson itself and asks for
/// the checks as data, as Magnesium's own engine does.
fn step_with_checks(e: &TestEnvironment, conversation: &str, request: Value) -> Value {
    use std::io::Write;
    let mut child = e
        .command()
        .env("CLAUDE_CODE_SESSION_ID", conversation)
        .env("MAGNESIUM_SITTING_CHECKS", "1")
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

#[test]
fn magnesiums_engine_gets_the_checks_after_the_opener_went_elsewhere() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    three_checks_due(&e);
    // An ordinary lesson in one tab begins the sitting and has its opener.
    prompt(&e, "p-1", "hi, let's carry on");
    let status = v2::tool(
        &e,
        "claude",
        "conv",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert!(status["sitting_opener"].is_string(), "{status}");
    // Then the control, in a new tab run by Magnesium's own engine.
    prompt_in(&e, "direct", "d-1", "Just the quick checks.");
    let asked = step_with_checks(
        &e,
        "direct",
        json!({"schema":"learner-model/v2","action":"just_checks"}),
    );
    assert_eq!(asked["checks"], 3, "{asked}");
    assert_eq!(
        asked["sitting_checks"].as_array().map(Vec::len),
        Some(3),
        "{asked}"
    );
    assert_eq!(asked["sitting_redos"], json!([]), "{asked}");
}
