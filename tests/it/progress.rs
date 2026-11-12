//! `magnesium progress` prints each skill's passes, the level they add up
//! to, what is due and the days with a marked answer. A study window reads
//! it, so that what it shows is what the record holds.
use crate::support::{TestEnvironment, clock, run_hook_with_host_flag, v2};
use chrono::{Days, Local};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;

const THREAD: &str = "conv";

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

fn answer(e: &TestEnvironment, id: &str, text: &str, attempt: Value) -> Value {
    let event = prompt(e, id, text);
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

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

fn days_pass(e: &TestEnvironment, days: u64) {
    clock::days_pass(&e.workspace, days);
}

fn progress(e: &TestEnvironment) -> Value {
    let output = e.command().args(["progress", "--json"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn skill<'a>(value: &'a Value, name: &str) -> &'a Value {
    value["other"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|skill| skill["skill"] == name)
        .unwrap_or_else(|| panic!("no skill {name} in {value}"))
}

#[test]
fn chat_lessons_earn_passes_for_a_check_and_for_a_skill_never_got_wrong() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);

    // Before any answer: nothing to show, and no day.
    let empty = progress(&e);
    assert_eq!(empty["active"], true, "{empty}");
    assert_eq!(empty["other"]["skills"], json!([]), "{empty}");
    assert_eq!(
        empty["other"]["progress"],
        json!({"skills": 0, "proven": 0, "in_progress": 0}),
        "a folder with no plan prints no level: {empty}"
    );
    assert_eq!(empty["days"], json!([]), "{empty}");

    // Day one. A right, cold, unaided answer on a skill that is never got
    // wrong, a miss that makes a quick check, and a declined question.
    answer(
        &e,
        "a-1",
        "µM per second",
        json!({"label":"units of a rate","question":"What are the units of Vmax?","first_try":"right","help":"none","cold":true}),
    );
    answer(
        &e,
        "a-2",
        "b is 7",
        json!({"label":"sign of b","question":"In x² − 7x + 10 = 0, what is b?","first_try":"wrong","help":"none","cold":false}),
    );
    let first_day = progress(&e);
    let units = skill(&first_day, "units of a rate");
    assert_eq!(
        (units["passes"].as_i64(), units["quick_check"].as_bool()),
        (Some(1), Some(false)),
        "{units}"
    );
    let sign = skill(&first_day, "sign of b");
    assert_eq!(
        (sign["passes"].as_i64(), sign["quick_check"].as_bool()),
        (Some(0), Some(true)),
        "{sign}"
    );
    assert_eq!(
        sign["due"], false,
        "a check made today is due tomorrow: {sign}"
    );
    assert_eq!(
        first_day["due"],
        json!({"redos": 0, "checks": 0}),
        "{first_day}"
    );

    // Day two. The check is due, and passing it cold earns its first pass.
    // The other skill is answered right again, on its second day.
    days_pass(&e, 1);
    let due = progress(&e);
    assert_eq!(due["due"]["checks"], 1, "{due}");
    assert_eq!(skill(&due, "sign of b")["due"], true, "{due}");
    answer(
        &e,
        "b-1",
        "b is -7",
        json!({"label":"sign of b","question":"In 3x² − 5x − 2 = 0, what is b?","first_try":"right","help":"none","cold":true}),
    );
    answer(
        &e,
        "b-2",
        "µM per second again",
        json!({"label":"units of a rate","question":"What are the units of a rate?","first_try":"right","help":"none","cold":true}),
    );
    // A right answer after a hint is no pass.
    answer(
        &e,
        "b-3",
        "it halves",
        json!({"label":"competitive inhibition","question":"What happens to Km?","first_try":"right","help":"hint","cold":false}),
    );

    let value = progress(&e);
    let sign = skill(&value, "sign of b");
    assert_eq!(
        (sign["passes"].as_i64(), sign["proven"].as_bool()),
        (Some(1), Some(false)),
        "{sign}"
    );
    assert_eq!(sign["due"], false, "{sign}");
    let today = Local::now().date_naive();
    assert_eq!(
        sign["next_on"],
        json!(today.checked_add_days(Days::new(3)).unwrap().to_string()),
        "the second pass is at least three days after the first: {sign}"
    );
    assert_eq!(sign["last_pass_on"], json!(today.to_string()), "{sign}");
    assert_eq!(skill(&value, "units of a rate")["passes"], 2, "{value}");
    // Right only after a hint: no pass, and a check due the next day.
    let hinted = skill(&value, "competitive inhibition");
    assert_eq!(
        (
            hinted["passes"].as_i64(),
            hinted["quick_check"].as_bool(),
            hinted["due"].as_bool()
        ),
        (Some(0), Some(true), Some(false)),
        "{hinted}"
    );
    assert_eq!(
        hinted["next_on"],
        json!(today.checked_add_days(Days::new(1)).unwrap().to_string()),
        "{hinted}"
    );

    // Three skills in no plan: counts, and no level.
    assert_eq!(
        value["other"]["progress"],
        json!({"skills": 3, "proven": 0, "in_progress": 3}),
        "{value}"
    );
    assert_eq!(value["due"], json!({"redos": 0, "checks": 0}), "{value}");
    assert_eq!(value["plans"], json!([]), "{value}");
    let yesterday = today.checked_sub_days(Days::new(1)).unwrap();
    assert_eq!(
        value["days"],
        json!([yesterday.to_string(), today.to_string()]),
        "{value}"
    );

    // The text form says the same to a person in a terminal.
    let output = e.command().arg("progress").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("In no plan: 0 proven, 3 in progress"),
        "{text}"
    );
    assert!(text.contains("●●○  units of a rate"), "{text}");
    assert!(
        text.contains("Days with a marked answer: 2 in all."),
        "{text}"
    );
}

const GRAPH: &str = "tags:\n  lineweaver-burk:\n    covers: reading Km and Vmax from the double reciprocal plot\n    kind: course\n    needs: [reciprocals]\n    key_need: [reciprocals]\n    contains: [reciprocals]\n    touches: []\n  reciprocals:\n    covers: 1/x and its inverse\n    kind: foundation\n    needs: []\n    key_need: []\n    contains: []\n    touches: []\n";

#[test]
fn a_plans_level_counts_its_own_skills_and_holds_when_the_learner_starts_something_new() {
    let e = plan_folder();
    let plan_progress = |value: &Value| value["plans"][0]["progress"].clone();

    // A chat answer on a skill the graph names, written another way, is
    // recorded under the graph's name and moves the plan's level.
    answer(
        &e,
        "a-1",
        "Km is minus one over the x intercept",
        json!({"label":"Lineweaver Burk","question":"Read Km from this plot.","first_try":"right","help":"none","cold":true}),
    );
    let before = progress(&e);
    assert_eq!(
        plan_progress(&before),
        json!({"passes": 1, "level_passes": 1, "of": 6, "proven": 0, "skills": 2, "level": 2, "to_next": 1}),
        "{before}"
    );
    let label: String = store(&e)
        .query_row("SELECT label FROM attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(label, "lineweaver-burk");

    // Answers under new names in no plan, right and wrong, leave the plan's
    // level as it was. They are listed with their passes, in no level.
    answer(
        &e,
        "a-2",
        "µM per second",
        json!({"label":"units of a rate","question":"What are the units of Vmax?","first_try":"right","help":"none","cold":true}),
    );
    answer(
        &e,
        "a-3",
        "b is 7",
        json!({"label":"sign of b","question":"In x² − 7x + 10 = 0, what is b?","first_try":"wrong","help":"none","cold":false}),
    );
    let after = progress(&e);
    assert_eq!(plan_progress(&after), plan_progress(&before), "{after}");
    assert_eq!(
        after["other"]["progress"],
        json!({"skills": 2, "proven": 0, "in_progress": 2}),
        "{after}"
    );
    assert!(after["other"]["progress"].get("level").is_none(), "{after}");
}

/// A study folder with one plan, `midterm-1`, whose graph has two tags.
fn plan_folder() -> TestEnvironment {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let dir = e.workspace.join("Study/midterm-1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plan.md"),
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | <YYYY-MM-DD> |\n",
    )
    .unwrap();
    fs::write(
        dir.join("gap-log.md"),
        include_str!("../../claude-plugin/skills/papers/templates/gap-log.md"),
    )
    .unwrap();
    fs::write(dir.join("graph.yaml"), GRAPH).unwrap();
    v2::start(&e, "claude", THREAD);
    e
}

#[test]
fn a_failed_check_leaves_the_plans_level_where_its_passes_took_it() {
    let e = plan_folder();
    let km = |id: &str, first_try: &str| {
        json!({"label":"lineweaver-burk","question":format!("Read Km from plot {id}."),
            "first_try":first_try,"help":"none","cold":first_try == "right"})
    };
    // A miss makes a check. Two cold passes on its due days follow.
    answer(&e, "a-1", "Km is the y intercept", km("a-1", "wrong"));
    days_pass(&e, 1);
    answer(
        &e,
        "b-1",
        "Km is minus one over the x intercept",
        km("b-1", "right"),
    );
    days_pass(&e, 3);
    answer(
        &e,
        "c-1",
        "minus one over the x intercept",
        km("c-1", "right"),
    );
    let passed = progress(&e);
    let level = |value: &Value| value["plans"][0]["progress"].clone();
    assert_eq!(
        level(&passed),
        json!({"passes": 2, "level_passes": 2, "of": 6, "proven": 0, "skills": 2, "level": 4, "to_next": 1}),
        "{passed}"
    );
    // The check is due again, and the learner gets it wrong.
    days_pass(&e, 7);
    answer(&e, "d-1", "the slope", km("d-1", "wrong"));
    let failed = progress(&e);
    let skill = &failed["plans"][0]["skills"][0];
    assert_eq!(skill["skill"], "lineweaver-burk", "{failed}");
    // The dots and the counts tell the truth: the passes start again.
    assert_eq!(
        (
            skill["passes"].as_i64(),
            skill["earlier_passes"].as_i64(),
            skill["best_passes"].as_i64()
        ),
        (Some(0), Some(2), Some(2)),
        "{skill}"
    );
    // The level, and the passes to the next, stay where they were.
    assert_eq!(
        level(&failed),
        json!({"passes": 0, "level_passes": 2, "of": 6, "proven": 0, "skills": 2, "level": 4, "to_next": 1}),
        "{failed}"
    );
}

#[test]
fn progress_counts_what_the_learner_did_today() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    assert_eq!(
        progress(&e)["today_counts"],
        json!({"answered": 0, "started": 0, "passes": 0, "due_tomorrow": 0})
    );
    // Yesterday: a miss on one skill makes a check, due today.
    answer(
        &e,
        "a-1",
        "b is 7",
        json!({"label":"sign of b","question":"In x² − 7x + 10 = 0, what is b?","first_try":"wrong","help":"none","cold":false}),
    );
    days_pass(&e, 1);
    // Today: the check is passed, a new skill is answered right, cold and
    // alone (a pass, with no check), a paper answer on a third skill earns a
    // pass, and a question is declined, which is no answer and makes a check
    // due tomorrow.
    answer(
        &e,
        "b-1",
        "b is -7",
        json!({"label":"sign of b","question":"In 3x² − 5x − 2 = 0, what is b?","first_try":"right","help":"none","cold":true}),
    );
    answer(
        &e,
        "b-2",
        "µM per second",
        json!({"label":"units of a rate","question":"Units of Vmax?","first_try":"right","help":"none","cold":true}),
    );
    answer(
        &e,
        "b-3",
        "4",
        json!({"label":"mean","question":"Mean of 2, 4, 6?","first_try":"right","help":"none","cold":true,
            "paper":{"name":"MockA","question":"1","marks":2,"out_of":2}}),
    );
    answer(
        &e,
        "b-4",
        "I don't know",
        json!({"label":"median","question":"Median of 1, 9, 3?","first_try":"declined","help":"none","cold":false}),
    );
    let value = progress(&e);
    assert_eq!(
        value["today_counts"],
        json!({"answered": 3, "started": 2, "passes": 3, "due_tomorrow": 1}),
        "{value}"
    );
    // The date stays where it was.
    assert_eq!(
        value["today"],
        json!(Local::now().date_naive().to_string()),
        "{value}"
    );
}

#[test]
fn progress_of_a_folder_an_earlier_version_set_up_asks_for_an_upgrade_and_changes_nothing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let store = e
        .workspace
        .join(crate::support::CONTROL_DIR)
        .join("state.sqlite");
    let id: String = Connection::open(&store)
        .unwrap()
        .query_row("SELECT workspace_id FROM workspace_meta", [], |r| r.get(0))
        .unwrap();
    // The record as the release of 21 October made it.
    fs::remove_file(&store).unwrap();
    let old = Connection::open(&store).unwrap();
    old.execute_batch(include_str!("../fixtures/learner_model/schema-2.sql"))
        .unwrap();
    old.execute(
        "INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms)
           VALUES(1,?1,'learner-model/v2',2,1000)",
        [&id],
    )
    .unwrap();
    drop(old);
    let before = fs::read(&store).unwrap();

    let value = progress(&e);
    assert_eq!(value["active"], true, "{value}");
    assert_eq!(value["upgrade_needed"], true, "{value}");
    assert!(value.get("plans").is_none(), "{value}");
    assert!(value.get("days").is_none(), "{value}");
    assert_eq!(fs::read(&store).unwrap(), before, "the record is as it was");
}

#[test]
fn progress_outside_a_study_folder_says_so() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let value = progress(&e);
    assert_eq!(value["active"], false, "{value}");
    let output = e.command().arg("progress").output().unwrap();
    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn a_new_conversation_on_a_topic_hears_the_answers_already_given_on_it() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    answer(
        &e,
        "a-1",
        "3/5",
        json!({"label":"adding fractions with the same denominator","question":"1/5 + 2/5 of a pizza","first_try":"right","help":"none","cold":false}),
    );
    answer(
        &e,
        "a-2",
        "µM per second",
        json!({"label":"units of a rate","question":"What are the units of Vmax?","first_try":"right","help":"none","cold":true}),
    );
    // Another topic that shares only common words with the goal: "what",
    // "with", "find".
    answer(
        &e,
        "a-3",
        "4",
        json!({"label":"what to find with a mean","question":"Find the mean of 2, 4 and 6. What is it?","first_try":"right","help":"none","cold":true}),
    );
    let started = v2::tool(
        &e,
        "claude",
        "another-conversation",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"again",
            "session":{"operation":"start","client_ref":"s","goal":"What to find when adding fractions with different denominators"}}),
    );
    assert_eq!(started["update_status"], "committed", "{started}");
    let earlier = started["earlier_answers"]
        .as_array()
        .expect("earlier answers");
    assert_eq!(earlier.len(), 1, "{started}");
    assert_eq!(
        earlier[0]["label"],
        "adding fractions with the same denominator"
    );
    assert_eq!(earlier[0]["mark"], "right");
}
