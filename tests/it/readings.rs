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
    let failed = answer(
        &e,
        "full-2",
        "2.67",
        json!({"label":"weights in an expected value","question":"A spinner pays 1, 2 or 5 with weights 1/2, 1/3, 1/6. Find E[X].",
            "first_try":"wrong","help":"none","cold":false,"kind":KIND}),
    );
    assert_eq!(level(&failed)["level"], "full", "{failed}");
    days_pass(&e, 2);
    let third = stuck(
        &e,
        "stuck-3",
        "Find the expected number of heads in 3 flips.",
    );
    assert_eq!(level(&third)["level"], "partly worked", "{third}");

    assert_eq!(memories(&e), 0);
    let stored: (String, Option<String>) = store(&e)
        .query_row(
            "SELECT kind, level FROM help_levels WHERE kind_key=?1",
            [KIND],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (KIND.to_owned(), Some("partial".to_owned())));

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

const OLD_GRAPH: &str = "tags:\n  a:\n    needs: b, c\n";

fn plan(e: &TestEnvironment) {
    let dir = e.workspace.join("Study/midterm-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("gap-log.md"),
        include_str!("../../claude-plugin/skills/papers/templates/gap-log.md"),
    )
    .unwrap();
    let written = v2::tool(
        e,
        "claude",
        THREAD,
        "course_map",
        json!({"action": "write", "target": "midterm-1",
               "facts": {"label": "Midterm 1"},
               "items": [
                   {"kind": "idea", "title": "Reading Km and Vmax from the double reciprocal plot", "ref": "lb"},
                   {"kind": "foundation", "title": "1/x and its inverse", "ref": "recip", "origin": "outline"},
                   {"kind": "idea", "title": "Competitive and noncompetitive inhibition", "ref": "inhib"}],
               "links": [{"item": "outline.lb", "uses": ["found.recip"]}, {"item": "outline.inhib", "uses": ["outline.lb"]}],
               "scope": [{"ref": "outline.lb", "state": "in"}, {"ref": "found.recip", "state": "in"}, {"ref": "outline.inhib", "state": "in"}]}),
    );
    assert_eq!(written["items_added"][1]["id"], "found.recip", "{written}");
}

fn has_reading(result: &Value, tag: &str) -> bool {
    let plan = &result["readings"]["plans"][0];
    ["open", "conditional", "sound"].iter().any(|name| {
        plan[*name]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entry| entry["tag"] == tag)
    }) || plan["untested"]
        .as_array()
        .is_some_and(|tags| tags.contains(&json!(tag)))
}

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
fn an_item_reading_changes_when_its_gap_row_changes_and_nothing_passes_along_a_link() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e);

    let started = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"start",
            "session":{"operation":"start","client_ref":"session","goal":"Midterm 1 practice papers"}}),
    );
    assert_eq!(
        started["readings"]["plans"][0]["plan"], "midterm-1",
        "{started}"
    );
    assert!(!has_reading(&started, "outline.lb"), "{started}");

    let event = prompt(&e, "p-1", "Diagnostic: 1. Km = 2 (S) 2. Vmax = 4 (U)");
    let marked = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"paper",
            "observations":[{"client_ref":"paper","kind":"learner_work","description":"Diagnostic, timed.",
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempts":[
                    {"label":"outline.lb","question":"Find Km from the plot","first_try":"right","help":"none","cold":true,
                     "paper":{"name":"Diagnostic","question":"1","marks":2,"out_of":2}},
                    {"label":"found.recip","question":"Find Vmax with an inhibitor","first_try":"partly","help":"none","cold":true,
                     "paper":{"name":"Diagnostic","question":"2","marks":1,"out_of":3},
                     "gap":{"type":"K","what_went_wrong":"Took 1/0.25 as 0.4."}}]}]}),
    );
    assert_eq!(
        reading(&marked, "found.recip"),
        (
            "open".to_owned(),
            "midterm-1#1 open, waiting for its repair".to_owned()
        )
    );
    assert_eq!(
        reading(&marked, "outline.lb"),
        ("sound".to_owned(), "cold in Diagnostic Q1".to_owned())
    );
    assert!(!has_reading(&marked, "outline.inhib"));

    let repaired = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"repair",
            "practice":[{"item":"midterm-1#1","repaired":"drilled reciprocals",
                "next":{"question":"Intercept 0.5: find Vmax.","answer_key":"2"}}]}),
    );
    assert_eq!(reading(&repaired, "found.recip").0, "open");

    days_pass(&e, 1);
    answer(
        &e,
        "redo-1",
        "Vmax = 2",
        json!({"label":"found.recip","question":"Intercept 0.5: find Vmax.","first_try":"right","help":"none","cold":true,
            "counts_for":["midterm-1#1"],"next":{"question":"Intercept 0.2: find Vmax.","answer_key":"5"}}),
    );
    let resumed = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume",
            "session":{"operation":"resume","session_id":started["references"]["session"],"expected_revision":1}}),
    );
    assert_eq!(
        reading(&resumed, "found.recip"),
        ("conditional".to_owned(), "midterm-1#1 at 1 of 3".to_owned())
    );

    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(reading(&load, "found.recip").0, "conditional");
    assert_eq!(memories(&e), 0);

    let stuck = answer(
        &e,
        "stuck",
        "No idea how to start this inhibition one.",
        json!({"label":"which intercept moves","question":"A competitive inhibitor is added. What happens to the plot?",
            "first_try":"declined","help":"none","cold":false,"kind":"inhibition","no_start":true}),
    );
    assert_eq!(
        stuck["help_levels"]["kinds"][0]["kind"], "inhibition",
        "{stuck}"
    );
    assert_eq!(stuck["help_levels"]["kinds"][0]["level"], "full", "{stuck}");

    std::fs::write(e.workspace.join("Study/midterm-1/graph.yaml"), OLD_GRAPH).unwrap();
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    let plan = &load["readings"]["plans"][0];
    assert!(plan.get("graph").is_none(), "{load}");
    assert_eq!(reading(&load, "found.recip").0, "conditional");
}
