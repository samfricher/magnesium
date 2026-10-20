//! `magnesium digest` gathers folder and learner-record facts for /magnesium:help without writing.
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use support::{TestEnvironment, v2};

const THREAD: &str = "digest";

fn digest(e: &TestEnvironment) -> Value {
    let output = e.command().arg("digest").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn update(e: &TestEnvironment, request: Value) -> Value {
    let receipt = v2::tool(e, "codex", THREAD, "learning_step", request);
    assert_eq!(receipt["update_status"], "committed", "{receipt}");
    receipt
}

/// One lesson with a checkpoint and three review suggestions, one due in the year 3000.
fn seed_lesson(e: &TestEnvironment) {
    let started = v2::start(e, "codex", THREAD);
    let session = started["references"]["session"].as_str().unwrap();
    let captured = update(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"capture",
        "session":{"operation":"use","session_id":session,"expected_revision":1},
        "reported_events":[{"external_event_key":"work","speaker":"learner","kind":"message",
            "text":"In 2x² + 7x + 3, b = 7.","occurred_at_ms":1}]}),
    );
    let event = &captured["captured_event_ids"][0];
    let reviews = [
        ("now", "Name b in a new quadratic", Some(0_i64)),
        ("undated", "Explain the sign of b", None),
        (
            "later",
            "Use b in the discriminant",
            Some(32_503_680_000_000_i64),
        ),
    ]
    .map(|(name, purpose, due)| {
        json!({"operation":"create","client_ref":name,"memory_id":"memory","purpose":purpose,
            "reason":"Spaced retrieval.","due_at_ms":due})
    });
    update(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"lesson",
        "session":{"operation":"use","session_id":session,"expected_revision":2},
        "observations":[{"client_ref":"work","kind":"learner_work",
            "description":"Identified b in one example.",
            "events":[{"event_id":event,"role":"learner_response"}]}],
        "memory_edits":[{"operation":"create","client_ref":"memory",
            "record":{"title":"Coefficient roles","scope":"Algebra","summary":"Correct on one example.",
                "state_text":"Transfer untested.","current_basis":[{"new_observation":"work"}]},
            "reason":"Keep local evidence.","change_basis":[{"new_observation":"work"}]}],
        "review_updates":reviews,
        "event_dispositions":[{"event_id":event,"outcome":"processed"}],
        "checkpoint":{"current_problem":"Midterm 1 Diagnostic marked",
            "working_diagnosis":"b correct once",
            "next_action":"Cold redo of the 2 open gaps, then sit Paper1-Easy",
            "memory_refs":[{"memory_id":"memory","revision":"after_edit"}],"pending_task":null}}),
    );
}

fn write_plan(root: &Path) {
    let dir = root.join("Study/midterm-1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plan.md"),
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | 2999-01-02 18:00 |\n\n## 6. Blocks\n\n| # | Block | Purpose |\n|---|---|---|\n| 1 | Diagnostic | find gaps |\n| 2 | Paper1-Easy | ramp |\n",
    )
    .unwrap();
    fs::write(
        dir.join("gap-log.md"),
        "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Q | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|\n\
         | 1 | 2000-01-01 | Diagnostic | Q2 | | 2999-01-01 | open |\n\
         | 2 | 2000-01-01 | Diagnostic | Q4 | 2000-01-02 pass | 2000-01-05 | 1 of 3 |\n\
         | 3 | 2000-01-01 | Diagnostic | Q5 | 2000-01-02 pass, 2000-01-05 pass | 2000-01-12 | 2 of 3 |\n\
         | 4 | 2000-01-01 | Diagnostic | Q6 | 2000-01-02 pass, 2000-01-05 pass, 2000-01-12 pass | | closed |\n\
         | 5 | 2000-01-01 | Diagnostic | Q7 | 2000-01-03 fail | | open |\n\n\
         ## Scores\n\n| Paper | Date | Score |\n|---|---|---|\n| Diagnostic | 2026-10-14 | 14/30 |\n",
    )
    .unwrap();
}

fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push((path.display().to_string(), fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn digest_reports_files_plans_checkpoint_and_ready_reviews_without_writing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    seed_lesson(&e);
    write_plan(&e.workspace);
    fs::write(e.workspace.join("AGENTS.md"), "setup text").unwrap();
    let before = tree(&e.workspace);

    let value = digest(&e);

    assert_eq!(
        tree(&e.workspace),
        before,
        "magnesium digest changed the folder"
    );
    assert_eq!(value["active"], true);
    // Hidden entries and setup's AGENTS.md are skipped; the plan files are a level too deep.
    assert_eq!(
        value["files"],
        json!({"count":3,"listed":["Study/","Study/midterm-1/","lesson.md"],"truncated":false})
    );

    let session = &value["record"]["sessions"][0];
    assert_eq!(session["goal"], "Learn coefficient roles");
    assert_eq!(session["days_ago"], 0);
    assert_eq!(
        session["next_action"],
        "Cold redo of the 2 open gaps, then sit Paper1-Easy"
    );
    assert_eq!(value["record"]["memories"], 1);

    let reviews = &value["record"]["reviews"];
    assert_eq!(reviews["ready"], 2, "{reviews}");
    let mut purposes = reviews["ready_purposes"].as_array().unwrap().clone();
    purposes.sort_by_key(|p| p.to_string());
    assert_eq!(
        purposes,
        [
            json!("Explain the sign of b"),
            json!("Name b in a new quadratic")
        ]
    );
    assert_eq!(reviews["later"], 1, "{reviews}");
    assert!(reviews["next_ready_in_days"].as_i64().unwrap() > 1000);

    let plan = &value["plans"][0];
    assert_eq!(plan["target"], "midterm-1");
    assert_eq!(plan["title"], "Midterm 1");
    assert_eq!(plan["date"], "2999-01-02");
    assert!(plan["days_left"].as_i64().unwrap() > 1000);
    assert_eq!(plan["blocks"], json!(["Diagnostic", "Paper1-Easy"]));
    assert_eq!(plan["unfixed_gaps"], 4);
    assert_eq!(plan["redos_due"], 2);
    assert_eq!(
        plan["first_due"],
        json!({"paper": "Diagnostic", "question": "Q4"})
    );
    assert_eq!(plan["scores"][0]["score"], "14/30");
    // The goal does not name the target, so no session is matched to the plan.
    assert_eq!(plan["session"], Value::Null);
}

#[test]
fn digest_counts_unfixed_rows_of_an_old_gap_log_and_none_as_due() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let dir = e.workspace.join("Study/final");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plan.md"), "# Final: study plan\n").unwrap();
    fs::write(
        dir.join("gap-log.md"),
        "# Gap log: final\n\n## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Cold-redo date | Status |\n|---|---|---|---|---|---|---|---|---|---|\n\
         | 1 | 2000-01-01 | Diagnostic | Q2 | km | sign error | A | tighten | 2000-01-02 | open |\n\
         | 2 | 2000-01-01 | Diagnostic | Q3 | vmax | no units | N | tighten | 2000-01-02 | closed |\n",
    )
    .unwrap();

    let plan = &digest(&e)["plans"][0];

    assert_eq!(plan["target"], "final");
    assert_eq!(plan["unfixed_gaps"], 1);
    assert_eq!(plan["redos_due"], 0);
    assert_eq!(plan["first_due"], Value::Null);
}

#[test]
fn digest_of_a_new_study_folder_is_empty_and_outside_one_is_inactive() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let outside = digest(&e);
    assert_eq!(
        outside,
        json!({"active":false,"reason":"workspace_not_initialized"})
    );
    assert!(!e.workspace.join(support::CONTROL_DIR).exists());

    v2::init(&e);
    let value = digest(&e);
    assert_eq!(value["plans"], json!([]));
    assert_eq!(value["record"]["sessions"], json!([]));
    assert_eq!(value["record"]["reviews"]["ready"], 0);
    assert_eq!(value["files"]["listed"], json!(["lesson.md"]));
}
