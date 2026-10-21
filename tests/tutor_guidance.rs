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
        "When you give one, record \"example step (KIND): full\" or \"example step (KIND): partly worked\"",
        "Keep one step per kind.",
        "Read that memory in full. No example step there means a full worked example.",
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
            "| No starting move, \"example step (KIND): full\" recorded | Give a partly worked example",
            "| No starting move, \"example step (KIND): partly worked\" recorded | Give the problem only",
            "Name the kind in every step, so a tag with several kinds keeps them apart.",
            "When rows conflict, take them in this order: an explicit request",
            "Record it in the turn you give the example",
            "Add the step to the existing state_text without dropping the rest",
        ] {
            assert!(teach.contains(required), "{host}: {required}");
        }
        let papers = served("papers_loop");
        for required in [
            "the tag's next example step if they could not start, or under rule 6 a worked example,",
            "\"conditional: needs TAG, which is open; example step (KIND): full\"",
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
        "then forward from an anchor, a skill or way of thinking the learner's own work has shown, not a learning style. Say which anchor you build on, if any.",
        "another anchor or representation",
        "Keep any example step or anchor in its state_text.",
    ];
    let question = "\"What do you already know that looks like this?\"";
    let skip_cases = [
        (
            "teach_and_practice",
            "So skip the anchor question after an explicit request, a fails-twice rewrite, or any question you just asked, such as the confusion question,",
        ),
        (
            "interpret_and_repair",
            "Skip that question when you just asked any question, such as the cause of a confusion or a diagnostic, when the learner asked for an explanation or a full solution, and when a fails-twice rewrite applies.",
        ),
    ];
    let procedure = [
        "memory search with all_of [\"anchor\"] and any_of naming the topic and the skills it needs, and read each memory it returns",
        "A tutor explanation, a source or a self-report is never an anchor.",
        "forward from the next dependency in small steps.",
        "lowercase \"anchor:\"",
        "to the existing state_text of the memory for that skill, without dropping the rest.",
    ];
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str, operation: Option<&str>| {
            let mut request = json!({"action":"instructions","host":host,"stage":stage});
            if let Some(operation) = operation {
                request["operation"] = json!(operation);
            }
            v2::tool(&e, host, "anchors", "context", request)["text"]
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
            let text = served(stage, None);
            for rule in rules {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
            assert!(
                !text.contains("literal_terms [\"anchor:\"]"),
                "{host}/{stage} looks anchors up without the topic, and a search returns at most 24"
            );
            assert!(
                !text.contains("Anchors") && !text.contains("copy it"),
                "{host}/{stage} tells the tutor to keep a plan copy of the anchors"
            );
            let skip = skip_cases.iter().find(|(name, _)| *name == stage);
            assert_eq!(
                text.contains(question),
                skip.is_some(),
                "{host}/{stage} serves the anchor question only with its skip cases"
            );
            if let Some((_, skip)) = skip {
                assert!(text.contains(skip), "{host}/{stage}: {skip}");
                for step in procedure {
                    assert!(text.contains(step), "{host}/{stage}: {step}");
                }
            }
        }
        for (stage, operation, required) in [
            (
                "orient_and_probe",
                None,
                "plus a query with all_of [\"anchor\"] and the topic in any_of for anchors",
            ),
            (
                "plan_and_route",
                None,
                "To find anchors, run a memory search with all_of [\"anchor\"] and any_of naming the goal and what it needs, and read each memory it returns. With none, start the route at the next dependency.",
            ),
            (
                "teach_and_practice",
                None,
                "Scope it to the topic, because one query returns at most 24 records.",
            ),
            (
                "papers_loop",
                None,
                "add an \"anchor:\" line to that skill's tag memory.",
            ),
            (
                "teach_and_practice",
                Some("state_and_evidence"),
                "That line is the only copy. A plan holds none, so correcting or retiring the memory corrects the anchor everywhere.",
            ),
        ] {
            assert!(
                served(stage, operation).contains(required),
                "{host}/{stage}: {required}"
            );
        }
    }
    let template = include_str!("../claude-plugin/skills/papers/templates/plan.md");
    assert!(
        template
            .lines()
            .any(|line| line.starts_with("## ") && line.ends_with(" Anchors")),
        "the plan template has no Anchors section"
    );
    assert!(
        !template.contains("- anchor:"),
        "the plan template lists an anchor line to fill in"
    );
}

#[test]
fn served_instructions_give_a_stuck_learner_the_worked_example_before_any_anchor_question() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let policy = "Build the similar problem on an anchor if one fits, and give it without one if none does. A missing anchor never delays it.";
    let by_stage = [
        (
            "teach_and_practice",
            [
                "then the rows for a learner with no starting move, then the anchor question, then the other rows.",
                "A learner with no starting move gets the example or problem in their row, never the anchor question first.",
                "A learner with no starting move gets the worked example first, built on an anchor if one fits and without one if none does.",
            ],
        ),
        (
            "interpret_and_repair",
            [
                "A learner with no starting move on a kind of problem gets the policy's worked example first, with no anchor question.",
                "Skip that question when you just asked any question",
                "Build each explanation on an anchor.",
            ],
        ),
    ];
    for host in ["codex", "claude", "opencode"] {
        let served = |stage: &str| {
            v2::tool(
                &e,
                host,
                "example-first",
                "context",
                json!({"action":"instructions","host":host,"stage":stage}),
            )["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        for (stage, required) in by_stage {
            let text = served(stage);
            assert!(text.contains(policy), "{host}/{stage}: {policy}");
            for rule in required {
                assert!(text.contains(rule), "{host}/{stage}: {rule}");
            }
        }
        let teach = served("teach_and_practice");
        let ask = teach.find("if no anchor fits, first ask").unwrap();
        let first = teach
            .find("A learner with no starting move gets the worked example first")
            .unwrap();
        assert!(
            first < ask,
            "{host}: the anchor question comes before the worked example"
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
