use crate::support::mcp::McpClient;
use crate::support::{TestEnvironment, run_hook, run_hook_off, v2};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const OFF_LINE: &str = "Magnesium is off in this session, so nothing here is kept. Tell the user: to study with Magnesium, type /magnesium:learn or /magnesium:papers.";

fn files(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut found = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(entry.path());
            }
            found.push((entry.path(), if meta.is_dir() { 0 } else { meta.len() }));
        }
    }
    found.sort();
    found
}

fn control(e: &TestEnvironment) -> PathBuf {
    e.workspace.join(crate::support::CONTROL_DIR)
}

fn session_start(session: &str, source: &str, e: &TestEnvironment) -> Value {
    json!({"session_id":session,"cwd":e.workspace,"hook_event_name":"SessionStart","source":source})
}

fn prompt(session: &str, id: &str, text: &str, e: &TestEnvironment) -> Value {
    json!({"session_id":session,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":id,"prompt":text})
}

fn stop(session: &str, id: &str, e: &TestEnvironment) -> Value {
    json!({"session_id":session,"cwd":e.workspace,"hook_event_name":"Stop","prompt_id":id,"stop_hook_active":false,"last_assistant_message":"A reply."})
}

fn printed(output: &std::process::Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn off_client(e: &TestEnvironment, session: &str) -> McpClient {
    let mut client = McpClient::spawn_with_env(
        e,
        true,
        &[
            ("MAGNESIUM_SESSION", "off"),
            ("CLAUDE_CODE_SESSION_ID", session),
        ],
    );
    client.initialize();
    client
}

fn learner_events(e: &TestEnvironment) -> i64 {
    v2::count(e, "learning_events")
}

#[test]
fn an_off_session_prints_nothing_keeps_nothing_and_answers_tools_in_one_line() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let before = files(&control(&e));
    for (event, input) in [
        ("session-start", session_start("work", "startup", &e)),
        (
            "user-prompt-submit",
            prompt("work", "p-1", "teach me confidence intervals", &e),
        ),
        ("stop", stop("work", "p-1", &e)),
        ("session-start", session_start("work", "compact", &e)),
    ] {
        assert_eq!(printed(&run_hook_off(&e, event, input)), "", "{event}");
    }
    assert_eq!(
        files(&control(&e)),
        before,
        "an off session wrote in .magnesium"
    );
    let mut client = off_client(&e, "work");
    for (id, tool, arguments) in [
        (
            2,
            "context",
            json!({"schema":"learner-model/v2","action":"status"}),
        ),
        (
            3,
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"g"}}),
        ),
        (4, "search_sources", json!({"query":"intervals"})),
    ] {
        let response = client.request(id, "tools/call", json!({"name":tool,"arguments":arguments}));
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert_eq!(response["result"]["content"][0]["text"], OFF_LINE, "{tool}");
    }
    client.close_cleanly();
    assert_eq!(
        files(&control(&e)),
        before,
        "an off tool call wrote in .magnesium"
    );
    assert_eq!(learner_events(&e), 0);
    assert_eq!(v2::count(&e, "learning_sessions"), 0);
}

#[test]
fn typing_a_study_command_turns_the_session_on_until_it_ends() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    assert_eq!(
        printed(&run_hook_off(
            &e,
            "user-prompt-submit",
            prompt("study", "p-1", "please run /magnesium:learn for me", &e)
        )),
        ""
    );
    assert_eq!(
        printed(&run_hook_off(
            &e,
            "user-prompt-submit",
            prompt("study", "p-2", "/magnesium:help", &e)
        )),
        ""
    );
    let on = printed(&run_hook_off(
        &e,
        "user-prompt-submit",
        prompt("study", "p-3", "/magnesium:learn teach me week 3", &e),
    ));
    assert!(on.contains("native_capture.ticket="), "{on}");
    assert!(on.contains("Magnesium is on in this session"), "{on}");
    let mut client = off_client(&e, "study");
    let started = client.call_value(
        2,
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Week 3"}}),
    );
    assert_eq!(started["update_status"], "committed", "{started}");
    client.close_cleanly();
    let kept = printed(&run_hook_off(
        &e,
        "user-prompt-submit",
        prompt("study", "p-4", "is it the sample mean?", &e),
    ));
    assert!(kept.contains("current event e-"), "{kept}");
    assert_eq!(learner_events(&e), 1);
    for source in ["compact", "resume"] {
        let notice = printed(&run_hook_off(
            &e,
            "session-start",
            session_start("study", source, &e),
        ));
        assert!(notice.contains("A lesson is active"), "{source}: {notice}");
    }
    let again = printed(&run_hook_off(
        &e,
        "user-prompt-submit",
        prompt("study", "p-5", "and the variance?", &e),
    ));
    assert!(again.contains("current event e-"), "{again}");
    assert_eq!(learner_events(&e), 2);
    assert_eq!(
        printed(&run_hook_off(
            &e,
            "user-prompt-submit",
            prompt("new-session", "p-1", "and the variance?", &e)
        )),
        ""
    );
    assert_eq!(learner_events(&e), 2);
}

#[test]
fn the_other_study_commands_turn_a_session_on_too() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for (session, command) in [
        ("papers", "/magnesium:papers I have a midterm Monday"),
        ("checks", "/magnesium:checks"),
    ] {
        let on = printed(&run_hook_off(
            &e,
            "user-prompt-submit",
            prompt(session, "p-1", command, &e),
        ));
        assert!(
            on.contains("Magnesium is on in this session"),
            "{command}: {on}"
        );
    }
}

#[test]
fn an_open_lesson_in_a_session_that_is_off_keeps_nothing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", "old-chat");
    assert!(
        printed(&run_hook(
            &e,
            "user-prompt-submit",
            prompt("old-chat", "p-1", "kept while on", &e)
        ))
        .contains("current event e-")
    );
    assert_eq!(learner_events(&e), 1);
    assert_eq!(
        printed(&run_hook_off(
            &e,
            "user-prompt-submit",
            prompt("old-chat", "p-2", "PRIVATE_OFF_PROMPT", &e)
        )),
        ""
    );
    let mut delivery = stop("old-chat", "p-2", &e);
    delivery["last_assistant_message"] = json!("PRIVATE_OFF_DELIVERY");
    assert_eq!(printed(&run_hook_off(&e, "stop", delivery)), "");
    let private: i64 = rusqlite::Connection::open(control(&e).join("state.sqlite"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM learning_events WHERE text LIKE '%PRIVATE_OFF%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(private, 0);
    assert_eq!(learner_events(&e), 1);
}
