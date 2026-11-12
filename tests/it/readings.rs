//! The runtime keeps each kind of problem's help level and works out each
//! topic's reading, and the tool results state both (#31). The tutor judges
//! the kind and whether the learner could start; it writes neither fact into
//! a memory.
use crate::support::{TestEnvironment, clock, run_hook_with_host_flag, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const THREAD: &str = "conv";
const KIND: &str = "discrete expected value";

fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":THREAD,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    let notice = v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
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
    let value = v2::tool(e, "claude", THREAD, "learning_step", request);
    assert_eq!(value["update_status"], "committed", "{value}");
    value
}

fn answer(e: &TestEnvironment, id: &str, text: &str, attempt: Value) -> Value {
    let event = prompt(e, id, text);
    step(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":id,
            "observations":[{"client_ref":"work","kind":"learner_work","description":"The learner's turn.",
                "events":[{"event_id":event,"role":"learner_response"}],"attempt":attempt}]}),
    )
}

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

fn days_pass(e: &TestEnvironment, days: u64) {
    clock::days_pass(&e.workspace, days);
}

/// The help level a result states for the kind.
fn level(result: &Value) -> &Value {
    let kinds = result["help_levels"]["kinds"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("no help_levels in {result}");
        });
    assert_eq!(kinds.len(), 1, "{result}");
    assert_eq!(kinds[0]["kind"], KIND, "{result}");
    &kinds[0]
}

fn stuck(e: &TestEnvironment, id: &str, question: &str) -> Value {
    answer(
        e,
        id,
        "I have no idea how to start this one.",
        json!({"label":"weights in an expected value","question":question,"first_try":"declined",
            "help":"none","cold":false,"kind":KIND,"no_start":true}),
    )
}

fn memories(e: &TestEnvironment) -> i64 {
    store(e)
        .query_row("SELECT count(*) FROM learner_memories", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_learner_stuck_twice_on_one_kind_gets_a_full_example_then_a_partly_worked_one() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);

    // First time stuck on the kind: a full worked example.
    let first = stuck(&e, "stuck-1", "A die shows 6 half the time. Find E[X].");
    let now = level(&first);
    assert_eq!(now["level"], "full", "{first}");
    assert!(
        now["give_now"]
            .as_str()
            .unwrap()
            .contains("full worked example"),
        "{first}"
    );
    assert_eq!(first["help_levels"]["known_kinds"], json!([KIND]));
    // The changed version, solved alone, holds the step.
    let changed = answer(
        &e,
        "changed-1",
        "4.5",
        json!({"label":"weights in an expected value","question":"A die shows 1 half the time. Find E[X].",
            "first_try":"right","help":"none","cold":false,"kind":KIND}),
    );
    assert_eq!(level(&changed)["level"], "full", "{changed}");
    assert!(
        level(&changed)["if_stuck_next"]
            .as_str()
            .unwrap()
            .contains("partly worked"),
        "{changed}"
    );

    // A later problem of the kind, stuck again: a partly worked example.
    days_pass(&e, 2);
    let second = stuck(
        &e,
        "stuck-2",
        "A spinner pays 1, 2 or 5. Find the mean payout.",
    );
    assert_eq!(level(&second)["level"], "partly worked", "{second}");
    assert!(
        level(&second)["give_now"]
            .as_str()
            .unwrap()
            .contains("partly worked example"),
        "{second}"
    );
    // The full problem that follows is not solved alone: one step back.
    let failed = answer(
        &e,
        "full-2",
        "2.67",
        json!({"label":"weights in an expected value","question":"A spinner pays 1, 2 or 5 with weights 1/2, 1/3, 1/6. Find E[X].",
            "first_try":"wrong","help":"none","cold":false,"kind":KIND}),
    );
    assert_eq!(level(&failed)["level"], "full", "{failed}");
    // Stuck on the next problem: partly worked again, not the problem only.
    days_pass(&e, 2);
    let third = stuck(
        &e,
        "stuck-3",
        "Find the expected number of heads in 3 flips.",
    );
    assert_eq!(level(&third)["level"], "partly worked", "{third}");

    // The tutor wrote no memory, and the store holds the level.
    assert_eq!(memories(&e), 0);
    let stored: (String, Option<String>) = store(&e)
        .query_row(
            "SELECT kind, level FROM help_levels WHERE kind_key=?1",
            [KIND],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (KIND.to_owned(), Some("partial".to_owned())));

    // Correcting the failed answer to right moves the ladder on from it.
    let attempt_id: String = store(&e)
        .query_row(
            "SELECT attempt_id FROM attempts WHERE question LIKE '%weights 1/2%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let corrected = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"correct",
            "attempt_corrections":[{"attempt_id":attempt_id,"reason":"The key had a slip; 2.67 is right.","first_try":"right"}]}),
    );
    assert_eq!(corrected["update_status"], "committed");
    let level: Option<String> = store(&e)
        .query_row(
            "SELECT level FROM help_levels WHERE kind_key=?1",
            [KIND],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(level.as_deref(), Some("problem_only"));
}

#[test]
fn a_kind_is_needed_for_no_start_and_names_a_method_not_a_problem() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    let refused = |id: &str, attempt: Value, field: &str| {
        let event = prompt(&e, id, "no idea");
        let output = v2::tool_output(
            &e,
            "claude",
            THREAD,
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":id,
                "observations":[{"client_ref":"work","kind":"learner_work","description":"Stuck.",
                    "events":[{"event_id":event,"role":"learner_response"}],"attempt":attempt}]}),
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["recovery"]["field_path"], field, "{value}");
    };
    refused(
        "no-kind",
        json!({"label":"weights","question":"Find E[X].","first_try":"declined","help":"none","cold":false,"no_start":true}),
        "observations[0].attempt.kind",
    );
    refused(
        "right",
        json!({"label":"weights","question":"Find E[X].","first_try":"right","help":"none","cold":false,"kind":KIND,"no_start":true}),
        "observations[0].attempt.first_try",
    );
    refused(
        "problem",
        json!({"label":"weights","question":"Find E[X].","first_try":"wrong","help":"none","cold":false,"kind":"x^2 = 4"}),
        "observations[0].attempt.kind",
    );
}

const GRAPH: &str = "tags:\n  lineweaver-burk:\n    covers: reading Km and Vmax from the double reciprocal plot\n    kind: course\n    needs: [reciprocals]\n    key_need: [reciprocals]\n    contains: [reciprocals]\n    touches: []\n  reciprocals:\n    covers: 1/x and its inverse\n    kind: foundation\n    needs: []\n    key_need: []\n    contains: []\n    touches: []\n  inhibition:\n    covers: competitive and noncompetitive inhibition\n    kind: course\n    needs: [lineweaver-burk]\n    key_need: [lineweaver-burk]\n    contains: []\n    touches: []\n";

fn plan(e: &TestEnvironment) {
    let dir = e.workspace.join("Study/midterm-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plan.md"),
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | <YYYY-MM-DD> |\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("gap-log.md"),
        include_str!("../../claude-plugin/skills/papers/templates/gap-log.md"),
    )
    .unwrap();
    std::fs::write(dir.join("graph.yaml"), GRAPH).unwrap();
}

/// The reading a result states for a tag, and why.
fn reading(result: &Value, tag: &str) -> (String, String) {
    let plan = &result["readings"]["plans"][0];
    assert_eq!(plan["plan"], "midterm-1", "{result}");
    for name in ["open", "conditional", "sound"] {
        for entry in plan[name].as_array().into_iter().flatten() {
            if entry["tag"] == tag {
                return (name.to_owned(), entry["why"].as_str().unwrap().to_owned());
            }
        }
    }
    if plan["untested"]
        .as_array()
        .is_some_and(|tags| tags.contains(&json!(tag)))
    {
        return ("untested".to_owned(), String::new());
    }
    panic!("no reading for {tag} in {result}");
}

#[test]
fn a_topic_reading_changes_when_its_gap_row_changes_with_nothing_written_by_the_tutor() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e);

    // Starting a session for the plan states its readings: nothing yet.
    let started = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"start",
            "session":{"operation":"start","client_ref":"session","goal":"Midterm 1 practice papers"}}),
    );
    assert_eq!(reading(&started, "lineweaver-burk").0, "untested");
    assert_eq!(reading(&started, "inhibition").0, "untested");

    // A marked paper: the double reciprocal question right, cold and in
    // time; the inhibition question lost marks on the reciprocal.
    let event = prompt(&e, "p-1", "Diagnostic: 1. Km = 2 (S) 2. Vmax = 4 (U)");
    let marked = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"paper",
            "observations":[{"client_ref":"paper","kind":"learner_work","description":"Diagnostic, timed.",
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempts":[
                    {"label":"lineweaver-burk","question":"Find Km from the plot","first_try":"right","help":"none","cold":true,
                     "paper":{"name":"Diagnostic","question":"1","marks":2,"out_of":2}},
                    {"label":"reciprocals","question":"Find Vmax with an inhibitor","first_try":"partly","help":"none","cold":true,
                     "paper":{"name":"Diagnostic","question":"2","marks":1,"out_of":3},
                     "gap":{"type":"K","what_went_wrong":"Took 1/0.25 as 0.4."}}]}]}),
    );
    // The row is open, so the tags that run through it read conditional,
    // even the one with its own cold pass.
    assert_eq!(
        reading(&marked, "reciprocals"),
        (
            "open".to_owned(),
            "midterm-1#1 open, waiting for its repair".to_owned()
        )
    );
    assert_eq!(
        reading(&marked, "lineweaver-burk"),
        (
            "conditional".to_owned(),
            "needs reciprocals, which is open".to_owned()
        )
    );
    assert_eq!(reading(&marked, "inhibition").0, "untested");

    // The repair dates the first redo; the row is still open.
    let repaired = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"repair",
            "practice":[{"item":"midterm-1#1","repaired":"drilled reciprocals",
                "next":{"question":"Intercept 0.5: find Vmax.","answer_key":"2"}}]}),
    );
    assert_eq!(reading(&repaired, "reciprocals").0, "open");

    // A day later the cold redo passes: the row is at 1 of 3, and the reading
    // follows it, with no memory edit.
    days_pass(&e, 1);
    answer(
        &e,
        "redo-1",
        "Vmax = 2",
        json!({"label":"reciprocals","question":"Intercept 0.5: find Vmax.","first_try":"right","help":"none","cold":true,
            "counts_for":["midterm-1#1"],"next":{"question":"Intercept 0.2: find Vmax.","answer_key":"5"}}),
    );
    let resumed = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume",
            "session":{"operation":"resume","session_id":started["references"]["session"],"expected_revision":1}}),
    );
    assert_eq!(
        reading(&resumed, "reciprocals"),
        ("conditional".to_owned(), "midterm-1#1 at 1 of 3".to_owned())
    );
    assert_eq!(
        reading(&resumed, "lineweaver-burk"),
        ("sound".to_owned(), "cold in Diagnostic Q1".to_owned())
    );

    // A paper stage load states them too.
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(reading(&load, "reciprocals").0, "conditional");
    assert_eq!(memories(&e), 0);

    // An attempt whose kind is a tag states the plan's readings with the
    // tag's help level, even when its label is not a tag.
    let stuck = answer(
        &e,
        "stuck",
        "No idea how to start this inhibition one.",
        json!({"label":"which intercept moves","question":"A competitive inhibitor is added. What happens to the plot?",
            "first_try":"declined","help":"none","cold":false,"kind":"inhibition","no_start":true}),
    );
    assert_eq!(
        stuck["readings"]["plans"][0]["help_levels"]["inhibition"], "full",
        "{stuck}"
    );

    // A graph.yaml it cannot read is named, and the rows still read.
    std::fs::write(
        e.workspace.join("Study/midterm-1/graph.yaml"),
        "tags:\n  a:\n    needs: b, c\n",
    )
    .unwrap();
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    let plan = &load["readings"]["plans"][0];
    assert!(
        plan["graph"]
            .as_str()
            .unwrap()
            .starts_with("Study/midterm-1/graph.yaml: line 3:"),
        "{load}"
    );
    assert_eq!(reading(&load, "reciprocals").0, "conditional");
}
