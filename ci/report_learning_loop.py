#!/usr/bin/env python3
"""Build a privacy-bounded offline learning-loop evaluation report.

The report validates normalized, content-free traces. Synthetic traces test the
evaluator only; authorized saved traces can describe observed agent behavior.
Neither trace kind establishes human learning or native implementation proof.
"""

import argparse
import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from ci import check_learning_loop_trace as trace_oracle

DEFAULT_SCHEMA = ROOT / "tests/fixtures/learning_loop/metrics.schema.json"
DEFAULT_BASELINE = ROOT / "tests/fixtures/learning_loop/baseline.json"
MAX_EVENTS = 256
MAX_DIAGNOSTICS = 20
ID_PATTERN = re.compile(r"[a-z][a-z0-9-]{0,23}\Z")
DELIVERY_PATTERN = re.compile(r"d-[a-z0-9]{1,16}\Z")
SHA_PATTERN = re.compile(r"[0-9a-f]{40}\Z")
METRICS_SCHEMA_VERSION = "learning-loop-metrics/v2"
REPORT_SCHEMA_VERSION = "learning-loop-report/v2"
METRICS_SCHEMA_CONTRACT = {
    "schema_version": METRICS_SCHEMA_VERSION,
    "validator": "finite-typed-learning-loop-contract",
}

EVENT_FIELDS = {
    "capture": {"kind", "captured"},
    "task_exposure": {"kind", "task_id", "purpose", "exposed"},
    "independent_attempt": {"kind", "task_id", "present"},
    "mastery_claim": {"kind", "task_id", "claimed"},
    "repair": {"kind", "repair_id", "issue_key", "correction_applied", "recheck_id"},
    "recheck": {"kind", "recheck_id", "issue_key", "task_id", "independent", "attempt_observed", "passed"},
    "next_action": {"kind", "repair_id", "updated", "action"},
    "coverage": {"kind", "planned_units", "addressed_units", "remaining_units"},
    "pause": {"kind", "requested", "respected", "post_pause_delivery"},
    "delivery": {"kind", "delivery_key"},
}


class ReportError(ValueError):
    """Input does not satisfy the published metrics contract."""


def _expect_mapping(value, fields, location):
    if not isinstance(value, dict) or set(value) != fields:
        raise ReportError(f"{location} fields do not match the versioned contract")


def _expect_bool(value, location):
    if type(value) is not bool:
        raise ReportError(f"{location} must be a boolean")


def _expect_labels(values, location, *, allow_empty=True):
    if not isinstance(values, list) or (not allow_empty and not values) or len(values) > 1000:
        raise ReportError(f"{location} must be a bounded label array")
    if any(not isinstance(item, str) or not ID_PATTERN.fullmatch(item) for item in values):
        raise ReportError(f"{location} contains an invalid fixture-local label")


def load_json(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ReportError("input JSON could not be read") from error


def validate_trace(trace, schema):
    if not isinstance(schema, dict) or schema != METRICS_SCHEMA_CONTRACT:
        raise ReportError("unsupported metrics schema version or vocabulary")
    _expect_mapping(schema, set(METRICS_SCHEMA_CONTRACT), "metrics schema")
    _expect_mapping(trace, {"schema_version", "source_kind", "saved_trace_authorized", "events"}, "trace")
    if trace["schema_version"] != METRICS_SCHEMA_VERSION:
        raise ReportError("unsupported trace schema version")
    if not isinstance(trace["source_kind"], str) or trace["source_kind"] not in {"synthetic", "authorized_saved"}:
        raise ReportError("unsupported trace source kind")
    _expect_bool(trace["saved_trace_authorized"], "saved_trace_authorized")
    events = trace["events"]
    if not isinstance(events, list) or len(events) > MAX_EVENTS:
        raise ReportError("trace events must be a bounded array")
    if trace["source_kind"] == "authorized_saved" and trace["saved_trace_authorized"] is not True:
        raise ReportError("saved trace requires explicit authorization")
    task_purposes = {}
    for event in events:
        if not isinstance(event, dict) or not isinstance(event.get("kind"), str) or event["kind"] not in EVENT_FIELDS:
            raise ReportError("trace contains an unsupported event kind")
        _expect_mapping(event, EVENT_FIELDS[event["kind"]], "trace event")
        kind = event["kind"]
        label_fields = {"task_id", "repair_id", "issue_key", "recheck_id"}
        if any(field in event and (not isinstance(event[field], str) or not ID_PATTERN.fullmatch(event[field])) for field in label_fields):
            raise ReportError("trace identifiers must be fixture-local labels")
        if "delivery_key" in event and (not isinstance(event["delivery_key"], str) or not DELIVERY_PATTERN.fullmatch(event["delivery_key"])):
            raise ReportError("delivery keys must be fixture-local labels")
        bool_fields = {
            "capture": ("captured",),
            "task_exposure": ("exposed",),
            "independent_attempt": ("present",),
            "mastery_claim": ("claimed",),
            "repair": ("correction_applied",),
            "recheck": ("independent", "attempt_observed", "passed"),
            "next_action": ("updated",),
            "pause": ("requested", "respected", "post_pause_delivery"),
        }
        for field in bool_fields.get(kind, ()):
            _expect_bool(event[field], f"{kind}.{field}")
        if kind == "task_exposure":
            if not isinstance(event["purpose"], str) or event["purpose"] not in {"guided", "independent"}:
                raise ReportError("task purpose must be guided or independent")
            task_purposes[event["task_id"]] = event["purpose"]
        elif kind == "recheck":
            if event["task_id"] not in task_purposes:
                raise ReportError("recheck task purpose is missing")
        elif kind == "next_action" and (not isinstance(event["action"], str) or event["action"] not in {"repair_smaller", "repair_same", "advance", "hold"}):
            raise ReportError("unsupported next-action kind")
        elif kind == "coverage":
            for field in ("planned_units", "addressed_units", "remaining_units"):
                _expect_labels(event[field], f"coverage.{field}", allow_empty=field != "planned_units")
    return trace


def validate_baseline(baseline):
    """Validate versioned aggregate evidence as data, not production constants."""
    required = {
        "baseline_version", "issue", "source", "inspected_source_sha", "provenance_limitations", "claim_boundaries", "turns", "nested_executions",
        "learning_step_calls", "checkpoints", "general_practice",
        "provisional_interpretations", "calls_with_rejected_components",
        "continuation_message_numbers", "model_split", "canonical_verified_evidence",
        "observations_nonempty", "checkpoints_nonempty",
    }
    if not isinstance(baseline, dict) or set(baseline) != required:
        raise ReportError("baseline fields do not match the versioned aggregate contract")
    if baseline["baseline_version"] != "learning-loop-baseline/v1":
        raise ReportError("unsupported learning-loop baseline version")
    if type(baseline["issue"]) is not int or baseline["issue"] < 1 or baseline["source"] != "aggregate_issue_observations":
        raise ReportError("baseline provenance is invalid")
    allowed_limitations = {"aggregate_only", "no_transcript_or_private_host_data", "partial_source_capture"}
    allowed_boundaries = {"no_canonical_verified_evidence", "no_retention_or_transfer_measurement", "no_actual_host_run"}
    if (not isinstance(baseline["provenance_limitations"], list)
            or any(not isinstance(item, str) for item in baseline["provenance_limitations"])
            or any(item not in allowed_limitations for item in baseline["provenance_limitations"])
            or not isinstance(baseline["claim_boundaries"], list)
            or any(not isinstance(item, str) for item in baseline["claim_boundaries"])
            or any(item not in allowed_boundaries for item in baseline["claim_boundaries"])):
        raise ReportError("baseline provenance limitations or claim boundaries are invalid")
    source_sha = baseline["inspected_source_sha"]
    if source_sha is not None and (not isinstance(source_sha, str) or not SHA_PATTERN.fullmatch(source_sha)):
        raise ReportError("inspected source SHA must be a full commit hash or null")
    integer_fields = ("turns", "nested_executions", "learning_step_calls", "checkpoints", "provisional_interpretations", "calls_with_rejected_components", "canonical_verified_evidence")
    if any(type(baseline[field]) is not int or baseline[field] < 0 for field in integer_fields):
        raise ReportError("baseline counts must be nonnegative integers")
    practice = baseline["general_practice"]
    if not isinstance(practice, dict) or set(practice) != {"total", "invalidated", "answered"} or any(type(value) is not int or value < 0 for value in practice.values()):
        raise ReportError("general practice aggregate is invalid")
    if practice["invalidated"] + practice["answered"] > practice["total"]:
        raise ReportError("general practice counts are inconsistent")
    continuations = baseline["continuation_message_numbers"]
    if not isinstance(continuations, list) or len(continuations) > 256 or any(type(value) is not int or value < 1 for value in continuations):
        raise ReportError("continuation message numbers are invalid")
    if continuations != sorted(set(continuations)):
        raise ReportError("continuation message numbers must be unique and ordered")
    model_split = baseline["model_split"]
    model_key_pattern = re.compile(r"[a-z][a-z0-9.-]{0,39}/[a-z][a-z0-9-]{0,15}\Z")
    if not isinstance(model_split, dict) or not model_split or any(not isinstance(key, str) or not model_key_pattern.fullmatch(key) for key in model_split) or any(type(value) is not int or value < 0 for value in model_split.values()):
        raise ReportError("model split is invalid")
    if any(type(baseline[field]) is not bool for field in ("observations_nonempty", "checkpoints_nonempty")):
        raise ReportError("baseline presence indicators must be booleans")
    if sum(model_split.values()) != baseline["turns"]:
        raise ReportError("baseline model split must account for all turns")
    return baseline


def _event(event, kind):
    return event["kind"] == kind


def _check(code, status, observed):
    return {"code": code, "status": status, "observed": observed}


def evaluate_trace(trace):
    """Evaluate only explicit event facts, returning fixed codes and counts."""
    events = trace["events"]
    captures = [event for event in events if _event(event, "capture") and event["captured"]]
    exposures = [event for event in events if _event(event, "task_exposure") and event["exposed"]]
    claims = [event for event in events if _event(event, "mastery_claim") and event["claimed"]]
    indexed_repairs = [(index, event) for index, event in enumerate(events) if _event(event, "repair")]
    rechecks = [(index, event) for index, event in enumerate(events) if _event(event, "recheck")]
    next_actions = [(index, event) for index, event in enumerate(events) if _event(event, "next_action")]
    coverage = [event for event in events if _event(event, "coverage")]
    pauses = [event for event in events if _event(event, "pause")]
    deliveries = [event for event in events if _event(event, "delivery")]

    checks = []
    checks.append(_check("capture", "PASS" if captures else "NOT_OBSERVABLE", len(captures)))

    prior_attempts = set()
    false_mastery = False
    for event in events:
        if _event(event, "independent_attempt") and event["present"]:
            prior_attempts.add(event["task_id"])
        elif _event(event, "mastery_claim") and event["claimed"] and event["task_id"] not in prior_attempts:
            false_mastery = True
    exposure_observed = bool(exposures or claims)
    checks.append(_check("exposure_is_not_mastery", "FAIL" if false_mastery else ("PASS" if exposure_observed else "NOT_OBSERVABLE"), len(exposures) + len(claims)))

    repair_observation = []
    next_action_response = []
    criterion_support = []
    for repair_index, repair in indexed_repairs:
        if not repair["correction_applied"]:
            repair_observation.append("FAIL")
            next_action_response.append("FAIL")
            criterion_support.append("FAIL")
            continue
        linked_rechecks = [
            (index, event) for index, event in rechecks
            if index > repair_index
            and event["recheck_id"] == repair["recheck_id"]
            and event["issue_key"] == repair["issue_key"]
        ]
        if not linked_rechecks:
            repair_observation.append("FAIL")
            next_action_response.append("FAIL")
            criterion_support.append("NOT_OBSERVABLE")
            continue
        recheck_index, recheck = linked_rechecks[0]
        purpose = next((event["purpose"] for event in reversed(events[:recheck_index])
                        if _event(event, "task_exposure") and event["task_id"] == recheck["task_id"]), None)
        purpose_matches = purpose is not None and recheck["independent"] == (purpose == "independent")
        if not recheck["attempt_observed"]:
            repair_observation.append("NOT_OBSERVABLE")
            next_action_response.append("NOT_OBSERVABLE")
            criterion_support.append("NOT_OBSERVABLE")
            continue

        repair_observation.append("PASS" if purpose_matches else "FAIL")
        actions = [event for index, event in next_actions
                   if index > recheck_index and event["repair_id"] == repair["repair_id"]]
        action = actions[0] if actions else None
        if not purpose_matches or action is None or not action["updated"]:
            next_action_response.append("FAIL")
        elif not recheck["passed"]:
            next_action_response.append("PASS" if action["action"] in {"repair_smaller", "hold"} else "FAIL")
        else:
            next_action_response.append("PASS" if action["action"] in {"advance", "hold"} else "FAIL")
        criterion_support.append("PASS" if recheck["passed"] and purpose == "independent" and purpose_matches else "FAIL")

    def aggregate_status(statuses):
        if "FAIL" in statuses:
            return "FAIL"
        if "PASS" in statuses:
            return "PASS"
        return "NOT_OBSERVABLE"

    checks.extend([
        _check("repair_followed_by_observed_check", aggregate_status(repair_observation), len(indexed_repairs)),
        _check("next_action_responds_to_check", aggregate_status(next_action_response), len(indexed_repairs)),
        _check("criterion_supported_after_repair", aggregate_status(criterion_support), len(indexed_repairs)),
    ])

    coverage_accounting_failed = False
    coverage_incomplete = False
    coverage_unaccounted = 0
    coverage_summary = []
    for event in coverage:
        planned = event["planned_units"]
        addressed = event["addressed_units"]
        remaining = event["remaining_units"]
        planned_set, addressed_set, remaining_set = set(planned), set(addressed), set(remaining)
        duplicated = len(planned_set) != len(planned) or len(addressed_set) != len(addressed) or len(remaining_set) != len(remaining)
        missing = planned_set - addressed_set - remaining_set
        unexpected = (addressed_set | remaining_set) - planned_set
        overlap = addressed_set & remaining_set
        accounted = not (duplicated or missing or unexpected or overlap)
        if not accounted:
            coverage_accounting_failed = True
            coverage_unaccounted += len(missing | unexpected | overlap) + int(duplicated)
        if remaining_set:
            coverage_incomplete = True
        coverage_summary.append({
            "planned_units": len(planned),
            "addressed_units": len(addressed),
            "remaining_units": len(remaining),
            "accounted": accounted,
        })
    checks.append(_check(
        "coverage_units_accounted",
        "FAIL" if coverage_accounting_failed else ("PASS" if coverage else "NOT_OBSERVABLE"),
        coverage_unaccounted,
    ))
    checks.append(_check(
        "planned_coverage_complete",
        "FAIL" if coverage_incomplete or coverage_accounting_failed else ("PASS" if coverage else "NOT_OBSERVABLE"),
        sum(len(event["remaining_units"]) for event in coverage),
    ))

    pause_violated = any(event["requested"] and (not event["respected"] or event["post_pause_delivery"]) for event in pauses)
    checks.append(_check("pause_respected", "FAIL" if pause_violated else ("PASS" if pauses else "NOT_OBSERVABLE"), len(pauses)))

    delivery_keys = [event["delivery_key"] for event in deliveries]
    repeated_delivery = len(delivery_keys) != len(set(delivery_keys))
    checks.append(_check("delivery_not_repeated", "FAIL" if repeated_delivery else ("PASS" if deliveries else "NOT_OBSERVABLE"), len(deliveries)))

    if not captures:
        agent_status = "NOT_OBSERVABLE"
        agent_reason = "capture_missing"
    else:
        # Caller-supplied source and authorization fields are assertions, not
        # verified host/model provenance. No receipt/snapshot adapter currently
        # binds this offline trace format to an actual runtime.
        agent_status = "NOT_TESTED"
        agent_reason = (
            "synthetic_trace_is_evaluator_only"
            if trace["source_kind"] == "synthetic"
            else "no_adapter_for_verified_runtime_receipts_or_snapshots"
        )

    diagnostics = []
    for item in checks:
        if item["status"] in {"FAIL", "NOT_OBSERVABLE"}:
            diagnostics.append({"code": item["code"] if item["status"] == "FAIL" else item["code"] + "_unobserved", "count": item["observed"]})
    diagnostics = diagnostics[:MAX_DIAGNOSTICS]

    return {
        "levels": {
            "native": {"status": "NOT_TESTED", "reason": "trace_report_does_not_execute_native_checks"},
            "agent": {"status": agent_status, "reason": agent_reason},
            "human": {"status": "NOT_TESTED", "reason": "no_human_learning_measurement"},
        },
        "checks": [{"code": item["code"], "status": item["status"]} for item in checks],
        "coverage": coverage_summary,
        "diagnostics": diagnostics,
        "counts": {"events": len(events), "captures": len(captures), "deliveries": len(deliveries)},
        "limitations": [
            "A schema-valid trace establishes only the recorded facts.",
            "Synthetic traces test the evaluator and do not establish agent execution.",
            "No adapter binds this trace format to verified runtime receipts or snapshots.",
            "No trace establishes human retention or transfer without a separate human measure.",
        ],
    }


def run_synthetic_contract_oracle():
    """Check the trace contract without claiming that native runtime tests ran."""
    try:
        fixture = trace_oracle.load_json(trace_oracle.FIXTURE)
        cases = trace_oracle.validate_fixture(fixture)
        failures = trace_oracle.negative_self_check(cases, fixture["negative_fixtures"])
    except (trace_oracle.ContractError, KeyError, TypeError, ValueError):
        return {"status": "FAIL", "reason": "synthetic_learning_loop_contract_oracle_failed",
                "cases": 0, "negative_fixtures": 0}
    if failures:
        return {"status": "FAIL", "reason": "synthetic_learning_loop_contract_oracle_failed",
                "cases": len(cases), "negative_fixtures": len(fixture["negative_fixtures"])}
    return {"status": "PASS", "reason": "synthetic_learning_loop_contract_oracle_passed",
            "cases": len(cases), "negative_fixtures": len(fixture["negative_fixtures"])}


def build_report(trace=None, baseline=None, schema=None):
    schema = load_json(DEFAULT_SCHEMA) if schema is None else schema
    baseline = validate_baseline(baseline if baseline is not None else load_json(DEFAULT_BASELINE))
    synthetic_oracle = run_synthetic_contract_oracle()
    report = {
        "report_schema": REPORT_SCHEMA_VERSION,
        "baseline": baseline,
        "levels": {
            "native": {"status": "NOT_TESTED", "reason": "report_does_not_execute_native_runtime_checks"},
            "agent": {"status": "NOT_TESTED", "reason": "no_trace_supplied"},
            "human": {"status": "NOT_TESTED", "reason": "no_human_learning_measurement"},
        },
        "synthetic_contract_oracle": synthetic_oracle,
        "trace_result": None,
    }
    if trace is not None:
        validate_trace(trace, schema)
        report["trace_result"] = evaluate_trace(trace)
        report["trace_result"]["levels"]["native"] = report["levels"]["native"]
        report["levels"] = report["trace_result"]["levels"]
    return report


def render_markdown(report):
    lines = ["# Learning-loop evaluation report", "", "## Evidence levels", "", "| Level | Status | Reason |", "|---|---|---|"]
    result = report.get("trace_result")
    levels = report["levels"]
    for name, item in levels.items():
        lines.append(f"| {name} | {item['status']} | {item['reason']} |")
    oracle = report["synthetic_contract_oracle"]
    lines.extend([
        "", "## Synthetic contract oracle", "",
        f"Status: **{oracle['status']}** (`{oracle['reason']}`); "
        f"{oracle['cases']} synthetic cases, {oracle['negative_fixtures']} negative fixtures. "
        "This status measures evaluator self-tests only.",
    ])
    if result is not None:
        lines.extend(["", "## Contract checks", "", "| Check | Status |", "|---|---|"])
        lines.extend(f"| {item['code']} | {item['status']} |" for item in result["checks"])
        lines.extend(["", "## Bounded diagnostics", ""])
        if result["diagnostics"]:
            lines.extend(f"- `{item['code']}`: {item['count']}" for item in result["diagnostics"])
        else:
            lines.append("No observed contract violations.")
        if result["coverage"]:
            lines.extend(["", "## Coverage", "", "| Planned | Addressed | Remaining | Accounted |", "|---:|---:|---:|---|"])
            lines.extend(
                f"| {item['planned_units']} | {item['addressed_units']} | {item['remaining_units']} | {str(item['accounted']).lower()} |"
                for item in result["coverage"]
            )
    lines.extend(["", "## Baseline", "", "Aggregate facts only; no transcript, host identifier or private content is included.", ""])
    for key, value in report["baseline"].items():
        lines.append(f"- **{key}:** `{json.dumps(value, sort_keys=True)}`")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trace", type=Path, help="content-free normalized trace JSON")
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--format", choices=("json", "markdown"), default="json")
    args = parser.parse_args(argv)
    try:
        schema = load_json(args.schema)
        baseline = load_json(args.baseline)
        trace = load_json(args.trace) if args.trace else None
        report = build_report(trace, baseline, schema)
    except ReportError as error:
        print(str(error), file=sys.stderr)
        return 2
    if args.format == "markdown":
        sys.stdout.write(render_markdown(report))
    else:
        json.dump(report, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
