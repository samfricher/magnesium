//! Historical synthetic teaching-fixture checks only; these do not execute or
//! establish acceptance of learner-model/v2 tools or a live subscription host.
//! Sparse-history multi-turn teaching regressions (LEARN-246).
//!
//! The loader in this file projects only an explicit allowlist of
//! `model_input` fields into model-visible prompts. The teaching gate consumes
//! observable decisions recorded in synthetic traces; it does not score
//! phrases, headings, or question counts.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};

const MODEL_INPUT_FIELDS: &[&str] = &["source", "turns"];
const EVALUATOR_ONLY_KEYS: &[&str] = &[
    "must_preserve",
    "forbidden",
    "required_decision",
    "diagnoses",
];
const REQUIRED_FAMILIES: &[&str] = &[
    "cold-start",
    "paired-history",
    "theory-vs-algebra",
    "dependency-recurrence",
    "badly-pitched-task",
    "opt-out",
    "cross-domain-control",
];
const FAULT_KINDS: &[&str] = &[
    "advanced_before_prerequisite",
    "repeated_ineffective_method",
    "unfamiliar_became_mastered_without_evidence",
    "answer_exposure_not_qualified",
    "answer_exposure_counted_as_learning",
    "unnecessary_probe",
];

fn fixture_value() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/interaction/statistics/chapter7_sparse.json");
    serde_json::from_slice(&fs::read(path).expect("read chapter7 sparse fixture"))
        .expect("parse chapter7 sparse fixture")
}

fn fixture_text(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelInput {
    source: String,
    turns: Vec<String>,
}

fn project_model_input(case: &Value) -> Result<ModelInput, String> {
    let object = case
        .get("model_input")
        .and_then(Value::as_object)
        .ok_or_else(|| "case has no model_input object".to_owned())?;
    for key in object.keys() {
        if !MODEL_INPUT_FIELDS.contains(&key.as_str()) {
            return Err(format!("model_input field {key:?} is not allowlisted"));
        }
    }
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| "model_input.source must be a string".to_owned())?;
    let turns = object
        .get("turns")
        .and_then(Value::as_array)
        .ok_or_else(|| "model_input.turns must be an array".to_owned())?;
    let turns = turns
        .iter()
        .map(|turn| {
            turn.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "model_input.turns entries must be strings".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if turns.is_empty() {
        return Err("model_input.turns must not be empty".to_owned());
    }
    Ok(ModelInput {
        source: source.to_owned(),
        turns,
    })
}

fn construct_prompt(input: &ModelInput, up_to_turn: usize) -> Result<String, String> {
    let visible = input
        .turns
        .get(..=up_to_turn)
        .ok_or_else(|| format!("turn {up_to_turn} is not in this case"))?;
    let mut prompt = format!("Source: {}\n", input.source);
    for (index, turn) in visible.iter().enumerate() {
        prompt.push_str(&format!("Learner turn {}: {turn}\n", index + 1));
    }
    Ok(prompt)
}

fn evaluator_only_text(case: &Value) -> Vec<String> {
    let mut strings = Vec::new();
    let Some(evaluator) = case.get("evaluator_only").and_then(Value::as_object) else {
        return strings;
    };
    for key in EVALUATOR_ONLY_KEYS {
        if let Some(value) = evaluator.get(*key) {
            strings.push((*key).to_owned());
            collect_scalars(value, &mut strings);
        }
    }
    strings
}

fn collect_scalars(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_scalars(item, output);
            }
        }
        Value::Object(fields) => {
            for item in fields.values() {
                collect_scalars(item, output);
            }
        }
        _ => {}
    }
}

#[test]
fn model_prompt_projection_uses_only_allowlisted_fields() {
    let fixture = fixture_value();
    let cases = fixture["cases"].as_array().expect("fixture cases");
    assert!(!cases.is_empty());
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let input = project_model_input(case).unwrap_or_else(|error| panic!("{id}: {error}"));
        let evaluator_text = evaluator_only_text(case);
        assert!(!evaluator_text.is_empty(), "{id}: evaluator text was empty");
        for up_to_turn in 0..input.turns.len() {
            let prompt = construct_prompt(&input, up_to_turn).expect("construct prompt");
            assert!(
                prompt.contains(&input.source),
                "{id}: prompt must name the allowlisted source"
            );
            for (index, turn) in input.turns.iter().enumerate() {
                if index <= up_to_turn {
                    assert!(
                        prompt.contains(turn),
                        "{id}: prompt for turn {up_to_turn} omitted fed turn {index}"
                    );
                } else {
                    assert!(
                        !prompt.contains(turn),
                        "{id}: prompt for turn {up_to_turn} leaked future turn {index}"
                    );
                }
            }
            for text in &evaluator_text {
                assert!(
                    !prompt.contains(text),
                    "{id}: evaluator-only text {text:?} leaked into the prompt"
                );
            }
        }
    }
    assert_eq!(
        fixture["model_input_fields"],
        json!(MODEL_INPUT_FIELDS),
        "fixture allowlist must match the loader allowlist"
    );
}

#[test]
fn loader_denies_nested_and_unknown_evaluator_fields() {
    let poisoned = json!({
        "id": "poisoned",
        "model_input": {
            "source": "chapter7_sources.md",
            "turns": ["Teach me."],
            "evaluator_only": {"required_decision": "secret"}
        },
        "evaluator_only": {"must_preserve": ["secret"], "forbidden": [], "required_decision": "secret", "diagnoses": []}
    });
    assert!(project_model_input(&poisoned).is_err());

    let nested_object = json!({
        "id": "nested",
        "model_input": {
            "source": {"document": "chapter7_sources.md", "diagnoses": ["secret"]},
            "turns": ["Teach me."]
        }
    });
    assert!(project_model_input(&nested_object).is_err());

    let nested_turn = json!({
        "id": "nested-turn",
        "model_input": {
            "source": "chapter7_sources.md",
            "turns": [{"text": "Teach me.", "required_decision": "secret"}]
        }
    });
    assert!(project_model_input(&nested_turn).is_err());

    let unknown_scalar = json!({
        "id": "unknown-scalar",
        "model_input": {
            "source": "chapter7_sources.md",
            "turns": ["Teach me."],
            "diagnoses": ["secret"]
        }
    });
    assert!(project_model_input(&unknown_scalar).is_err());

    let valid = json!({
        "id": "valid",
        "model_input": {"source": "chapter7_sources.md", "turns": ["Teach me."]}
    });
    let input = project_model_input(&valid).expect("allowlisted shape is accepted");
    assert_eq!(input.source, "chapter7_sources.md");
    assert_eq!(input.turns, vec!["Teach me.".to_owned()]);
    let prompt = construct_prompt(&input, 0).expect("construct prompt");
    assert_eq!(
        prompt,
        "Source: chapter7_sources.md\nLearner turn 1: Teach me.\n"
    );
}

#[test]
fn required_case_families_are_present_and_versioned() {
    let fixture = fixture_value();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["fixture_version"], "statistics/chapter7/v1");
    assert_eq!(fixture["synthetic"], true);
    assert_eq!(fixture["private_material"], false);
    assert_eq!(fixture["source"], "chapter7_sources.md");
    let cases = fixture["cases"].as_array().expect("fixture cases");
    let mut ids = BTreeSet::new();
    let mut families = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id.to_owned()), "duplicate case id {id}");
        families.insert(case["family"].as_str().expect("case family").to_owned());
        let input = project_model_input(case).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(input.source, "chapter7_sources.md", "{id}: fixture source");
        let evaluator = case["evaluator_only"].as_object().expect("evaluator_only");
        for key in EVALUATOR_ONLY_KEYS {
            let value = evaluator
                .get(*key)
                .unwrap_or_else(|| panic!("{id}: missing {key}"));
            let present = match value {
                Value::Array(items) => !items.is_empty(),
                Value::String(text) => !text.trim().is_empty(),
                _ => panic!("{id}: {key} has an unexpected shape"),
            };
            assert!(present, "{id}: {key} must be non-empty");
        }
    }
    for family in REQUIRED_FAMILIES {
        assert!(families.contains(*family), "missing case family {family}");
    }
    let paired: Vec<&Value> = cases
        .iter()
        .filter(|case| case["family"] == "paired-history")
        .collect();
    assert_eq!(paired.len(), 2, "paired history requires exactly two cases");
    let first_turn = paired[0]["model_input"]["turns"][0].clone();
    let second_turn = paired[1]["model_input"]["turns"][0].clone();
    assert_ne!(first_turn, second_turn, "paired histories must differ");
    let final_turn = json!("What is an MGF?");
    assert_eq!(paired[0]["model_input"]["turns"][1], final_turn);
    assert_eq!(paired[1]["model_input"]["turns"][1], final_turn);
    assert_eq!(paired[0]["evaluator_only"]["paired_with"], paired[1]["id"]);
    assert_eq!(paired[1]["evaluator_only"]["paired_with"], paired[0]["id"]);
    for id in [
        "opt-out-no-quiz",
        "opt-out-full-solution",
        "opt-out-stop",
        "opt-out-operational-work",
        "opt-out-tracking-off",
        "opt-out-third-party-claim",
        "cross-domain-code-trace",
        "cross-domain-evidence-argument",
    ] {
        assert!(ids.contains(id), "missing required case {id}");
    }
    for case in cases
        .iter()
        .filter(|case| case["family"] == "dependency-recurrence")
    {
        assert!(
            case["evaluator_only"]["fresh_host_after_turn"].is_u64(),
            "dependency recurrence must record a fresh-host boundary"
        );
    }
}

#[test]
fn synthetic_source_covers_the_required_teaching_content() {
    let source = fixture_text("tests/fixtures/interaction/statistics/chapter7_sources.md");
    assert!(
        source.lines().count() > 150,
        "the synthetic source must be substantial"
    );
    for heading in [
        "central limit theorem",
        "continuity correction and cell boundaries",
        "normal approximation to a discrete count",
        "Stirling",
        "midpoint",
        "Binomial",
    ] {
        assert!(
            source.to_lowercase().contains(&heading.to_lowercase()),
            "synthetic source is missing required content {heading:?}"
        );
    }
    assert!(
        source.contains("Original synthetic") && source.contains("no private slides"),
        "synthetic source must declare its synthetic provenance"
    );
}

#[derive(Debug, Default, Deserialize)]
struct Reports {
    #[serde(default)]
    unfamiliar: Vec<String>,
    #[serde(default)]
    ineffective_method: Option<IneffectiveMethod>,
}

#[derive(Debug, Deserialize)]
struct IneffectiveMethod {
    method: String,
    target: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TraceEvent {
    Learner {
        turn: u64,
        text: String,
        #[serde(default)]
        demonstrates: Vec<String>,
        #[serde(default)]
        reports: Reports,
    },
    Tutor {
        turn: u64,
        decision: String,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        requires: Vec<String>,
        #[serde(default)]
        establishes: Vec<String>,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        probe_target: Option<String>,
        #[serde(default)]
        presents_new_representation: bool,
        #[serde(default)]
        assistance_call: bool,
        #[serde(default)]
        qualifies_original_task: bool,
        #[serde(default)]
        independent_attempt: bool,
        #[serde(default)]
        reconnects_goal: bool,
        #[serde(default)]
        offers_learner_work: bool,
        #[serde(default)]
        treats_provided_answer_as_learning: bool,
        #[serde(default)]
        discards_correct_work: bool,
    },
    Receipt {
        operation: String,
        status: String,
        reason: String,
        canonical_row_delta: i64,
    },
    HostBoundary {
        turn: u64,
        note: String,
    },
}

#[derive(Debug, Deserialize)]
struct TraceExpectation {
    teaching_gate: String,
    native_receipt_gate: String,
    fault_kinds: Vec<String>,
    unnecessary_probes: usize,
    identified_missing_link: bool,
    preserved_correct_work: bool,
    reconnected_to_goal: bool,
    offered_learner_work: bool,
}

#[derive(Debug, Deserialize)]
struct Trace {
    id: String,
    description: String,
    missing_link: String,
    events: Vec<TraceEvent>,
    expected: TraceExpectation,
}

#[derive(Debug, Default)]
struct TeachingReport {
    faults: Vec<String>,
    unnecessary_probes: usize,
    identified_missing_link: bool,
    preserved_correct_work: bool,
    reconnected_to_goal: bool,
    offered_learner_work: bool,
    accepted_receipts: usize,
    rejected_receipts: usize,
}

impl TeachingReport {
    fn teaching_gate_passed(&self) -> bool {
        self.faults.is_empty() && self.unnecessary_probes == 0
    }

    fn native_receipt_gate_passed(&self) -> bool {
        self.accepted_receipts > 0 && self.rejected_receipts == 0
    }
}

fn evaluate_trace(trace: &Trace) -> TeachingReport {
    // Supplied material may sequence instruction; only actual learner work can
    // support a capability claim.
    let mut demonstrated = BTreeSet::new();
    let mut supplied = BTreeSet::new();
    let mut unfamiliar = BTreeSet::new();
    let mut ineffective = BTreeSet::new();
    let mut faults = BTreeSet::new();
    let mut answer_exposed_tasks = BTreeSet::new();
    let mut report = TeachingReport::default();
    for event in &trace.events {
        match event {
            TraceEvent::Learner {
                demonstrates,
                reports,
                ..
            } => {
                demonstrated.extend(demonstrates.iter().cloned());
                unfamiliar.extend(reports.unfamiliar.iter().cloned());
                if let Some(ineffective_method) = &reports.ineffective_method {
                    ineffective.insert(format!(
                        "{}->{}",
                        ineffective_method.method, ineffective_method.target
                    ));
                }
            }
            TraceEvent::Tutor {
                decision,
                target,
                requires,
                establishes,
                method,
                probe_target,
                presents_new_representation,
                assistance_call,
                qualifies_original_task,
                independent_attempt,
                reconnects_goal,
                offers_learner_work,
                treats_provided_answer_as_learning,
                ..
            } => {
                for required in requires {
                    if !demonstrated.contains(required) && !supplied.contains(required) {
                        faults.insert("advanced_before_prerequisite");
                    }
                }
                if decision == "probe"
                    && let Some(probe_target) = probe_target
                    && (demonstrated.contains(probe_target) || supplied.contains(probe_target))
                {
                    report.unnecessary_probes += 1;
                    faults.insert("unnecessary_probe");
                }
                if decision == "assert_mastered"
                    && let Some(target) = target
                    && unfamiliar.contains(target)
                    && !demonstrated.contains(target)
                {
                    faults.insert("unfamiliar_became_mastered_without_evidence");
                }
                if matches!(decision.as_str(), "explain" | "repair")
                    && let Some(method) = method
                    && let Some(target) = target
                    && ineffective.contains(&format!("{method}->{target}"))
                    && !presents_new_representation
                {
                    faults.insert("repeated_ineffective_method");
                }
                if decision == "repair"
                    && let Some(target) = target
                    && target == &trace.missing_link
                    && establishes.contains(target)
                {
                    report.identified_missing_link = true;
                }
                if decision == "expose_full_answer" && !assistance_call && !qualifies_original_task
                {
                    faults.insert("answer_exposure_not_qualified");
                }
                if decision == "expose_full_answer"
                    && let Some(target) = target
                {
                    answer_exposed_tasks.insert(target.clone());
                }
                if decision == "treat_as_learning"
                    && *treats_provided_answer_as_learning
                    && !independent_attempt
                    && let Some(target) = target
                    && answer_exposed_tasks.contains(target)
                {
                    faults.insert("answer_exposure_counted_as_learning");
                }
                supplied.extend(establishes.iter().cloned());
                report.reconnected_to_goal |= reconnects_goal;
                report.offered_learner_work |= offers_learner_work;
            }
            TraceEvent::Receipt {
                status,
                operation,
                reason,
                canonical_row_delta,
            } => {
                assert!(
                    matches!(operation.as_str(), "learning_step" | "practice"),
                    "unknown receipt operation {operation}"
                );
                assert!(!reason.is_empty(), "receipt reason must be recorded");
                if status == "committed" {
                    assert!(
                        *canonical_row_delta > 0,
                        "committed receipt {operation} must record a positive canonical delta"
                    );
                    report.accepted_receipts += 1;
                } else {
                    report.rejected_receipts += 1;
                }
            }
            TraceEvent::HostBoundary { .. } => {}
        }
    }
    report.faults = FAULT_KINDS
        .iter()
        .filter(|kind| faults.contains(*kind))
        .map(|kind| (*kind).to_owned())
        .collect();
    report.preserved_correct_work = !trace.events.iter().any(|event| {
        matches!(
            event,
            TraceEvent::Tutor {
                discards_correct_work: true,
                ..
            }
        )
    });
    report
}

fn fixture_traces(fixture: &Value) -> Vec<Trace> {
    serde_json::from_value(fixture["traces"].clone()).expect("parse fixture traces")
}

#[test]
fn trace_fixtures_are_well_formed_and_interleaved() {
    let fixture = fixture_value();
    let traces = fixture_traces(&fixture);
    assert!(!traces.is_empty());
    for trace in &traces {
        assert!(!trace.description.is_empty(), "{}: description", trace.id);
        assert!(!trace.missing_link.is_empty(), "{}: missing link", trace.id);
        let mut pending_learner = false;
        let mut last_turn = 0;
        for event in &trace.events {
            let (turn, is_learner, is_tutor) = match event {
                TraceEvent::Learner {
                    turn,
                    text,
                    reports,
                    ..
                } => {
                    assert!(!text.is_empty(), "{}: learner text", trace.id);
                    if let Some(ineffective_method) = &reports.ineffective_method {
                        assert!(
                            !ineffective_method.reason.is_empty(),
                            "{}: inefficacy report needs a reason",
                            trace.id
                        );
                    }
                    (*turn, true, false)
                }
                TraceEvent::Tutor { turn, decision, .. } => {
                    assert!(!decision.is_empty(), "{}: tutor decision", trace.id);
                    (*turn, false, true)
                }
                TraceEvent::Receipt { .. } => (last_turn, false, false),
                TraceEvent::HostBoundary { turn, note } => {
                    assert!(!note.is_empty(), "{}: host boundary note", trace.id);
                    (*turn, false, false)
                }
            };
            if turn < last_turn {
                panic!("{}: turns must not go backwards", trace.id);
            }
            last_turn = turn;
            if is_learner {
                assert!(
                    !pending_learner,
                    "{}: a learner message must follow an assistant turn",
                    trace.id
                );
                pending_learner = true;
            }
            if is_tutor {
                pending_learner = false;
            }
        }
    }
}

#[test]
fn recorded_traces_match_their_expected_gates_and_measures() {
    let fixture = fixture_value();
    let scoring = &fixture["scoring_rules"];
    assert_eq!(scoring["fault_kinds"], json!(FAULT_KINDS));
    for trace in fixture_traces(&fixture) {
        let report = evaluate_trace(&trace);
        assert_eq!(
            report.faults, trace.expected.fault_kinds,
            "{}: fault kinds",
            trace.id
        );
        assert_eq!(
            report.unnecessary_probes, trace.expected.unnecessary_probes,
            "{}: unnecessary probes",
            trace.id
        );
        assert_eq!(
            report.identified_missing_link, trace.expected.identified_missing_link,
            "{}: identified missing link",
            trace.id
        );
        assert_eq!(
            report.preserved_correct_work, trace.expected.preserved_correct_work,
            "{}: preserved correct work",
            trace.id
        );
        assert_eq!(
            report.reconnected_to_goal, trace.expected.reconnected_to_goal,
            "{}: reconnected to goal",
            trace.id
        );
        assert_eq!(
            report.offered_learner_work, trace.expected.offered_learner_work,
            "{}: offered learner work",
            trace.id
        );
        let expected_teaching_pass = trace.expected.teaching_gate == "pass";
        assert_eq!(
            report.teaching_gate_passed(),
            expected_teaching_pass,
            "{}: teaching gate",
            trace.id
        );
        let expected_native_pass = trace.expected.native_receipt_gate == "pass";
        assert_eq!(
            report.native_receipt_gate_passed(),
            expected_native_pass,
            "{}: native receipt gate",
            trace.id
        );
        assert!(
            ["pass", "fail"].contains(&trace.expected.teaching_gate.as_str()),
            "{}: teaching gate verdict",
            trace.id
        );
    }
}

#[test]
fn negative_trace_fails_teaching_gate_while_native_receipt_gate_passes() {
    let fixture = fixture_value();
    let trace = fixture_traces(&fixture)
        .into_iter()
        .find(|trace| trace.id == "negative-advanced-before-prerequisites")
        .expect("required negative trace");
    let report = evaluate_trace(&trace);
    assert!(
        !report.teaching_gate_passed(),
        "the negative trace must fail the teaching gate"
    );
    assert!(
        report.native_receipt_gate_passed(),
        "accepted capture receipts must still pass the native receipt gate"
    );
    assert!(report.accepted_receipts >= 5);
    for fault in [
        "advanced_before_prerequisite",
        "repeated_ineffective_method",
        "answer_exposure_not_qualified",
        "answer_exposure_counted_as_learning",
    ] {
        assert!(
            report.faults.contains(&fault.to_owned()),
            "the negative trace must record {fault}"
        );
    }
}

#[test]
fn positive_trace_passes_with_zero_unnecessary_probes() {
    let fixture = fixture_value();
    let trace = fixture_traces(&fixture)
        .into_iter()
        .find(|trace| trace.id == "positive-diagnosis-repair-feedback-return")
        .expect("required positive trace");
    let report = evaluate_trace(&trace);
    assert!(
        report.teaching_gate_passed(),
        "the positive trace must pass: {:?}",
        report.faults
    );
    assert_eq!(report.unnecessary_probes, 0);
    assert!(report.identified_missing_link);
    assert!(report.reconnected_to_goal);
    assert!(report.offered_learner_work);
    assert!(report.native_receipt_gate_passed());
}

fn trace_from_value(value: Value) -> Trace {
    serde_json::from_value(value).expect("parse trace value")
}

#[test]
fn gate_sensitivity_matches_the_recorded_failure_modes() {
    let fixture = fixture_value();
    let traces = fixture_traces(&fixture);

    let mut repaired = fixture["traces"]
        .as_array()
        .expect("traces")
        .iter()
        .find(|trace| trace["id"] == "negative-repeated-substitution")
        .cloned()
        .expect("repetition trace");
    for event in repaired["events"].as_array_mut().expect("events") {
        if event["decision"] == "repair" {
            event["method"] = json!("midpoint-cell-cancellation");
            event["presents_new_representation"] = json!(true);
        }
    }
    let report = evaluate_trace(&trace_from_value(repaired));
    assert!(
        !report
            .faults
            .contains(&"repeated_ineffective_method".to_owned()),
        "a changed representation must clear only the repetition fault: {:?}",
        report.faults
    );

    let positive_index = fixture["traces"]
        .as_array()
        .expect("traces")
        .iter()
        .position(|trace| trace["id"] == "positive-diagnosis-repair-feedback-return")
        .expect("positive trace");
    let mut probed = fixture["traces"][positive_index].clone();
    probed["events"]
        .as_array_mut()
        .expect("events")
        .push(json!({
            "kind": "tutor",
            "turn": 5,
            "decision": "probe",
            "probe_target": "taylor-expansion",
            "target": "mgf-as-expectation"
        }));
    let report = evaluate_trace(&trace_from_value(probed));
    assert!(
        report.faults.contains(&"unnecessary_probe".to_owned()),
        "probing an established prerequisite must be recorded"
    );

    let negative = traces
        .iter()
        .find(|trace| trace.id == "negative-advanced-before-prerequisites")
        .expect("negative trace");
    let report = evaluate_trace(negative);
    assert_eq!(report.faults, negative.expected.fault_kinds);
}

#[test]
fn teaching_gate_is_not_hard_coded_to_statistics_vocabulary() {
    let configured = json!({
        "id": "configured-vocabulary",
        "description": "highest-variance prerequisite ordering trace",
        "missing_link": "half-cell-alignment",
        "events": [
            {"kind": "learner", "turn": 1, "text": "a", "reports": {"unfamiliar": ["integration"]}},
            {"kind": "tutor", "turn": 1, "decision": "advance", "target": "lattice-argument", "requires": ["integration"]},
            {"kind": "tutor", "turn": 2, "decision": "assert_mastered", "target": "integration"},
            {"kind": "receipt", "operation": "learning_step", "status": "committed", "reason": "accepted", "canonical_row_delta": 1}
        ],
        "expected": {
            "teaching_gate": "fail",
            "native_receipt_gate": "pass",
            "fault_kinds": ["advanced_before_prerequisite", "unfamiliar_became_mastered_without_evidence"],
            "unnecessary_probes": 0,
            "identified_missing_link": false,
            "preserved_correct_work": true,
            "reconnected_to_goal": false,
            "offered_learner_work": false
        }
    });
    let renamed = json!({
        "id": "renamed-vocabulary",
        "description": "same structure with unrelated identifiers",
        "missing_link": "stack-frame",
        "events": [
            {"kind": "learner", "turn": 1, "text": "a", "reports": {"unfamiliar": ["frame-lifetime"]}},
            {"kind": "tutor", "turn": 1, "decision": "advance", "target": "recursion-depth", "requires": ["frame-lifetime"]},
            {"kind": "tutor", "turn": 2, "decision": "assert_mastered", "target": "frame-lifetime"},
            {"kind": "receipt", "operation": "learning_step", "status": "committed", "reason": "accepted", "canonical_row_delta": 1}
        ],
        "expected": {
            "teaching_gate": "fail",
            "native_receipt_gate": "pass",
            "fault_kinds": ["advanced_before_prerequisite", "unfamiliar_became_mastered_without_evidence"],
            "unnecessary_probes": 0,
            "identified_missing_link": false,
            "preserved_correct_work": true,
            "reconnected_to_goal": false,
            "offered_learner_work": false
        }
    });
    let configured = evaluate_trace(&trace_from_value(configured));
    let renamed = evaluate_trace(&trace_from_value(renamed));
    assert_eq!(configured.faults, renamed.faults);
    assert_eq!(configured.unnecessary_probes, renamed.unnecessary_probes);
    assert_eq!(
        configured.identified_missing_link,
        renamed.identified_missing_link
    );
}

#[test]
fn live_observation_lane_is_explicitly_not_run() {
    let fixture = fixture_value();
    let live = &fixture["live_observation"];
    assert_eq!(live["status"], "NOT_RUN");
    assert_eq!(live["requested"], true);
    assert!(
        live["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("not substituted")),
        "NOT_RUN must state that synthetic traces are not substituted"
    );
    assert_eq!(
        live["required_sequence"],
        json!([
            "diagnosis",
            "repair",
            "learner_response",
            "qualified_feedback",
            "fresh_host_return"
        ])
    );
    assert!(
        live["owner_note"]
            .as_str()
            .is_some_and(|note| note.contains("#171")),
        "value and retention ownership must remain with #171"
    );
    for trace in fixture_traces(&fixture) {
        assert!(
            !trace.description.to_lowercase().contains("live host"),
            "{}: synthetic traces must not claim live-host execution",
            trace.id
        );
    }
}
