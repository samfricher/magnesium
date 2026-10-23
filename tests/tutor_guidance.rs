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
    let resumed = v2::context(&run_hook_with_host_flag(
        &e,
        &e.workspace,
        "session-start",
        json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"SessionStart","source":"resume"}),
        "claude",
    ));
    assert!(
        resumed.contains("compaction, load the learn skill"),
        "{resumed}"
    );

    let codex = v2::context(&v2::prompt(&e, "codex", "conv", Some("t-1"), "teach me"));
    assert!(!codex.contains("learn skill"), "{codex}");
}

fn claude_stop(e: &TestEnvironment, thread: &str, prompt_id: &str, reply: &str) {
    let input = json!({"session_id":thread,"cwd":e.workspace,"hook_event_name":"Stop","prompt_id":prompt_id,"stop_hook_active":false,"last_assistant_message":reply});
    let output = run_hook_with_host_flag(e, &e.workspace, "stop", input, "claude");
    assert!(output.status.success(), "{output:?}");
}

const REMINDER: &str = "Teaching reply: one idea, under 200 words, end with one task.";
const WHEN_CONFUSED: &str =
    "If the learner is confused, ask one question to find out why before explaining.";

#[test]
fn claude_lesson_turns_carry_the_teaching_reminder_and_name_what_the_last_reply_missed() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let inactive = claude_prompt(&e, "conv", "p-1", "can we practise confidence intervals?");
    assert!(!inactive.contains("Teaching reply"), "{inactive}");
    assert!(!inactive.contains(WHEN_CONFUSED), "{inactive}");

    v2::start(&e, "claude", "conv");
    let first = claude_prompt(&e, "conv", "p-2", "is it 950?");
    assert!(first.contains(REMINDER), "{first}");
    assert!(first.contains(WHEN_CONFUSED), "{first}");
    assert!(!first.contains("Previous reply"), "{first}");
    assert!(first.len() <= 512, "{} bytes: {first}", first.len());

    claude_stop(&e, "conv", "p-2", &"Here is why. ".repeat(283));
    let second = claude_prompt(&e, "conv", "p-3", "so the margin halves?");
    assert!(second.contains(" Previous reply: 849 words."), "{second}");
    assert!(second.contains(REMINDER), "{second}");
    assert!(!second.contains(WHEN_CONFUSED), "{second}");
    assert!(second.len() <= 512, "{} bytes: {second}", second.len());

    claude_stop(
        &e,
        "conv",
        "p-3",
        "Yes. Now try n = 400: what is the margin?",
    );
    let third = claude_prompt(&e, "conv", "p-4", "about 4.9?");
    assert!(!third.contains("Previous reply"), "{third}");

    let session_start = v2::context(&run_hook_with_host_flag(
        &e,
        &e.workspace,
        "session-start",
        json!({"session_id":"conv","cwd":e.workspace,"hook_event_name":"SessionStart","source":"compact"}),
        "claude",
    ));
    assert!(
        session_start.contains("After a new conversation or compaction, load the learn skill"),
        "{session_start}"
    );
    assert!(!session_start.contains("Teaching reply"), "{session_start}");

    v2::start(&e, "codex", "conv");
    let codex = v2::context(&v2::prompt(&e, "codex", "conv", Some("t-1"), "teach me"));
    assert!(!codex.contains("Teaching reply"), "{codex}");
}

#[test]
fn claude_reminder_stays_without_feedback_while_recording_is_off() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", "conv");
    let event = claude_prompt(&e, "conv", "p-1", "stop recording please")
        .split_once("current event ")
        .unwrap()
        .1
        .split('.')
        .next()
        .unwrap()
        .to_owned();
    v2::tool(
        &e,
        "claude",
        "conv",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"control","request_id":"off","control":"recording_off","turn_ref":event}),
    );
    claude_stop(&e, "conv", "p-1", &"Unrecorded words. ".repeat(300));
    let notice = claude_prompt(&e, "conv", "p-2", "what next?");
    assert!(notice.contains("recording false"), "{notice}");
    assert!(notice.contains(REMINDER), "{notice}");
    assert!(!notice.contains("Previous reply"), "{notice}");
    assert!(notice.len() <= 512, "{} bytes: {notice}", notice.len());
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
    let host = "claude";
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

#[test]
fn served_instructions_skip_the_task_only_on_the_learners_terms() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let served = v2::tool(
        &e,
        host,
        "task-exits",
        "context",
        json!({"action":"instructions","host":host,"stage":"teach_and_practice"}),
    );
    let text = served["text"].as_str().unwrap();
    assert!(
        text.contains(
            "Skip it only when the learner requested explanation-only or the session is ending."
        ),
        "{host}"
    );
    for exit in ["already observed", "not yet usable"] {
        assert!(
            !text.contains(exit),
            "{host} still skips the task when {exit}"
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
        "The next time means a later problem of that kind, not another message about the same problem or its changed version.",
        "Help goes back as well as down: when the learner does not solve the changed version or the full problem alone, or later fails a cold redo or delayed check of that kind, move the step back one level, so \"partly worked\" becomes \"full\" and \"full\" is removed.",
        "A learner who can start gets no example step.",
        "When you give one, record \"example step (KIND): full\" or \"example step (KIND): partly worked\"",
        "With no course graph, it is the memory with the kind in topics. Search memory for that label, and create the memory if none has it.",
        "Keep one step per kind.",
        "Read that memory in full. No example step there means a full worked example.",
        "A requested full solution and a fails-twice rewrite record none.",
        "rewrite the task with smaller steps or a worked example first, whatever the example step;",
        "The worked example is never the question they must answer or a later paper question, unless the learner asks for that question's full solution.",
    ];
    let host = "claude";
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
        "| No starting move on a later problem of this kind, \"example step (KIND): full\" recorded | Give a partly worked example",
        "| No starting move on a later problem of this kind, \"example step (KIND): partly worked\" recorded | Give the problem only",
        "| No starting move on the same problem or its changed version | Take a smaller step from the example already given, with no new example |",
        "Record a step back in the turn that shows they did not solve the follow-up alone.",
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
    let contract = v2::tool(
        &e,
        host,
        "worked-example",
        "context",
        json!({"action":"instructions","host":host,"stage":"teach_and_practice","operation":"state_and_evidence"}),
    )["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        contract.contains("Every rewrite or merge of a memory keeps each \"example step (KIND)\" from the state_text it replaces, unless it records a step back, and a merge that finds two steps for one kind keeps \"partly worked\"."),
        "{host}"
    );
}

#[test]
fn the_spaced_redo_rules_are_served_with_the_tool_recipes() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
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

#[test]
fn the_gap_log_upgrade_is_served_and_the_papers_skill_adds_only_the_template_steps() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let papers = v2::tool(
        &e,
        host,
        "gap-log-upgrade",
        "context",
        json!({"action":"instructions","host":host,"stage":"papers_loop","operation":"tool_recipes"}),
    );
    let text = papers["text"].as_str().unwrap();
    for step in [
        "## Upgrade an old gap log",
        "Rename `Cold-redo date` to `Next redo`",
        "after an empty `Passes` column.",
        "write Status as exactly `closed`",
        "gets tomorrow's date if it has none and its Repair done is filled",
        "If the log has no Conf column, add one after Type, empty in every row. In a log that already has `Next redo`, change no other column or cell, since an open row with an empty Next redo there waits for a new repair. A `gap-log.md` without a `Next redo` column predates spaced redos.",
        "Dismiss the plan's old gap review suggestions",
        "log the upgrade as one line under Changes in `plan.md`",
    ] {
        assert!(text.contains(step), "{host}: {step}");
    }
    let (_, old_log_steps) = text.split_once("predates spaced redos.").unwrap();
    assert!(
        old_log_steps.contains("gets tomorrow's date"),
        "{host}: the date fill applies only to a log without Next redo"
    );
    let skill = include_str!("../claude-plugin/skills/papers/SKILL.md");
    let (_, section) = skill.split_once("## Upgrade an old gap log").unwrap();
    let section = section.split("\n## ").next().unwrap();
    assert!(
        section.contains("papers_loop") && section.contains("`## Tags`"),
        "{section}"
    );
    for served_step in [
        "Cold-redo date",
        "Passes",
        "Dismiss",
        "tomorrow",
        "today",
        "exactly",
        "after `Type`",
        "empty in every row",
    ] {
        assert!(
            !section.contains(served_step),
            "the papers skill repeats a served step of the upgrade ({served_step}): {section}"
        );
    }
}

#[test]
fn the_verb_the_help_screen_offers_for_unreadable_lines_is_served_and_listed() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let verb = "fix my plan files";
    for (name, text) in [
        (
            "help skill",
            include_str!("../claude-plugin/skills/help/SKILL.md"),
        ),
        (
            "papers skill",
            include_str!("../claude-plugin/skills/papers/SKILL.md"),
        ),
        (
            "plan template",
            include_str!("../claude-plugin/skills/papers/templates/plan.md"),
        ),
    ] {
        assert!(text.contains(verb), "{name} does not mention {verb:?}");
    }
    let papers_description = include_str!("../claude-plugin/skills/papers/SKILL.md")
        .lines()
        .find(|line| line.starts_with("description:"))
        .unwrap();
    assert!(
        papers_description.contains(verb),
        "the papers skill description, which routes the verb to the skill, does not list {verb:?}"
    );
    let host = "claude";
    let served = v2::tool(
        &e,
        host,
        "fix-plan-files",
        "context",
        json!({"action":"instructions","host":host,"stage":"papers_loop","operation":"tool_recipes"}),
    );
    let text = served["text"].as_str().unwrap();
    for part in [
        verb,
        "correct each `unreadable` item in `magnesium digest` at its `file` and `line`",
        "`unreadable`",
        "rerun it until none is left",
    ] {
        assert!(text.contains(part), "{host}: {part}");
    }
}

#[test]
fn served_instructions_ask_a_confused_learner_one_question_before_explaining() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rule = "When the learner says they are confused, ask one question to find the cause and explain nothing yet.";
    let host = "claude";
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
        assert!(served(stage).contains(rule), "{host}/{stage}: {rule}");
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
    let host = "claude";
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
            !text.contains("the memory wins"),
            "{host}/{stage} repeats the rule that belongs to the state_and_evidence contract"
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
            "add an \"anchor:\" line to that skill's tag memory, unless an anchor there already covers it.",
        ),
        (
            "papers",
            None,
            "At build, run a memory search with all_of [\"anchor\"] and any_of naming the course's tags and topics, and read each memory it returns. Copy into Anchors each \"anchor:\" line that names a skill this course uses.",
        ),
    ] {
        assert!(
            served(stage, operation).contains(required),
            "{host}/{stage}: {required}"
        );
    }
    let rule = "That line is the source. Before you add one, read the memory, and replace an anchor that the new one covers instead of adding a second, so the list stays short. A plan's Anchors section keeps a copy of the anchors its topics use, and where a copy and its memory differ, the memory wins. When you add, correct or retire an anchor, make the same change in the Anchors section of each plan that covers its topic or already holds a copy of it, in the same step, adding the section if the plan lacks it. A plan covers a topic when its `Study/<target>/graph.yaml` has a tag for it.";
    for stage in [
        "teach_and_practice",
        "interpret_and_repair",
        "papers",
        "papers_loop",
    ] {
        let text = v2::tool(
            &e,
            host,
            "anchors",
            "context",
            json!({"action":"instructions","host":host,"stage":stage,"operation":"state_and_evidence"}),
        )["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(text.contains(rule), "{host}/{stage}: {rule}");
    }
    let template = include_str!("../claude-plugin/skills/papers/templates/plan.md");
    assert!(
        template
            .lines()
            .any(|line| line.starts_with("## ") && line.ends_with(" Anchors")),
        "the plan template has no Anchors section"
    );
    assert!(
        template.lines().any(|line| line.starts_with("- anchor: <")),
        "the plan template has no anchor line to fill in"
    );
}

#[test]
fn served_instructions_give_a_stuck_learner_the_worked_example_before_any_anchor_question() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let policy = "Build the similar problem on an anchor if one fits, and give it without one if none does. A missing anchor never delays it.";
    let by_stage: [(&str, &[&str]); 2] = [
        (
            "teach_and_practice",
            &[
                "then the rows for a learner with no starting move, then the anchor question, then the other rows,",
                "A learner who knows how to start tries first, with no anchor question. A learner with no starting move gets the example or problem in their row, never the anchor question first.",
                "A learner with no starting move gets the worked example first, built on an anchor if one fits and without one if none does.",
            ],
        ),
        (
            "interpret_and_repair",
            &[
                "A learner with no starting move on a kind of problem gets the policy's worked example first, with no anchor question.",
                "Build each explanation on an anchor.",
            ],
        ),
    ];
    let host = "claude";
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
    let ask = teach
        .find("Otherwise, before you explain, if no anchor fits, first ask")
        .unwrap();
    let first = teach
        .find("A learner with no starting move gets the worked example first")
        .unwrap();
    assert!(
        first < ask,
        "{host}: the anchor question comes before the worked example"
    );
}

#[test]
fn served_instructions_make_an_anchor_only_from_an_unaided_answer() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let rule = "Add an anchor only from an unaided answer, one the learner gave with no hint, step or solution from you after you asked the question, since an anchor is their own work. An answer that copies the steps of a full solution or worked example you just gave is reproduction, not an anchor.";
    let host = "claude";
    let served = |stage: &str| {
        v2::tool(
            &e,
            host,
            "unaided",
            "context",
            json!({"action":"instructions","host":host,"stage":stage}),
        )["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for stage in ["teach_and_practice", "interpret_and_repair"] {
        assert!(served(stage).contains(rule), "{host}/{stage}: {rule}");
    }
    assert!(
        served("papers_loop").contains(
            "a skill a fully correct part shows, answered with no hint, step or solution from you,"
        ),
        "{host}: a marked part can be helped"
    );
}

#[test]
fn served_papers_build_asks_when_and_where_the_learner_will_practise() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let served = |stage: &str| {
        v2::tool(
            &e,
            host,
            "sessions",
            "context",
            json!({"action":"instructions","host":host,"stage":stage}),
        )["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let line_form = "Each session is one line, \"If it is <days> at <HH:MM>, then I <what> <where>.\", with a 24-hour start time. Leave out \"<days> at\" for every day. Then add one backup line that starts \"If I miss\".";
    let write = format!("Write the answer under a Sessions heading in `plan.md`. {line_form}");
    let papers = served("papers");
    for required in [
        "Also ask when and where they will practise, and what they will do if they miss a session. Suggest a default that fits their hours and commitments. They may take it or skip the question.",
        write.as_str(),
    ] {
        assert!(papers.contains(required), "{host}/papers: {required}");
    }
    let rule = format!(
        "If the learner changes when or where they practise, rewrite Sessions, adding it if missing. {line_form}"
    );
    assert!(
        served("papers_loop").contains(&rule),
        "{host}/papers_loop: {rule}"
    );
}

#[test]
fn served_instructions_ask_how_sure_the_learner_is_of_each_answer() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
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

#[test]
fn served_instructions_repair_sure_mistakes_first() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
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

#[test]
fn served_orientation_aims_its_one_question_near_the_top() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let text = v2::tool(
        &e,
        host,
        "start-high",
        "context",
        json!({"action":"instructions","host":host,"stage":"orient_and_probe"}),
    )["text"]
        .as_str()
        .unwrap()
        .to_owned();
    for rule in [
        "Ask at most one discriminating question at a time.",
        "Aim it at the hardest step the goal needs that the learner might already know, not at the basics,",
        "After a confident correct answer, aim any further question sharply higher. Ask a basic question only after a miss.",
        "a two-stage test problem to solve, not a request to state Bayes' rule.",
    ] {
        assert!(text.contains(rule), "{host}/orient_and_probe: {rule}");
    }
}

#[test]
fn served_instructions_ask_for_a_guess_before_each_new_step() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let served = |stage: &str| {
        v2::tool(
            &e,
            host,
            "guess-first",
            "context",
            json!({"action":"instructions","host":host,"stage":stage}),
        )["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let teach = served("teach_and_practice");
    for rule in [
        "Before you explain a step the learner has not seen, ask what they think comes next and why,",
        "A rough guess is enough, since the attempt is the point.",
        "open the explanation with what the guess got right and what it got wrong, then give the check task.",
        "the turn that explains a step ends with its check, and the next turn ends with the guess for the following step.",
        "then the anchor question, then the other rows, then the guess.",
        "Skip the guess in the same cases, when a learner with no starting move gets a worked example, and in a turn that repairs a missed check.",
        "A correct guess with a sound reason is an unaided answer.",
    ] {
        assert!(teach.contains(rule), "{host}/teach_and_practice: {rule}");
    }
    let not_a_gap = "A wrong guess you asked for before teaching a step is not a mistake: never record it as a difficulty or a gap.";
    for stage in ["teach_and_practice", "interpret_and_repair"] {
        assert!(
            served(stage).contains(not_a_gap),
            "{host}/{stage}: {not_a_gap}"
        );
    }
}

#[test]
fn served_instructions_look_at_every_figure_before_the_learner_sees_it() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let rules = [
        "When you make a figure or page as a file, render it to an image with what the machine has, such as a headless browser screenshot or an SVG-to-PNG converter, then look at the image, fix what is wrong and look again.",
        "Show it only after you have looked at the final version.",
        "Check that nothing overlaps or is cut off, every label is readable,",
        "and nothing in it is unlabelled decoration.",
        "When a browser is available, open a page with scripts once in a headless browser to confirm it runs without errors.",
        "If the machine cannot render the file, say so and use text or a Mermaid diagram instead.",
    ];
    for (stage, shows) in [
        (
            "teach_and_practice",
            "the picture shows what the step says,",
        ),
        ("papers", "the picture shows what its question or key says,"),
    ] {
        let text = v2::tool(
            &e,
            host,
            "figures",
            "context",
            json!({"action":"instructions","host":host,"stage":stage}),
        )["text"]
            .as_str()
            .unwrap()
            .to_owned();
        for rule in rules.iter().chain([&shows]) {
            assert!(text.contains(rule), "{host}/{stage}: {rule}");
        }
    }
}

#[test]
fn served_instructions_use_three_question_shapes_that_resist_pattern_matching() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let host = "claude";
    let served = |stage: &str| {
        v2::tool(
            &e,
            host,
            "shapes",
            "context",
            json!({"action":"instructions","host":host,"stage":stage,"operation":"tool_recipes"}),
        )
    };
    let shapes = "gives a true result and asks why it holds, sets two look-alikes that need different methods and asks what decides it, or drops one assumption and asks which step fails.";
    let reason = "A given result earns full marks only for a correct reason.";
    let by_stage: [(&str, &[&str]); 3] = [
        (
            "papers",
            &[
                "Three question shapes resist pattern matching.",
                "Give the result, ask the reason:",
                "Two look-alikes, one difference:",
                "Break one assumption:",
                "Every mock paper has at least one question of each shape.",
                reason,
            ],
        ),
        (
            "papers_loop",
            &["For a Type K row, prefer a question that", shapes, reason],
        ),
        (
            "teach_and_practice",
            &[
                "When the gap is about understanding, not speed, notation or arithmetic, prefer a task that",
                shapes,
                reason,
            ],
        ),
    ];
    for (stage, rules) in by_stage {
        let loaded = served(stage);
        assert_eq!(loaded["deferred_modules"], json!([]), "{host}/{stage}");
        let text = loaded["text"].as_str().unwrap();
        for rule in rules {
            assert!(text.contains(rule), "{host}/{stage}: {rule}");
        }
    }
    let template = include_str!("../claude-plugin/skills/papers/templates/plan.md");
    for shape in [
        "- Give the reason:",
        "- Two look-alikes:",
        "- Break one assumption:",
    ] {
        assert!(template.contains(shape), "plan template: {shape}");
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
