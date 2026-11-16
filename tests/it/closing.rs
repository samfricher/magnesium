//! A sitting ends with at most two lines from the record: the passes earned
//! today, and when each skill answered today can count next. Claude Code in
//! a terminal shows them once, from the Stop hook, under the reply that
//! ended the sitting.
use crate::support::{TestEnvironment, run_hook_with_host_flag, v2};
use serde_json::{Value, json};

const RIGHT: &str = "roots of a factored quadratic";
const WRONG: &str = "sign of b in the quadratic formula";

/// The learner's message, and the record's event for it once a lesson is on.
fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    let notice = v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
    notice
        .split_once("current event ")
        .map(|(_, rest)| rest.split(['.', ' ']).next().unwrap_or_default().to_owned())
        .unwrap_or_default()
}

/// What the Stop hook printed for a turn.
fn stop(e: &TestEnvironment, prompt_id: &str) -> Value {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"Stop","prompt_id":prompt_id,"stop_hook_active":false,"last_assistant_message":"Good place to stop."});
    let output = run_hook_with_host_flag(e, &e.workspace, "stop", input, "claude");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn step(e: &TestEnvironment, request: Value) -> Value {
    v2::tool(e, "claude", "conv", "learning_step", request)
}

fn answer(e: &TestEnvironment, id: &str, event: &str, label: &str, first_try: &str) {
    let cold = first_try == "right";
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
                "attempt":{"label":label,"question":"Solve x² − 7x + 10 = 0","first_try":first_try,"help":"none","cold":cold}}],
            "event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    );
}

fn closing_json(e: &TestEnvironment) -> Value {
    let output = e.command().args(["closing", "--json"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_sitting_the_learner_ends_shows_its_two_lines_once_under_the_reply() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me the quadratic formula");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "x is 2 or 5");
    answer(&e, "right", &event, RIGHT, "right");
    let event = prompt(&e, "p-3", "b is 7");
    answer(&e, "wrong", &event, WRONG, "wrong");

    // Before the end, every turn's Stop hook says nothing to the learner.
    assert_eq!(stop(&e, "p-3"), json!({}));
    let lines = json!([
        format!("Earned today: a pass on {RIGHT}."),
        // Both names do not fit on one line: the first is cut at a word.
        "Can count from tomorrow: roots of a factored… and 1 more.".to_owned(),
    ]);
    let read = closing_json(&e);
    assert_eq!(read["closing"]["lines"], lines, "{read}");
    assert!(read["sitting"]["ended_ms"].is_null(), "{read}");

    // The learner stops; the tutor ends the sitting before its reply.
    prompt(&e, "p-4", "that's enough for today");
    let ended = step(
        &e,
        json!({"schema":"learner-model/v2","action":"end_sitting"}),
    );
    assert_eq!(ended["sitting_ended"], true, "{ended}");
    assert_eq!(ended["closing_shown"], true, "{ended}");
    // The tutor is told the learner sees the lines, not what they say.
    let note = ended["note"].as_str().unwrap();
    assert!(note.contains("Set no task"), "{note}");
    assert!(!note.contains(RIGHT), "{note}");

    let shown = stop(&e, "p-4");
    assert_eq!(
        shown["systemMessage"],
        lines
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
        "{shown}"
    );
    // Once: the next turn's Stop hook is quiet again.
    prompt(&e, "p-5", "one more thing");
    assert_eq!(stop(&e, "p-5"), json!({}));
    assert!(closing_json(&e)["sitting"]["ended_ms"].is_i64());
    // The text form prints the same lines.
    let text = e.command().arg("closing").output().unwrap();
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        format!(
            "{}\n{}\n",
            lines[0].as_str().unwrap(),
            lines[1].as_str().unwrap()
        )
    );
}

#[test]
fn a_sitting_with_only_a_decline_ends_with_nothing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "quiz me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "I'd rather not");
    answer(&e, "decline", &event, WRONG, "declined");
    assert!(closing_json(&e)["closing"].is_null());
    prompt(&e, "p-3", "bye for now");
    let ended = step(
        &e,
        json!({"schema":"learner-model/v2","action":"end_sitting"}),
    );
    assert_eq!(ended["sitting_ended"], true, "{ended}");
    assert_eq!(ended["closing_shown"], false, "{ended}");
    assert_eq!(stop(&e, "p-3"), json!({}));
}

#[test]
fn a_stop_the_learner_asks_for_ends_the_sitting_too() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "b is 7");
    answer(&e, "wrong", &event, WRONG, "wrong");
    let event = prompt(&e, "p-3", "stop learning");
    let stopped = step(
        &e,
        json!({"schema":"learner-model/v2","action":"control","request_id":"stop-1","control":"stop","turn_ref":event}),
    );
    assert_eq!(stopped["durable"], true, "{stopped}");
    assert_eq!(
        stop(&e, "p-3")["systemMessage"],
        format!("Can count from tomorrow: {WRONG}.")
    );
}
