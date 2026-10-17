#!/usr/bin/env python3
"""Check synthetic learning-loop contracts and recorded trace facts offline.

This checker compares event receipts, ordered fixture-local inputs and
decisions, claims, and state changes. It never grades prose or infers semantic
correctness from praise, keywords, response length, or tool use.
"""

import argparse
import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests/fixtures/learning_loop/cases.json"
REQUIRED_CASE_IDS = {
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
}


class ContractError(ValueError):
    pass


def load_json(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read JSON from {path}: {error}") from error


def validate_fixture(fixture):
    if fixture.get("schema_version") != "learning-loop/v1" or fixture.get("synthetic") is not True:
        raise ContractError("expected synthetic learning-loop/v1 fixture")
    cases = fixture.get("cases")
    if not isinstance(cases, list):
        raise ContractError("cases must be an array")
    ids = [case.get("id") for case in cases]
    if len(ids) != len(set(ids)):
        raise ContractError("duplicate case id")
    if set(ids) != REQUIRED_CASE_IDS:
        raise ContractError(f"case inventory mismatch: expected {len(REQUIRED_CASE_IDS)} required cases")

    input_ids = set()
    for case in cases:
        for field in ("initial_state", "source_fixture", "inputs", "allowed_actions", "forbidden_claims", "required_state_changes", "reviewed_decisions", "claim_limit"):
            if field not in case:
                raise ContractError(f"{case['id']}: missing {field}")
        if case["source_fixture"] not in {"course_outline.md", "database_scenarios.md"}:
            raise ContractError(f"{case['id']}: source fixture must stay inside the synthetic pack")
        source_path = ROOT / "tests/fixtures/learning_loop" / case["source_fixture"]
        if not source_path.is_file():
            raise ContractError(f"{case['id']}: missing synthetic source fixture {source_path.name}")
        if not case["inputs"] or not case["allowed_actions"] or not case["forbidden_claims"]:
            raise ContractError(f"{case['id']}: inputs, allowed actions, and forbidden claims must be nonempty")
        for item in case["inputs"]:
            identity = item.get("id")
            if not isinstance(identity, str) or not re.fullmatch(r"[a-z][a-z0-9-]*-i[0-9]+", identity):
                raise ContractError(f"{case['id']}: invalid fixture-local input id")
            if identity in input_ids:
                raise ContractError(f"duplicate input id {identity}")
            input_ids.add(identity)
            if item.get("actor") not in {"learner", "fixture"} or not isinstance(item.get("text"), str) or not item["text"].strip():
                raise ContractError(f"{case['id']}: malformed synthetic input {identity}")
        for field in ("allowed_actions", "forbidden_claims", "reviewed_decisions"):
            values = case[field]
            if not isinstance(values, list) or any(not isinstance(value, str) or not value for value in values):
                raise ContractError(f"{case['id']}: {field} must contain nonempty strings")
        if not isinstance(case["required_state_changes"], list):
            raise ContractError(f"{case['id']}: required_state_changes must be an array")
        state_keys = [change.get("key") for change in case["required_state_changes"]]
        if any(not isinstance(key, str) or not key for key in state_keys) or len(state_keys) != len(set(state_keys)):
            raise ContractError(f"{case['id']}: state change keys must be unique nonempty strings")

    negative = fixture.get("negative_fixtures")
    expected_negative_ids = {
        "premature-advancement",
        "wrong-attribution",
        "solution-counted-as-attempt",
        "coverage-inflation",
        "repeated-completed-teaching",
        "no-instructional-action",
        "action-after-delivery",
    }
    if not isinstance(negative, list) or {item.get("id") for item in negative} != expected_negative_ids:
        raise ContractError("negative fixture inventory mismatch")
    if len({item.get("id") for item in negative}) != len(negative):
        raise ContractError("duplicate negative fixture id")
    known = set(ids)
    for item in negative:
        if item.get("case_id") not in known or item.get("expected_violation") not in {"forbidden_claim", "state_change_mismatch", "missing_instructional_action"}:
            raise ContractError(f"{item.get('id')}: invalid negative fixture contract")
        mutation = item.get("mutation", {})
        if mutation.get("kind") not in {"claim", "state_change", "remove_actions", "move_actions_after_delivery"}:
            raise ContractError(f"{item['id']}: unsupported negative fixture mutation")
        if mutation.get("kind") == "claim" and mutation.get("value") not in case_by_id(cases, item["case_id"])["forbidden_claims"]:
            raise ContractError(f"{item['id']}: claim mutation is not forbidden by its case")
        if mutation.get("kind") == "state_change" and mutation.get("key") not in {change["key"] for change in case_by_id(cases, item["case_id"])["required_state_changes"]}:
            raise ContractError(f"{item['id']}: state mutation does not target a required state")
        if mutation.get("kind") in {"remove_actions", "move_actions_after_delivery"} and item["expected_violation"] != "missing_instructional_action":
            raise ContractError(f"{item['id']}: action mutation must expect missing_instructional_action")
    return cases


def case_by_id(cases, identity):
    return next(case for case in cases if case["id"] == identity)


def evaluate(case, trace):
    violations = []
    if trace.get("case_id") != case["id"]:
        return ["case_id_mismatch"], []
    events = trace.get("events")
    if not isinstance(events, list):
        return ["events_missing"], []

    prior_seq = -1
    for event in events:
        seq = event.get("seq")
        if not isinstance(seq, int) or seq <= prior_seq:
            violations.append("event_order")
            break
        prior_seq = seq

    input_events = [event for event in events if event.get("kind") == "input"]
    expected_inputs = [item["id"] for item in case["inputs"]]
    observed_inputs = [event.get("input_id") for event in input_events]
    if observed_inputs != expected_inputs:
        violations.append("input_order")
    expected_actors = {item["id"]: item["actor"] for item in case["inputs"]}
    if any(event.get("actor") != expected_actors.get(event.get("input_id")) for event in input_events):
        violations.append("input_attribution_mismatch")

    for event in events:
        kind = event.get("kind")
        if kind == "action" and event.get("action") not in case["allowed_actions"]:
            violations.append("disallowed_action")
        elif kind == "claim" and event.get("claim") in case["forbidden_claims"]:
            violations.append("forbidden_claim")
    conditions = {
        event.get("condition"): event.get("value")
        for event in events
        if event.get("kind") == "reviewed_condition"
    }
    for contract in case.get("conditional_contracts", []):
        condition = contract["condition"]
        condition_value = conditions.get(condition)
        if condition_value is contract["if_value"]:
            assistance = [event for event in events if event.get("kind") == "assistance"]
            followup_actions = [
                event for event in events
                if event.get("kind") == "action" and event.get("action") == contract["requires_assistance_before_action"]
            ]
            if not assistance or not followup_actions or assistance[0]["seq"] >= followup_actions[0]["seq"]:
                violations.append("conditional_assistance_order")
    decisions = [event.get("label") for event in events if event.get("kind") == "decision"]
    if decisions != case["reviewed_decisions"]:
        violations.append("reviewed_decision_mismatch")

    observed_states = {}
    for event in events:
        if event.get("kind") == "state_change":
            observed_states[event.get("key")] = event.get("value")
    for required in case["required_state_changes"]:
        if required["key"] not in observed_states:
            violations.append("state_change_missing")
        elif observed_states[required["key"]] != required.get("value"):
            violations.append("state_change_mismatch")

    receipts = [event for event in events if event.get("kind") == "receipt"]
    delivered_receipts = [item for item in receipts if item.get("receipt") == "delivery" and item.get("status") == "delivered"]
    delivery_observed = bool(delivered_receipts)
    if delivered_receipts:
        delivery_seq = min(item["seq"] for item in delivered_receipts)
        allowed_actions = [
            event for event in events
            if event.get("kind") == "action" and event.get("action") in case["allowed_actions"]
        ]
        if not any(event["seq"] < delivery_seq for event in allowed_actions):
            violations.append("missing_instructional_action")
    assistance_observed = any(item.get("kind") == "assistance" for item in events)
    observability = {
        "delivery": "OBSERVED_IN_TRACE" if delivery_observed else "NOT_OBSERVABLE",
        "authorship": "OBSERVED_IN_TRACE" if input_events and not any(event.get("actor") is None for event in input_events) else "NOT_OBSERVABLE",
        "assistance": "OBSERVED_IN_TRACE" if assistance_observed else "NOT_OBSERVABLE",
    }
    for contract in case.get("conditional_contracts", []):
        condition_value = conditions.get(contract["condition"])
        observability[f"condition:{contract['condition']}"] = (
            "OBSERVED_TRUE" if condition_value is True else "OBSERVED_FALSE" if condition_value is False else "NOT_OBSERVABLE"
        )
    return sorted(set(violations)), observability


def baseline_trace(case):
    events = []
    seq = 0
    for item in case["inputs"]:
        events.append({"seq": seq, "kind": "input", "input_id": item["id"], "actor": item["actor"]})
        seq += 1
    for label in case["reviewed_decisions"]:
        events.append({"seq": seq, "kind": "decision", "label": label})
        seq += 1
    for change in case["required_state_changes"]:
        events.append({"seq": seq, "kind": "state_change", **change})
        seq += 1
    events.append({"seq": seq, "kind": "action", "action": case["allowed_actions"][0]})
    seq += 1
    events.append({"seq": seq, "kind": "receipt", "receipt": "delivery", "status": "delivered"})
    return {"case_id": case["id"], "events": events}


def negative_self_check(cases, negative_fixtures):
    failures = []
    for case in cases:
        for action in case["allowed_actions"]:
            trace = baseline_trace(case)
            action_event = next(event for event in trace["events"] if event.get("kind") == "action")
            action_event["action"] = action
            violations, _ = evaluate(case, trace)
            if violations:
                failures.append(f"{case['id']}: allowed alternative {action} was rejected with {violations}")

    for fixture in negative_fixtures:
        case = case_by_id(cases, fixture["case_id"])
        trace = baseline_trace(case)
        mutation = fixture["mutation"]
        if mutation["kind"] == "claim":
            trace["events"].append({"seq": len(trace["events"]), "kind": "claim", "claim": mutation["value"]})
        elif mutation["kind"] == "state_change":
            for event in trace["events"]:
                if event.get("kind") == "state_change" and event.get("key") == mutation["key"]:
                    event["value"] = mutation["value"]
                    break
        elif mutation["kind"] == "remove_actions":
            trace["events"] = [event for event in trace["events"] if event.get("kind") != "action"]
            for seq, event in enumerate(trace["events"]):
                event["seq"] = seq
        else:
            action_events = [event for event in trace["events"] if event.get("kind") == "action"]
            trace["events"] = [event for event in trace["events"] if event.get("kind") != "action"]
            trace["events"].extend(action_events)
            for seq, event in enumerate(trace["events"]):
                event["seq"] = seq
        violations, _ = evaluate(case, trace)
        if fixture["expected_violation"] not in violations:
            failures.append(f"{fixture['id']}: expected {fixture['expected_violation']}, got {violations}")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trace", type=Path, help="recorded trace JSON for one fixture case")
    parser.add_argument("--fixture", type=Path, default=FIXTURE, help="contract fixture path")
    args = parser.parse_args()
    try:
        fixture = load_json(args.fixture)
        cases = validate_fixture(fixture)
        negative_failures = negative_self_check(cases, fixture["negative_fixtures"])
        if negative_failures:
            raise ContractError("negative self-check failed: " + "; ".join(negative_failures))
        if args.trace:
            trace = load_json(args.trace)
            try:
                case = case_by_id(cases, trace.get("case_id"))
            except StopIteration as error:
                raise ContractError(f"unknown trace case id: {trace.get('case_id')}") from error
            violations, observability = evaluate(case, trace)
            print(json.dumps({"case_id": case["id"], "violations": violations, "observability": observability, "claim_limit": case["claim_limit"]}, sort_keys=True))
            if violations:
                return 1
        else:
            print(f"Validated {len(cases)} synthetic cases and {len(fixture['negative_fixtures'])} failing negative fixtures.")
        print("Limits: event and explicit-label contract checking only; no semantic grading, delivery proof, or human learning claim.")
        return 0
    except (ContractError, KeyError, TypeError) as error:
        print(f"learning-loop trace check failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
