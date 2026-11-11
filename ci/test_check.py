"""Tests for the local check runner. No test here starts Cargo."""
import contextlib
import io
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from ci import check


class FakeRun:
    """Stands in for subprocess.run: records each command and fails the ones named."""

    def __init__(self, failing=(), missing=()):
        self.failing, self.missing, self.calls = set(failing), set(missing), []

    def __call__(self, command, cwd, env):
        self.calls.append((command, cwd, env))
        if command[0] in self.missing:
            raise FileNotFoundError(command[0])
        return subprocess.CompletedProcess(command, 1 if tuple(command[:2]) in self.failing else 0)


def run_check(*argv, fake=None):
    fake = fake or FakeRun()
    out, err = io.StringIO(), io.StringIO()
    with mock.patch.object(check.subprocess, "run", fake), \
            contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = check.main(list(argv))
    return code, out.getvalue(), err.getvalue(), fake.calls


class CheckRunnerTests(unittest.TestCase):
    def test_list_shows_every_command_and_runs_none(self):
        code, out, _, calls = run_check("--list")
        self.assertEqual((code, calls), (0, []))
        for expected in ("cargo fmt --all --check", "cargo clippy --locked --all-targets -- -D warnings",
                         "cargo test --locked", "ci/check_markdown_links.py", "ci/check_help_text.py",
                         "workers/docling/tests/test_gate.py"):
            self.assertIn(expected, out)

    def test_scopes_run_once_each_in_a_fixed_order(self):
        self.assertEqual(check.selected_scopes(["docs", "core", "docs"]), ["core", "docs"])
        self.assertEqual(check.selected_scopes([]), list(check.SCOPES))
        self.assertEqual(check.selected_scopes(["python", "all"]), list(check.SCOPES))

    def test_every_command_runs_from_the_repository_root(self):
        code, _, _, calls = run_check()
        self.assertEqual(code, 0)
        self.assertEqual([command for command, _, _ in calls],
                         [command for scope in check.SCOPES for command in check.commands(scope)])
        self.assertEqual({cwd for _, cwd, _ in calls}, {check.ROOT})

    def test_a_scope_without_core_runs_no_cargo(self):
        _, _, _, calls = run_check("python", "docs")
        self.assertTrue(calls)
        self.assertFalse([command for command, _, _ in calls if command[0] == "cargo"])

    def test_a_failed_command_is_named_and_stops_the_run(self):
        code, _, err, calls = run_check(fake=FakeRun(failing={("cargo", "clippy")}))
        self.assertEqual(code, 1)
        self.assertIn("Failed: cargo clippy", err)
        self.assertEqual(calls[-1][0][:2], ["cargo", "clippy"])

    def test_a_missing_prerequisite_is_a_failure(self):
        code, _, err, _ = run_check(fake=FakeRun(missing={"cargo"}))
        self.assertEqual(code, 1)
        self.assertIn("Missing prerequisite: cargo", err)

    def test_unknown_scope_is_rejected_before_any_command_runs(self):
        fake = FakeRun()
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            run_check("release", fake=fake)
        self.assertEqual(fake.calls, [])

    def test_the_tests_never_inherit_another_build_of_the_program(self):
        with mock.patch.dict(check.os.environ, {"MAGNESIUM_TEST_BINARY": "/elsewhere/magnesium"}):
            _, _, _, calls = run_check("core", "--offline")
        for _, _, env in calls:
            self.assertNotIn("MAGNESIUM_TEST_BINARY", env)
            self.assertEqual(env["CARGO_NET_OFFLINE"], "true")


if __name__ == "__main__":
    unittest.main()
