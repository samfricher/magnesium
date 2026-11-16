//! A pass's receipt, end to end: the record's line under an answer that
//! earned a pass, in `magnesium progress --json` for the study window and in
//! the PostToolUse hook's output for a terminal.
use crate::support::{
    TestEnvironment, clock, magnesium_version, run_hook_with_host_flag, test_binary, v2,
};
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;

const THREAD: &str = "conv";

fn answer(e: &TestEnvironment, id: &str, text: &str, attempt: Value) -> Value {
    let input = json!({"session_id":THREAD,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":id,"prompt":text});
    let notice = v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
    let event = notice
        .split_once("current event ")
        .unwrap()
        .1
        .split(['.', ' '])
        .next()
        .unwrap()
        .to_owned();
    let value = v2::tool(
        e,
        "claude",
        THREAD,
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":id,
            "observations":[{"client_ref":"work","kind":"learner_work","description":"The learner's turn.",
                "events":[{"event_id":event,"role":"learner_response"}],"attempt":attempt}]}),
    );
    assert_eq!(value["update_status"], "committed", "{value}");
    value
}

fn progress(e: &TestEnvironment) -> Value {
    let output = e.command().args(["progress", "--json"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The PostToolUse hook as Claude Code runs it after `tool`, with the tool's
/// result handed over as its content. `surface` is the study window's mark
/// on a Claude Code it starts.
fn post_tool_use(e: &TestEnvironment, tool: &str, result: &Value, surface: Option<&str>) -> String {
    let input = json!({"session_id":THREAD,"cwd":e.workspace,"hook_event_name":"PostToolUse",
        "tool_name":tool,"tool_input":{},"tool_response":[{"type":"text","text":result.to_string()}]});
    let mut command = e.command_with(test_binary(), &e.workspace);
    command.args([
        "_internal",
        "hook",
        "post-tool-use",
        "--plugin-version",
        magnesium_version(),
        "--adapter-protocol-version",
        "2",
        "--host-kind",
        "claude",
    ]);
    if let Some(surface) = surface {
        command.env("MAGNESIUM_SURFACE", surface);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

const LEARNING_STEP: &str = "mcp__plugin_magnesium_magnesium__learning_step";

#[test]
fn a_pass_after_a_miss_carries_its_receipt_to_the_window_and_the_terminal() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    answer(
        &e,
        "a-1",
        "b is 7",
        json!({"label":"sign of b","question":"In x² − 7x + 10 = 0, what is b?","first_try":"wrong","help":"none","cold":false}),
    );
    // A miss on another skill whose label differs only by a symbol.
    answer(
        &e,
        "a-2",
        "e^x",
        json!({"label":"integral of e^{-x}","question":"Integrate e^{-x}.","first_try":"wrong","help":"none","cold":false}),
    );
    clock::days_pass(&e.workspace, 3);
    let right = answer(
        &e,
        "a-3",
        "b is −5",
        json!({"label":"sign of b","question":"In x² − 5x + 6 = 0, what is b?","first_try":"right","help":"none","cold":true}),
    );
    let symbol = answer(
        &e,
        "a-4",
        "e^x + C",
        json!({"label":"integral of e^{x}","question":"Integrate e^{x}.","first_try":"right","help":"none","cold":true}),
    );
    let id = |result: &Value| {
        result["practice"]["attempts"][0]["attempt_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (passed, other) = (id(&right), id(&symbol));

    // The window's reading: the receipt under the pass, from the record.
    let printed = progress(&e);
    let receipt = &printed["pass_receipts"][&passed];
    assert_eq!(receipt["missed"], "sign of b", "{printed}");
    assert_eq!(receipt["missed_mark"], "wrong", "{receipt}");
    assert_eq!(receipt["days"], 3, "{receipt}");
    let text = receipt["text"].as_str().unwrap();
    assert!(text.starts_with("Not right on "), "{text}");
    assert!(
        text.ends_with(": sign of b. It held through the 3 days since you last answered it."),
        "{text}"
    );
    // The first answer on "integral of e^{x}" names no other skill's miss.
    assert!(printed["pass_receipts"].get(&other).is_none(), "{printed}");

    // A terminal: the same line after the learning_step that recorded it.
    let said: Value =
        serde_json::from_str(&post_tool_use(&e, LEARNING_STEP, &right, None)).unwrap();
    assert_eq!(
        said,
        json!({"systemMessage": format!("First of three days. {text}")})
    );
    // A first answer with no receipt says nothing.
    assert_eq!(post_tool_use(&e, LEARNING_STEP, &symbol, None), "");
    // A Claude Code the study window started: the window shows the line
    // from the record, so the hook says nothing.
    assert_eq!(post_tool_use(&e, LEARNING_STEP, &right, Some("window")), "");
}

#[test]
fn the_reply_tools_end_of_turn_is_unchanged() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    for surface in [None, Some("window")] {
        assert_eq!(
            post_tool_use(
                &e,
                "mcp__plugin_magnesium_magnesium__reply",
                &json!({}),
                surface
            ),
            r#"{"continue":false,"suppressOutput":true}"#
        );
    }
}
