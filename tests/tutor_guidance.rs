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
fn served_instructions_teach_a_new_kind_of_problem_with_a_similar_worked_problem() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rules = [
        "then give a changed version with the same structure and new details to solve alone.",
        "The next time they are stuck on that kind of problem, give a partly worked example",
        "No step recorded means the full worked example comes next.",
        "The worked example is never the question they must answer or a later paper question, unless the learner asks for that question's full solution.",
        "Keep any example step or anchor in its state_text.",
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
        for stage in ["teach_and_practice", "papers_loop"] {
            let text = served(stage);
            for rule in rules {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
        }
        let teach = served("teach_and_practice");
        for required in [
            "| Knows how to start | Let the learner try first |",
            "| No starting move, no example step recorded for this kind of problem | Work a similar problem",
            "| No starting move, \"example step: full\" recorded | Give a partly worked example",
            "| No starting move, \"example step: partly worked\" recorded | Give the problem only",
            "When rows conflict, take them in this order: an explicit request",
        ] {
            assert!(teach.contains(required), "{host}: {required}");
        }
        let papers = served("papers_loop");
        for required in [
            "the tag's next example step from the teaching policy",
            "\"conditional: needs TAG, which is open; example step: full\"",
        ] {
            assert!(papers.contains(required), "{host}: {required}");
        }
    }
}

#[test]
fn served_instructions_ask_a_confused_learner_one_question_before_explaining() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rules = [
        "When the learner says they are confused, ask one question to find the cause and explain nothing yet.",
        "When the learner says they are confused, ask about the cause before you choose.",
    ];
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
            let text = served(stage);
            for rule in rules {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
        }
        assert!(
            served("teach_and_practice")
                .contains("| Says they are confused | Ask one question to find the cause"),
            "{host}"
        );
        assert!(
            served("interpret_and_repair")
                .contains("If confusion repeats after you explain from the learner's answer"),
            "{host}"
        );
        assert!(
            served("orient_and_probe")
                .contains("explain now, unless the learner says they are confused."),
            "{host}"
        );
    }
}

#[test]
fn served_instructions_build_each_explanation_from_an_anchor() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rules = [
        "then forward from an anchor, a skill or way of thinking the learner's own work has shown, with where they showed it.",
        "Say which anchor you build on before you explain.",
        "If none fits, first ask \"What do you already know that looks like this?\"",
        "When the learner then answers a check question correctly, add the approach that worked as an anchor, written as what they did, not as a learning style.",
        "Keep any example step or anchor in its state_text.",
    ];
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "anchors",
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
            let text = served(stage);
            for rule in rules {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
        }
        for (stage, required) in [
            (
                "teach_and_practice",
                "Look for anchors in the state_text of current memories and in the Anchors section of a plan that covers the topic.",
            ),
            (
                "teach_and_practice",
                "Skip that question right after you asked a confused learner about the cause",
            ),
            (
                "teach_and_practice",
                "Outside a plan, add a line such as \"anchor: ",
            ),
            (
                "papers",
                "At build, fill Anchors from the memories you read.",
            ),
            (
                "papers_loop",
                "When an open tag needs a skill a fully correct part shows, add it to the plan's Anchors.",
            ),
        ] {
            assert!(
                served(stage).contains(required),
                "{host}/{stage}: {required}"
            );
        }
    }
    let template = include_str!("../claude-plugin/skills/papers/templates/plan.md");
    assert!(
        template
            .lines()
            .any(|line| line.starts_with("## ") && line.ends_with(" Anchors")),
        "the plan template has no Anchors section for the instructions to name"
    );
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
