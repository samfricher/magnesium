//! Historical synthetic teaching-fixture checks only; these do not execute or
//! establish acceptance of learner-model/v2 tools or a live subscription host.
//! Structural acceptance for the synthetic LEARN-257 learning-loop cases.
//! Semantic decisions remain explicit fixture labels and are checked by the
//! offline Python trace-contract checker; these assertions do not grade prose.

use std::collections::HashSet;
use std::process::{Command, Output};

use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/learning_loop/cases.json");

#[test]
fn synthetic_case_contracts_are_unique_machine_readable_and_complete() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("parse learning-loop fixture");
    assert_eq!(fixture["schema_version"], "learning-loop/v1");
    assert_eq!(fixture["synthetic"], true);
    let cases = fixture["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), 12);

    let expected = [
        "unknown-notation",
        "recurring-application-misconception",
        "pattern-versus-recursion",
        "empty-list-versus-zero",
        "unprompted-correct-reasoning",
        "clarification-during-task",
        "answer-skip-pause",
        "coverage-drift",
        "invalid-trace-material",
        "novel-database-application",
        "cold-resume",
        "direct-factual-answer-outside-lesson",
    ];
    let mut case_ids = HashSet::new();
    let mut input_ids = HashSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(case_ids.insert(id), "duplicate case id {id}");
        assert!(expected.contains(&id), "unexpected case {id}");
        assert!(case["initial_state"].is_object());
        assert!(case["source_fixture"].is_string());
        assert!(
            case["claim_limit"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        assert!(
            case["allowed_actions"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert!(
            case["forbidden_claims"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert!(
            case["reviewed_decisions"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        let inputs = case["inputs"].as_array().expect("ordered inputs");
        assert!(!inputs.is_empty());
        for input in inputs {
            let input_id = input["id"].as_str().expect("fixture-local input id");
            assert!(input_ids.insert(input_id), "duplicate input id {input_id}");
            assert!(input["actor"].is_string());
            assert!(input["text"].as_str().is_some_and(|text| !text.is_empty()));
        }
        let state_changes = case["required_state_changes"]
            .as_array()
            .expect("required state changes");
        let mut state_keys = HashSet::new();
        for change in state_changes {
            let key = change["key"].as_str().expect("state key");
            assert!(state_keys.insert(key), "duplicate required state key {key}");
            assert!(
                change.get("value").is_some(),
                "state change {key} needs expected value"
            );
        }
    }
    assert_eq!(case_ids.len(), expected.len());
    for required in expected {
        assert!(
            case_ids.contains(required),
            "missing required case {required}"
        );
    }
}

#[test]
fn negative_trace_fixtures_cover_required_failure_classes() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("parse learning-loop fixture");
    let negative = fixture["negative_fixtures"]
        .as_array()
        .expect("negative fixtures");
    let ids: HashSet<_> = negative
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    for required in [
        "premature-advancement",
        "wrong-attribution",
        "solution-counted-as-attempt",
        "coverage-inflation",
        "repeated-completed-teaching",
        "no-instructional-action",
        "action-after-delivery",
    ] {
        assert!(
            ids.contains(required),
            "missing negative fixture {required}"
        );
    }
    assert_eq!(negative.len(), 7);
}

#[test]
fn offline_trace_checker_runs_its_negative_oracle() {
    let root = env!("CARGO_MANIFEST_DIR");
    let checker = format!("{root}/ci/check_learning_loop_trace.py");
    let mut last_not_found = None;
    let output: Output = ["python3", "python"]
        .into_iter()
        .find_map(|interpreter| {
            match Command::new(interpreter)
                .arg(&checker)
                .current_dir(root)
                .output()
            {
                Ok(output) => Some(output),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    last_not_found = Some(format!("{interpreter}: {error}"));
                    None
                }
                Err(error) => panic!("failed to launch {interpreter}: {error}"),
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "Python 3 is required to execute the learning-loop trace oracle ({})",
                last_not_found.unwrap_or_else(|| "no interpreter attempted".to_owned())
            )
        });
    assert!(
        output.status.success(),
        "offline checker rejected its required fixtures; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn offline_report_consumes_the_existing_synthetic_oracle_without_agent_claims() {
    let root = env!("CARGO_MANIFEST_DIR");
    let reporter = format!("{root}/ci/report_learning_loop.py");
    let output = ["python3", "python"]
        .into_iter()
        .find_map(|interpreter| {
            match Command::new(interpreter)
                .arg(&reporter)
                .current_dir(root)
                .output()
            {
                Ok(output) => Some(output),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("failed to launch {interpreter}: {error}"),
            }
        })
        .expect("Python 3 is required for the offline learning-loop report");
    assert!(
        output.status.success(),
        "offline report failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse offline report");
    assert_eq!(report["synthetic_contract_oracle"]["cases"], 12);
    assert_eq!(report["synthetic_contract_oracle"]["negative_fixtures"], 7);
    assert_eq!(report["synthetic_contract_oracle"]["status"], "PASS");
    assert_eq!(report["levels"]["native"]["status"], "NOT_TESTED");
    assert_eq!(
        report["levels"]["native"]["reason"],
        "report_does_not_execute_native_runtime_checks"
    );
    assert_eq!(report["levels"]["agent"]["status"], "NOT_TESTED");
    assert_eq!(report["levels"]["human"]["status"], "NOT_TESTED");
}
