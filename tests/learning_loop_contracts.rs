//! Historical synthetic teaching-fixture checks only; these do not execute or
//! establish acceptance of learner-model/v2 tools or a live subscription host.
//! The offline Python trace-contract checker validates the synthetic LEARN-257
//! learning-loop cases; these tests run it and its report. Semantic decisions
//! remain explicit fixture labels; nothing here grades prose.

use std::process::{Command, Output};

use serde_json::Value;

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
