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
        "The next time they are stuck on it, give a partly worked example",
        "A learner who can start gets no example step.",
        "Read that memory, not a search result. No example step there means a full worked example.",
        "A requested full solution and a fails-twice rewrite record none.",
        "rewrite the task with smaller steps or a worked example first, whatever the example step;",
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
            "Record it in the turn you give the example",
        ] {
            assert!(teach.contains(required), "{host}: {required}");
        }
        let papers = served("papers_loop");
        for required in [
            "the tag's next example step if they could not start",
            "\"conditional: needs TAG, which is open; example step: full\"",
        ] {
            assert!(papers.contains(required), "{host}: {required}");
        }
    }
}

#[test]
fn every_host_is_served_the_spaced_redo_rules_with_its_tool_recipes() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "spaced-redos",
                "context",
                json!({"action":"instructions","host":host,"stage":stage,"operation":"tool_recipes"}),
            )
        };
        let papers = served("papers_loop");
        assert_eq!(papers["deferred_modules"], json!([]), "{host}: {papers}");
        let text = papers["text"].as_str().unwrap();
        for rule in [
            "A gap-log row closes after three cold passes on three different days.",
            "The first redo is at least 1 day after the repair, never on the repair day.",
            "The second is at least 3 days after the first pass, and the third at least 7 days after the second.",
            "move Status from `open` to `1 of 3`, `2 of 3`, then `closed`",
            "An attempt while Next redo is empty or still to come counts neither way.",
            "A due redo that is not a pass fails the row.",
            "Spaced redos overrides any one-pass rule left in the log or plan.",
        ] {
            assert!(text.contains(rule), "{host}/papers_loop: {rule}");
        }
        let review = served("review_and_transfer");
        let rule = "a gap-log row's Next redo is its only schedule, never a review suggestion.";
        assert!(
            review["text"].as_str().unwrap().contains(rule),
            "{host}/review_and_transfer: {rule}"
        );
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
                "Search memories for \"anchor:\" and read the memories it finds, since a search result can leave state_text out. Also check the Anchors section of a plan that covers the topic.",
            ),
            (
                "teach_and_practice",
                "Skip that question right after you asked a confused learner about the cause",
            ),
            (
                "teach_and_practice",
                "When a plan covers the topic, add it to that plan's Anchors section. Otherwise add a line such as \"anchor: ",
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
fn served_instructions_ask_how_sure_the_learner_is_of_each_answer() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "confidence",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let papers_loop = served("papers_loop");
        for required in [
            "If the log has no Conf column, add one after Type, empty in every row.",
            "In papers and cold redos, ask for S (sure), U (unsure) or G (guess) after each answer, or before marking a paper without letters. Log a decline under Changes and do not ask again.",
            "One letter on a question covers its parts.",
            "Copy each letter into its rows' Conf, or a blank, as for a lone `?`.",
            "show S, U and G against right (full marks) and wrong answers, per part and without stretch, in a table. Keep the counts in its Scores Notes.",
            "Conf is not evidence and never passes or closes a row or changes a tag's reading.",
            "Conf takes any letter the failing answer has",
        ] {
            assert!(
                papers_loop.contains(required),
                "{host}/papers_loop: {required}"
            );
        }
        assert!(
            served("papers").contains("and S (sure), U (unsure) or G (guess) after each answer."),
            "{host}/papers"
        );
        assert!(
            served("review_and_transfer")
                .contains("Load stage papers_loop before the redo; it picks and records it."),
            "{host}/review_and_transfer"
        );
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
            for (at, _) in text.match_indices("S row") {
                let before = &text[..at];
                assert!(
                    before.ends_with("Type ") || before.ends_with("Conf "),
                    "{host}/{stage}: the bare S row at byte {at} could mean slow or sure"
                );
            }
        }
    }
}

#[test]
fn served_instructions_repair_sure_mistakes_first() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "sure-first",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert!(
            served("papers_loop").contains(
                "When you choose a repair, take Conf S rows first, after their unrepaired key_need tags (no Next redo). Rule 6 still orders a repair's steps after two fails."
            ),
            "{host}/papers_loop"
        );
        let repair = served("interpret_and_repair");
        for required in [
            "When the learner was sure of an answer that is wrong, repair that mistake before the others",
            "How sure they were is not evidence of what they can do.",
            "papers-loop says how this order combines with its other order rules.",
        ] {
            assert!(
                repair.contains(required),
                "{host}/interpret_and_repair: {required}"
            );
        }
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
