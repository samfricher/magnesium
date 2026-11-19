use crate::support::mcp::McpClient;
use crate::support::{TestEnvironment, run_hook_with_host_flag, v2};
use serde_json::{Value, json};

const RIGHT: &str = "roots of a factored quadratic";
const WRONG: &str = "sign of b in the quadratic formula";

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

    assert_eq!(stop(&e, "p-3"), json!({}));
    let lines = json!([
        format!("Earned today: a pass on {RIGHT}."),
        "Can count from tomorrow: roots of a factored… and 1 more.".to_owned(),
    ]);
    let read = closing_json(&e);
    assert_eq!(read["closing"]["lines"], lines, "{read}");
    assert!(read["sitting"]["ended_ms"].is_null(), "{read}");

    prompt(&e, "p-4", "that's enough for today");
    let ended = step(
        &e,
        json!({"schema":"learner-model/v2","action":"end_sitting"}),
    );
    assert_eq!(ended["sitting_ended"], true, "{ended}");
    assert_eq!(ended["closing_shown"], true, "{ended}");
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
    prompt(&e, "p-5", "one more thing");
    assert_eq!(stop(&e, "p-5"), json!({}));
    assert!(closing_json(&e)["sitting"]["ended_ms"].is_null());
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

fn stop_in(e: &TestEnvironment, conversation: &str, prompt_id: &str) -> Value {
    let input = json!({"session_id":conversation,"cwd":e.workspace,"hook_event_name":"Stop","prompt_id":prompt_id,"stop_hook_active":false,"last_assistant_message":"Fine."});
    let output = run_hook_with_host_flag(e, &e.workspace, "stop", input, "claude");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or(json!({}))
}

fn end(e: &TestEnvironment) -> Value {
    step(
        e,
        json!({"schema":"learner-model/v2","action":"end_sitting"}),
    )
}

#[test]
fn an_end_no_stop_hook_showed_is_set_aside_by_the_next_message() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "x is 2 or 5");
    answer(&e, "right", &event, RIGHT, "right");
    prompt(&e, "p-3", "that's it for today");
    assert_eq!(end(&e)["sitting_ended"], true);
    prompt(&e, "p-4", "actually, one more question");
    assert_eq!(stop(&e, "p-4"), json!({}));
    assert!(closing_json(&e)["sitting"]["ended_ms"].is_null());
}

#[test]
fn a_sitting_ended_twice_shows_its_lines_each_time_and_only_where_it_ended() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "x is 2 or 5");
    answer(&e, "right", &event, RIGHT, "right");
    prompt(&e, "p-3", "done for today");
    end(&e);
    assert_eq!(stop_in(&e, "conv-b", "b-1"), json!({}));
    assert!(stop(&e, "p-3")["systemMessage"].is_string());
    let event = prompt(&e, "p-4", "b is 7");
    answer(&e, "wrong", &event, WRONG, "wrong");
    assert_eq!(stop(&e, "p-4"), json!({}));
    prompt(&e, "p-5", "now I'm really done");
    end(&e);
    let again = stop(&e, "p-5");
    assert_eq!(
        again["systemMessage"],
        format!(
            "Earned today: a pass on {RIGHT}.\nCan count from tomorrow: roots of a factored… and 1 more."
        ),
        "{again}"
    );
}

#[test]
fn an_end_made_during_a_turn_waits_for_that_turns_stop_hook() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "x is 2 or 5, and that's me done");
    answer(&e, "right", &event, RIGHT, "right");
    end(&e);
    assert!(stop(&e, "p-2")["systemMessage"].is_string());
}

#[test]
fn magnesium_closing_changes_nothing_in_a_store_from_before_the_end_table() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    prompt(&e, "p-1", "teach me");
    v2::start(&e, "claude", "conv");
    let event = prompt(&e, "p-2", "x is 2 or 5");
    answer(&e, "right", &event, RIGHT, "right");
    let path = e.workspace.join(".magnesium/sittings.sqlite");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE sitting_ends;")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let read = closing_json(&e);
    assert!(read["sitting"]["ended_ms"].is_null(), "{read}");
    assert!(read["closing"]["lines"].is_array(), "{read}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the read changed the store"
    );
}

#[test]
fn a_cleared_conversation_cannot_end_the_sitting() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "2.1.293"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let mut mcp = McpClient::spawn_claude(&e, &[("CLAUDE_CODE_SESSION_ID", "conv")]);
    mcp.request(1, "server/discover", json!({"_meta": meta}));
    let input = json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"SessionEnd","reason":"clear"});
    assert!(
        run_hook_with_host_flag(&e, &e.workspace, "session-end", input, "claude")
            .status
            .success()
    );
    let refused = mcp.request(
        2,
        "tools/call",
        json!({"_meta": meta, "name": "learning_step", "arguments": {"schema":"learner-model/v2","action":"end_sitting"}}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(
        refused.to_string().contains("conversation_changed"),
        "{refused}"
    );
}
