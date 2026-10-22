"""The local check entrypoint must run real checks and propagate failures."""
import contextlib
import io
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from ci import check


class CheckRunnerTests(unittest.TestCase):
    def run_main(self, args):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()), patch.object(check, "native_target", return_value="test-host"):
            return check.main(args)

    def test_default_lists_every_scope_without_running_commands(self):
        with patch.object(check.subprocess, "run") as run:
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(check.main(["--list"]), 0)
            run.assert_not_called()
        for scope in check.SCOPES:
            self.assertIn(f"[{scope}]", output.getvalue())

    def test_scopes_are_deduplicated_and_run_in_dependency_order(self):
        self.assertEqual(check.selected_scopes(["docs", "core", "docs"]), ["core", "docs"])
        self.assertEqual(check.selected_scopes(["python", "all"]), list(check.SCOPES))

    def position(self, commands, *prefix):
        """Index of the first command starting with prefix."""
        for index, command in enumerate(commands):
            if command[:len(prefix)] == list(prefix):
                return index
        self.fail(f"no command starts with {prefix}")

    def test_core_tests_use_the_freshly_built_release_executable(self):
        commands = [command for _, command in check.commands("core", "test-host")]
        build = self.position(commands, "cargo", "build")
        self.assertIn("--release", commands[build])
        self.assertLess(build, self.position(commands, "cargo", "test"))
        with patch.dict(os.environ, {"MAGNESIUM_TEST_BINARY": "/stale/magnesium"}):
            binary = Path(check.environment(offline=False, target="test-host")["MAGNESIUM_TEST_BINARY"])
        self.assertEqual(binary.parent, check.ROOT / "target/test-host/release")
        self.assertTrue(binary.is_absolute())

    def test_native_target_comes_from_the_selected_compiler(self):
        with patch.object(check.subprocess, "check_output", return_value="rustc 1.95.0\nhost: aarch64-apple-darwin\n"):
            self.assertEqual(check.native_target(), "aarch64-apple-darwin")
        with patch.object(check.subprocess, "check_output", return_value="rustc unknown\n"):
            with self.assertRaises(RuntimeError):
                check.native_target()

    def test_explicit_target_and_output_override_inherited_cross_build_settings(self):
        with patch.dict(os.environ, {"CARGO_BUILD_TARGET": "other-target", "CARGO_TARGET_DIR": "/stale/output"}):
            env = check.environment(offline=False, target="native-host")
        self.assertEqual(env["CARGO_TARGET_DIR"], str(check.ROOT / "target"))
        self.assertEqual(Path(env["MAGNESIUM_TEST_BINARY"]).parent, check.ROOT / "target/native-host/release")
        for _, command in check.commands("core", "native-host"):
            if command[0] == "cargo" and command[1] != "fmt":
                self.assertEqual(command[command.index("--target") + 1], "native-host")

    def test_core_runs_actual_native_source_smoke_after_build(self):
        commands = [command for _, command in check.commands("core", "test-host")]
        smoke = self.position(commands, check.sys.executable, "ci/check_native_sources.py")
        self.assertLess(self.position(commands, "cargo", "build"), smoke)

    def test_python_includes_script_based_docling_tests(self):
        commands = [command for _, command in check.commands("python")]
        self.assertIn([check.sys.executable, "workers/docling/tests/test_gate.py"], commands)
        self.assertIn([check.sys.executable, "workers/docling/tests/test_worker.py"], commands)

    def test_failure_stops_the_selected_scope(self):
        with patch.object(check.subprocess, "run", side_effect=subprocess.CalledProcessError(7, "check")) as run:
            self.assertEqual(self.run_main(["core"]), 1)
            self.assertEqual(run.call_count, 1)

    def test_missing_prerequisite_is_a_failure(self):
        with patch.object(check.subprocess, "run", side_effect=FileNotFoundError):
            self.assertEqual(self.run_main(["python"]), 1)

    def test_docs_scope_runs_link_help_text_bundle_and_brand_checks_in_the_repository(self):
        with patch.object(check.subprocess, "run") as run:
            self.assertEqual(self.run_main(["docs", "--offline"]), 0)
        commands = [call.args[0] for call in run.call_args_list]
        for script in ("ci/check_markdown_links.py", "ci/check_help_text.py",
                       "ci/check_learning_bundle_release.py", "ci/check_brand.py"):
            self.position(commands, check.sys.executable, script)
        for call in run.call_args_list:
            self.assertEqual(call.kwargs["cwd"], check.ROOT)
            self.assertEqual(call.kwargs["env"]["CARGO_NET_OFFLINE"], "true")
            self.assertTrue(call.kwargs["check"])

    def test_unknown_scope_is_rejected_before_any_execution(self):
        with patch.object(check.subprocess, "run") as run, self.assertRaises(SystemExit) as error:
            self.run_main(["typo"])
        self.assertEqual(error.exception.code, 2)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
