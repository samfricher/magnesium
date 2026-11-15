//! A message the host marks as talk is never an attempt (issue 65), and
//! `magnesium lesson` prints the lesson and its open task from the record
//! (issue 66).
use crate::support::{TestEnvironment, run_hook_with_host_flag, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const TALK_LEAD: &str = "(This is not an answer to the task, so please don't mark it.)";
const TASK: &str = "**Task.** Solve x² − 7x + 10 = 0.";

fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ))
}

fn stop(e: &TestEnvironment, prompt_id: &str, reply: &str) {
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

fn step_output(e: &TestEnvironment, request: Value) -> Value {
    serde_json::from_slice(&v2::tool_output(e, "claude", "conv", "learning_step", request).stdout)
        .unwrap()
}

fn answer(id: &str, event: &str) -> Value {
    json!({"schema":"learner-model/v2","action":"update","request_id":id,
        "observations":[{"client_ref":"work","kind":"learner_work","description":"Solved it.",
            "events":[{"event_id":event,"role":"learner_response"}],
            "attempt":{"label":"factorising a quadratic","question":"Solve x² − 7x + 10 = 0","first_try":"right","help":"none","cold":false}}],
        "event_dispositions":[{"event_id":event,"outcome":"processed"}]})
}

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

fn lesson(e: &TestEnvironment, args: &[&str]) -> Value {
    let output = e
        .command()
        .args(["lesson", "--json"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["lesson"].clone()
}

/// A lesson in conversation "conv" whose tutor posed `TASK` and saved it as
/// the checkpoint's pending task, with two goal steps.
fn lesson_with_a_task(e: &TestEnvironment) {
    v2::init(e);
    prompt(e, "p-1", "teach me quadratics");
    let started = v2::start(e, "claude", "conv");
    stop(e, "p-1", &format!("Let's begin.\n\n{TASK}"));
    let tutor: String = store(e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE speaker='tutor' ORDER BY event_no DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(started["update_status"], "committed", "{started}");
    let (session, revision): (String, i64) = store(e)
        .query_row(
            "SELECT session_id,revision FROM learning_sessions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let saved = step(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"task",
            "session":{"operation":"use","session_id":session,"expected_revision":revision},
            "checkpoint":{"current_problem":"Factorising","working_diagnosis":"New","next_action":"Wait for the answer",
                "goal_steps":[{"step":"Find the factors","status":"doing"},{"step":"Read off the roots","status":"todo"}],
                "pending_task":{"proposal_event_ref":tutor,"conditions":"No help"}}}),
    );
    assert_eq!(saved["update_status"], "committed", "{saved}");
}

#[test]
fn talk_is_kept_without_the_windows_line_and_refuses_an_attempt() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    lesson_with_a_task(&e);

    let notice = prompt(
        &e,
        "p-2",
        &format!("{TALK_LEAD}\n\nwhy do we factorise first?"),
    );
    let talk = current_event(&notice);
    let (text, metadata): (String, String) = store(&e)
        .query_row(
            "SELECT text,metadata_json FROM learning_events WHERE event_id=?1",
            [&talk],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(text, "why do we factorise first?");
    assert_eq!(
        serde_json::from_str::<Value>(&metadata).unwrap()["intent"],
        "talk"
    );

    let refused = step_output(&e, answer("on-talk", &talk));
    assert_eq!(
        refused["error"]["recovery"]["field_path"], "observations[0].events[0]",
        "{refused}"
    );
    assert!(refused.to_string().contains("as talk"), "{refused}");
    // The same message as context, with no attempt, is recorded.
    let noted = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"question",
            "observations":[{"client_ref":"q","kind":"learner_question","description":"Asked why factorise.",
                "events":[{"event_id":talk,"role":"learner_response"}]}],
            "event_dispositions":[{"event_id":talk,"outcome":"processed"}]}),
    );
    assert_eq!(noted["update_status"], "committed", "{noted}");
    let attempts: i64 = store(&e)
        .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attempts, 0);
    // The task is still open after talk.
    assert_eq!(
        lesson(&e, &["--conversation", "conv"])["open_task"]["text"],
        format!("Let's begin.\n\n{TASK}")
    );

    // An answer sent as one is marked, and closes the task.
    let notice = prompt(&e, "p-3", "x = 2 or x = 5");
    let marked = step(&e, answer("on-answer", &current_event(&notice)));
    assert_eq!(marked["update_status"], "committed", "{marked}");
    assert!(lesson(&e, &["--conversation", "conv"])["open_task"].is_null());
}

#[test]
fn a_model_reported_event_cannot_claim_to_be_talk() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", "conv");
    let refused = step_output(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"claim",
            "reported_events":[{"external_event_key":"l-1","speaker":"learner","kind":"message","text":"hi","metadata":{"intent":"talk"}}]}),
    );
    assert!(
        refused
            .to_string()
            .contains("metadata.intent is set by the host"),
        "{refused}"
    );
}

#[test]
fn the_lesson_command_prints_goal_steps_and_the_open_task_and_writes_nothing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    assert!(lesson(&e, &[]).is_null(), "no lesson yet");
    lesson_with_a_task(&e);
    let before = std::fs::read(e.workspace.join(".magnesium/state.sqlite")).unwrap();

    let printed = lesson(&e, &["--conversation", "conv"]);
    assert_eq!(printed["goal"], "Learn coefficient roles");
    assert_eq!(printed["status"], "active");
    assert_eq!(printed["learning_on"], true);
    assert_eq!(printed["recording_on"], true);
    assert_eq!(
        printed["steps"],
        json!([{"step":"Find the factors","status":"doing"},{"step":"Read off the roots","status":"todo"}])
    );
    assert_eq!(printed["open_task"]["kind"], "task");
    assert!(
        printed["open_task"]["text"]
            .as_str()
            .unwrap()
            .ends_with(TASK)
    );
    assert!(printed["open_task"]["posed_at_ms"].as_i64().is_some());
    // Without a conversation, the most recently changed lesson.
    assert_eq!(lesson(&e, &[])["session_id"], printed["session_id"]);
    // A conversation the record has no lesson for.
    assert!(lesson(&e, &["--conversation", "other"]).is_null());

    let text = e.command().arg("lesson").output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("[>] Find the factors"), "{text}");
    assert!(text.contains("Open task, posed"), "{text}");
    assert_eq!(
        std::fs::read(e.workspace.join(".magnesium/state.sqlite")).unwrap(),
        before,
        "the command changed the record"
    );
}
