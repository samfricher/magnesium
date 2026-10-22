"""Mutation coverage for the offline learning-loop report."""

import copy
import json
import re
import unittest
from unittest.mock import patch
from pathlib import Path

from ci import report_learning_loop as report


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = report.load_json(ROOT / "tests/fixtures/learning_loop/metrics.schema.json")


def complete_saved_trace():
    return {
        "schema_version": "learning-loop-metrics/v2",
        "source_kind": "authorized_saved",
        "saved_trace_authorized": True,
        "events": [
            {"kind": "capture", "captured": True},
            {"kind": "task_exposure", "task_id": "task-1", "purpose": "independent", "exposed": True},
            {"kind": "independent_attempt", "task_id": "task-1", "present": True},
            {"kind": "mastery_claim", "task_id": "task-1", "claimed": True},
            {"kind": "repair", "repair_id": "repair-1", "issue_key": "issue-1", "correction_applied": True, "recheck_id": "recheck-1"},
            {"kind": "recheck", "recheck_id": "recheck-1", "issue_key": "issue-1", "task_id": "task-1", "independent": True, "attempt_observed": True, "passed": True},
            {"kind": "next_action", "repair_id": "repair-1", "updated": True, "action": "advance"},
            {"kind": "coverage", "planned_units": ["unit-a", "unit-b"], "addressed_units": ["unit-a", "unit-b"], "remaining_units": []},
            {"kind": "pause", "requested": True, "respected": True, "post_pause_delivery": False},
            {"kind": "delivery", "delivery_key": "d-first"},
        ],
    }


def result(trace):
    report.validate_trace(trace, SCHEMA)
    return report.evaluate_trace(trace)


def check_status(evaluation, code):
    return next(item["status"] for item in evaluation["checks"] if item["code"] == code)


class LearningLoopReportTests(unittest.TestCase):
    def test_report_runs_existing_synthetic_oracle_and_fails_when_it_fails(self):
        assembled = report.build_report()
        self.assertIn("Status: **PASS**", report.render_markdown(assembled))
        with patch.object(report.trace_oracle, "negative_self_check", return_value=["failed"]):
            failed = report.build_report()
        self.assertEqual(failed["synthetic_contract_oracle"]["status"], "FAIL")
        self.assertEqual(failed["levels"]["native"]["status"], "NOT_TESTED")
        self.assertIn("Status: **FAIL**", report.render_markdown(failed))
        self.assertEqual(failed["levels"]["agent"]["status"], "NOT_TESTED")

    def test_fixture_validation_rejects_incomplete_case_contracts(self):
        oracle = report.trace_oracle
        fixture = oracle.load_json(oracle.FIXTURE)
        self.assertEqual(len(oracle.validate_fixture(fixture)), 12)
        mutations = {
            "empty claim_limit": (
                lambda case: case.update(claim_limit=""),
                "claim_limit must be a nonempty string",
            ),
            "empty reviewed_decisions": (
                lambda case: case.update(reviewed_decisions=[]),
                "reviewed decisions must be nonempty",
            ),
            "non-object initial_state": (
                lambda case: case.update(initial_state=[]),
                "initial_state must be an object",
            ),
            "state change without value": (
                lambda case: case["required_state_changes"][0].pop("value"),
                r"state change \S+ needs an expected value",
            ),
        }
        for index, case in enumerate(fixture["cases"]):
            for name, (mutate, message) in mutations.items():
                with self.subTest(case=case["id"], mutation=name):
                    mutated = copy.deepcopy(fixture)
                    mutate(mutated["cases"][index])
                    with self.assertRaisesRegex(
                        oracle.ContractError, re.escape(case["id"]) + ": .*" + message
                    ):
                        oracle.validate_fixture(mutated)

    def test_caller_authorization_cannot_promote_offline_trace_to_agent_pass(self):
        evaluation = result(complete_saved_trace())
        self.assertEqual(evaluation["levels"]["agent"]["status"], "NOT_TESTED")
        self.assertEqual(evaluation["levels"]["native"]["status"], "NOT_TESTED")
        self.assertEqual(evaluation["levels"]["human"]["status"], "NOT_TESTED")

    def test_synthetic_trace_never_claims_agent_execution(self):
        trace = complete_saved_trace()
        trace["source_kind"] = "synthetic"
        evaluation = result(trace)
        self.assertEqual(evaluation["levels"]["agent"]["status"], "NOT_TESTED")

    def test_missing_capture_is_not_observable(self):
        trace = complete_saved_trace()
        trace["events"] = [event for event in trace["events"] if event["kind"] != "capture"]
        evaluation = result(trace)
        self.assertEqual(evaluation["levels"]["agent"]["status"], "NOT_OBSERVABLE")
        self.assertEqual(check_status(evaluation, "capture"), "NOT_OBSERVABLE")

    def test_exposure_without_independent_attempt_cannot_support_mastery(self):
        trace = complete_saved_trace()
        attempt = next(event for event in trace["events"] if event["kind"] == "independent_attempt")
        attempt["present"] = False
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "exposure_is_not_mastery"), "FAIL")
        self.assertEqual(evaluation["levels"]["agent"]["status"], "NOT_TESTED")

    def test_later_attempt_cannot_retroactively_support_a_mastery_claim(self):
        trace = complete_saved_trace()
        events = trace["events"]
        attempt = events.pop(2)
        events.insert(3, attempt)
        self.assertEqual(check_status(result(trace), "exposure_is_not_mastery"), "FAIL")
        events.insert(2, {"kind": "independent_attempt", "task_id": "task-1", "present": True})
        self.assertEqual(check_status(result(trace), "exposure_is_not_mastery"), "PASS")

    def test_repair_without_linked_recheck_fails_loop_closure(self):
        trace = complete_saved_trace()
        trace["events"] = [event for event in trace["events"] if event["kind"] != "recheck"]
        self.assertEqual(check_status(result(trace), "repair_followed_by_observed_check"), "FAIL")

    def test_wrong_recheck_followed_by_smaller_repair_closes_loop_but_not_criterion(self):
        trace = complete_saved_trace()
        next(event for event in trace["events"] if event["kind"] == "recheck")["passed"] = False
        next(event for event in trace["events"] if event["kind"] == "next_action")["action"] = "repair_smaller"
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "repair_followed_by_observed_check"), "PASS")
        self.assertEqual(check_status(evaluation, "next_action_responds_to_check"), "PASS")
        self.assertEqual(check_status(evaluation, "criterion_supported_after_repair"), "FAIL")

    def test_ignored_wrong_recheck_fails_decision_responsiveness(self):
        trace = complete_saved_trace()
        next(event for event in trace["events"] if event["kind"] == "recheck")["passed"] = False
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "repair_followed_by_observed_check"), "PASS")
        self.assertEqual(check_status(evaluation, "next_action_responds_to_check"), "FAIL")
        self.assertEqual(check_status(evaluation, "criterion_supported_after_repair"), "FAIL")

    def test_missing_attempt_remains_not_observable(self):
        trace = complete_saved_trace()
        next(event for event in trace["events"] if event["kind"] == "recheck")["attempt_observed"] = False
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "repair_followed_by_observed_check"), "NOT_OBSERVABLE")
        self.assertEqual(check_status(evaluation, "next_action_responds_to_check"), "NOT_OBSERVABLE")
        self.assertEqual(check_status(evaluation, "criterion_supported_after_repair"), "NOT_OBSERVABLE")

    def test_guided_check_can_close_loop_without_supporting_independent_criterion(self):
        trace = complete_saved_trace()
        exposure = next(event for event in trace["events"] if event["kind"] == "task_exposure")
        exposure["purpose"] = "guided"
        recheck = next(event for event in trace["events"] if event["kind"] == "recheck")
        recheck["independent"] = False
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "repair_followed_by_observed_check"), "PASS")
        self.assertEqual(check_status(evaluation, "criterion_supported_after_repair"), "FAIL")

    def test_check_mode_must_match_the_declared_task_purpose(self):
        trace = complete_saved_trace()
        next(event for event in trace["events"] if event["kind"] == "recheck")["independent"] = False
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "repair_followed_by_observed_check"), "FAIL")
        self.assertEqual(check_status(evaluation, "next_action_responds_to_check"), "FAIL")
        self.assertEqual(check_status(evaluation, "criterion_supported_after_repair"), "FAIL")

    def test_repair_recheck_without_next_action_effect_fails(self):
        trace = complete_saved_trace()
        trace["events"] = [event for event in trace["events"] if event["kind"] != "next_action"]
        self.assertEqual(check_status(result(trace), "next_action_responds_to_check"), "FAIL")

    def test_unsupported_schema_versions_and_vocabulary_reject(self):
        trace = complete_saved_trace()
        trace["schema_version"] = "learning-loop-metrics/v3"
        with self.assertRaisesRegex(report.ReportError, "unsupported trace schema version"):
            result(trace)
        schema = dict(SCHEMA, nullable=True)
        with self.assertRaisesRegex(report.ReportError, "version or vocabulary"):
            report.validate_trace(complete_saved_trace(), schema)

    def test_falsy_supplied_schemas_are_rejected_instead_of_defaulted(self):
        for schema in ({}, [], ""):
            with self.subTest(schema=schema), self.assertRaisesRegex(report.ReportError, "unsupported metrics schema"):
                report.build_report(trace=complete_saved_trace(), schema=schema)

    def test_lost_source_unit_fails_coverage_accounting(self):
        trace = complete_saved_trace()
        coverage = next(event for event in trace["events"] if event["kind"] == "coverage")
        coverage["planned_units"] = ["unit-a", "unit-b", "unit-c"]
        coverage["addressed_units"] = ["unit-a", "unit-c"]
        coverage["remaining_units"] = []
        self.assertEqual(check_status(result(trace), "coverage_units_accounted"), "FAIL")

    def test_incomplete_coverage_preserves_remaining_units_and_cannot_pass(self):
        trace = complete_saved_trace()
        coverage = next(event for event in trace["events"] if event["kind"] == "coverage")
        coverage["planned_units"] = ["unit-a", "unit-b", "unit-c"]
        coverage["addressed_units"] = ["unit-a", "unit-c"]
        coverage["remaining_units"] = ["unit-b"]
        evaluation = result(trace)
        self.assertEqual(check_status(evaluation, "coverage_units_accounted"), "PASS")
        self.assertEqual(check_status(evaluation, "planned_coverage_complete"), "FAIL")
        self.assertEqual(evaluation["levels"]["agent"]["status"], "NOT_TESTED")
        self.assertEqual(evaluation["coverage"][0]["remaining_units"], 1)

    def test_pause_violation_fails_when_delivery_continues_after_pause(self):
        trace = complete_saved_trace()
        pause = next(event for event in trace["events"] if event["kind"] == "pause")
        pause["post_pause_delivery"] = True
        self.assertEqual(check_status(result(trace), "pause_respected"), "FAIL")

    def test_repeated_delivery_key_fails(self):
        trace = complete_saved_trace()
        trace["events"].append({"kind": "delivery", "delivery_key": "d-first"})
        self.assertEqual(check_status(result(trace), "delivery_not_repeated"), "FAIL")

    def test_schema_rejects_free_text_private_content_and_unapproved_saved_traces(self):
        trace = complete_saved_trace()
        trace["events"][0]["transcript"] = "/private/path/token-secret"
        with self.assertRaisesRegex(report.ReportError, "versioned contract"):
            result(trace)
        trace = complete_saved_trace()
        trace["saved_trace_authorized"] = False
        with self.assertRaisesRegex(report.ReportError, "requires explicit authorization"):
            result(trace)

    def test_report_does_not_emit_trace_ids_or_local_paths(self):
        trace = complete_saved_trace()
        trace["trace_id"] = "private-host-identifier"
        with self.assertRaises(report.ReportError):
            result(trace)

    def test_report_diagnostics_are_codes_and_counts_only(self):
        trace = complete_saved_trace()
        next(event for event in trace["events"] if event["kind"] == "pause")["post_pause_delivery"] = True
        evaluation = result(trace)
        rendered = json.dumps(evaluation)
        self.assertIn('"code": "pause_respected"', rendered)
        self.assertNotIn("task-1", rendered)
        self.assertLessEqual(len(evaluation["diagnostics"]), report.MAX_DIAGNOSTICS)
        self.assertLessEqual(evaluation["counts"]["events"], report.MAX_EVENTS)

    def test_report_rejects_nonaggregate_baseline_fields(self):
        baseline = report.load_json(ROOT / "tests/fixtures/learning_loop/baseline.json")
        baseline["private_host_id"] = "host-secret"
        with self.assertRaisesRegex(report.ReportError, "versioned aggregate contract"):
            report.build_report(baseline=baseline)

    def test_versioned_baseline_facts_are_data_not_hardcoded_study_branches(self):
        baseline = report.load_json(ROOT / "tests/fixtures/learning_loop/baseline.json")
        baseline["turns"] = 30
        baseline["model_split"]["gpt-5.6-terra/max"] = 25
        report.validate_baseline(baseline)

    def test_baseline_source_hash_is_explicitly_inspected_or_null(self):
        baseline = report.load_json(ROOT / "tests/fixtures/learning_loop/baseline.json")
        baseline["inspected_source_sha"] = None
        report.validate_baseline(baseline)
        baseline["inspected_source_sha"] = "/private/path/token-secret"
        with self.assertRaisesRegex(report.ReportError, "full commit hash or null"):
            report.validate_baseline(baseline)


if __name__ == "__main__":
    unittest.main()
