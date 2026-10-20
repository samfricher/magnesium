//! Guidance the runtime gives a live tutor, found missing by dogfooding Claude Code.
use serde_json::{Value, json};
use support::{TestEnvironment, run_hook_with_host_flag, v2};

fn claude_prompt(e: &TestEnvironment, thread: &str, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":thread,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ))
}

#[test]
fn claude_notices_say_when_to_start_and_continue_a_lesson_and_codex_notices_do_not() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);

    let inactive = claude_prompt(&e, "conv", "p-1", "can we practise confidence intervals?");
    let ticket = inactive
        .split_once("native_capture.ticket=")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    assert_eq!(ticket.len(), 32, "{inactive}");
    assert!(
        inactive.contains("load the learn skill and start or resume a session"),
        "{inactive}"
    );

    v2::start(&e, "claude", "conv");
    let active = claude_prompt(&e, "conv", "p-2", "is it 950?");
    assert!(active.contains("current event e-"), "{active}");
    assert!(
        active.contains("interpret this learner turn with learning_step"),
        "{active}"
    );
    assert!(
        active.contains("compaction, load the learn skill"),
        "{active}"
    );

    let codex = v2::context(&v2::prompt(&e, "codex", "conv", Some("t-1"), "teach me"));
    assert!(!codex.contains("learn skill"), "{codex}");
}

#[test]
fn claude_session_start_points_a_new_learner_at_help_until_a_session_exists() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let session_start = |host: &str, thread: &str| {
        let input = json!({"session_id":thread,"cwd":e.workspace,"hook_event_name":"SessionStart","source":"startup"});
        v2::context(&run_hook_with_host_flag(
            &e,
            &e.workspace,
            "session-start",
            input,
            host,
        ))
    };

    let fresh = session_start("claude", "first");
    assert!(
        fresh.contains("\"New here? Type /magnesium:help.\""),
        "{fresh}"
    );
    let codex = session_start("codex", "first");
    assert!(!codex.contains("/magnesium:help"), "{codex}");

    v2::start(&e, "claude", "first");
    let returning = session_start("claude", "second");
    assert!(returning.contains("context.prepare"), "{returning}");
    assert!(!returning.contains("/magnesium:help"), "{returning}");
}

#[test]
fn reused_request_ids_name_the_fix_for_updates_and_controls() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "claude", "conv");
    let session = started["references"]["session"].as_str().unwrap();
    let reuse = |request: Value| {
        let output = v2::tool_output(&e, "claude", "conv", "learning_step", request);
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    // The start recipe's own request_id, reused for a different update.
    let update = reuse(
        json!({"schema":"learner-model/v2","action":"update","request_id":"start-claude-conv","session":{"operation":"use","session_id":session,"expected_revision":1}}),
    );
    let event = v2::context(&v2::prompt(
        &e,
        "claude",
        "conv",
        Some("t-1"),
        "pause please",
    ))
    .split_once("current event ")
    .unwrap()
    .1
    .split('.')
    .next()
    .unwrap()
    .to_owned();
    let first = reuse(
        json!({"schema":"learner-model/v2","action":"control","request_id":"pause-1","control":"recording_off","turn_ref":event}),
    );
    assert_eq!(first["durable"], true, "{first}");
    let control = reuse(
        json!({"schema":"learner-model/v2","action":"control","request_id":"pause-1","control":"pause","turn_ref":event}),
    );
    for response in [update, control] {
        assert_eq!(
            response["error"]["code"], "request_key_conflict",
            "{response}"
        );
        let instruction = response["error"]["recovery"]["instruction"]
            .as_str()
            .unwrap();
        assert!(instruction.contains("new request_id"), "{instruction}");
        assert!(
            instruction.contains("shared by every conversation"),
            "{instruction}"
        );
    }
}

#[test]
fn served_instructions_ask_for_plain_learner_facing_save_failures() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["codex", "claude", "opencode"] {
        let served = v2::tool(
            &e,
            host,
            "plain-words",
            "context",
            json!({"action":"instructions","host":host,"stage":"teach_and_practice","operation":"tool_recipes"}),
        );
        let text = served["text"].as_str().unwrap();
        assert!(text.contains("Never show the learner tool names, fields, error codes or IDs."));
        assert!(
            served["files"]
                .to_string()
                .contains("contracts/tool-recipes.md")
        );
    }
}

#[test]
fn served_instructions_teach_a_new_question_type_with_a_similar_worked_problem() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rules = [
        "The worked example is never the question they must answer or a later paper question.",
        "then give a changed version with the same structure and new details to solve alone.",
        "Keep its times-met count current.",
    ];
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "worked-example",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let teach = served("teach_and_practice");
        for required in [
            "| Knows how to start | Let the learner try first |",
            "| New type of question, no starting move | Work a similar problem",
            "| Same type, second time, no starting move | Give a partly worked example",
            "| Same type, third time or later, no starting move | Give the problem only",
            "The state_text of the learner's memory for this type of question holds its times-met count",
        ] {
            assert!(teach.contains(required), "{host}: {required}");
        }
        for stage in ["teach_and_practice", "papers_loop"] {
            let text = served(stage);
            for rule in rules {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
        }
    }
}

#[test]
fn served_instructions_ask_a_confused_learner_one_question_before_explaining() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rule = "When the learner says they are confused, ask one question to find the cause and explain nothing yet.";
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "confused",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        for stage in [
            "orient_and_probe",
            "plan_and_route",
            "teach_and_practice",
            "interpret_and_repair",
            "review_and_transfer",
            "consolidate_and_resume",
            "papers",
            "papers_loop",
        ] {
            assert!(served(stage).contains(rule), "{host}/{stage}");
        }
        assert!(
            served("teach_and_practice")
                .contains("| Says they are confused | Ask one question to find the cause"),
            "{host}"
        );
        assert!(
            served("interpret_and_repair").contains(
                "When the learner says they are confused, ask one question to find the cause"
            ),
            "{host}"
        );
    }
}

#[test]
fn malformed_context_calls_name_both_complete_shapes() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for request in [
        json!({"action":"instructions","host":"claude","stage":"start"}),
        json!({"action":"instructions","host":"claude"}),
        json!({"host":"claude","stage":"orient_and_probe"}),
    ] {
        let output = v2::tool_output(&e, "claude", "conv", "context", request.clone());
        assert!(!output.status.success(), "{request}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        for required in [
            "{\"action\":\"instructions\",\"host\":",
            "\"stage\":\"orient_and_probe\"|",
            "{\"schema\":\"learner-model/v2\",\"action\":\"status\"|\"prepare\"",
        ] {
            assert!(stderr.contains(required), "{request}: {stderr}");
        }
    }
}

#[test]
fn malformed_memory_calls_name_the_search_read_and_browse_shapes() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for request in [
        json!({"schema":"learner-model/v2","action":"search","query":"intervals"}),
        json!({"schema":"learner-model/v2","action":"read","kind":"event","id":"e-1"}),
    ] {
        let output = v2::tool_output(&e, "claude", "conv", "memory", request.clone());
        assert!(!output.status.success(), "{request}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        for required in [
            "\"action\":\"search\",\"goal\"",
            "\"refs\":[{\"kind\":\"current_memory\"|\"observation\"|\"event\",\"id\"",
            "\"action\":\"browse\"",
        ] {
            assert!(stderr.contains(required), "{request}: {stderr}");
        }
    }
}
