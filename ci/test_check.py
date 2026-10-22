"""The local check entrypoint must run real checks and propagate failures."""
import contextlib
import io
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
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
        test = self.position(commands, "cargo", "test")
        for flag in ["--locked", "--release"]:
            self.assertIn(flag, commands[build])
        self.assertIn("--locked", commands[test])
        # The test binary path below is only fresh if the build writes there.
        self.assertEqual(commands[build][commands[build].index("--target") + 1], "test-host")
        self.assertLess(build, test)
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


class FakeDiskutil:
    """Stands in for subprocess.run so no test creates a real RAM disk."""

    def __init__(self, volumes, *, fail=(), eject_failures=0):
        self.volumes = volumes
        self.fail = set(fail)
        self.eject_failures = eject_failures
        self.calls = []

    def __call__(self, command, **kwargs):
        self.calls.append(command)
        action = command[1] if command[0] == "diskutil" else command[0]
        if action in self.fail:
            raise subprocess.CalledProcessError(1, command, stderr="Error: could not do that\n")
        if action == "eject" and self.eject_failures:
            self.eject_failures -= 1
            return subprocess.CompletedProcess(command, 1, "", "busy")
        if action == "erasevolume":
            (self.volumes / command[3]).mkdir()
        stdout = "/dev/disk9\n" if action == "image" else ""
        return subprocess.CompletedProcess(command, 0, stdout, "")

    def ejected(self):
        return [command for command in self.calls if command[:2] == ["diskutil", "eject"]]


class TestTempDirTests(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.scratch, ignore_errors=True)
        env = patch.dict(os.environ, clear=False)
        env.start()
        self.addCleanup(env.stop)
        os.environ.pop(check.DISK_TMP_OPT_OUT, None)
        self.output = io.StringIO()
        redirect = contextlib.redirect_stdout(self.output)
        redirect.__enter__()
        self.addCleanup(redirect.__exit__, None, None, None)
        for name, value in [("VOLUMES_DIR", self.scratch), ("SHM_DIR", self.scratch / "shm")]:
            patcher = patch.object(check, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        sleep = patch.object(check.time, "sleep")
        sleep.start()
        self.addCleanup(sleep.stop)

    def macos(self, **options):
        diskutil = FakeDiskutil(self.scratch, **options)
        platform = patch.object(check.sys, "platform", "darwin")
        mounted = patch.object(check.os.path, "ismount", return_value=True)
        run = patch.object(check.subprocess, "run", diskutil)
        for patcher in (platform, mounted, run):
            patcher.start()
            self.addCleanup(patcher.stop)
        return diskutil

    def test_macos_uses_a_uniquely_named_ram_volume_and_ejects_the_attached_device(self):
        diskutil = self.macos()
        with check.test_temp_dir() as directory:
            self.assertEqual(Path(directory), self.scratch / f"magnesium-ci-{os.getpid()}")
            self.assertEqual(diskutil.ejected(), [])
            self.assertIn(["diskutil", "image", "attach", "--noMount", "ram://1048576"], diskutil.calls)
            self.assertIn(["diskutil", "erasevolume", "APFS", f"magnesium-ci-{os.getpid()}", "/dev/disk9"], diskutil.calls)
        self.assertEqual(diskutil.ejected(), [["diskutil", "eject", "/dev/disk9"]])
        self.assertIn("RAM-backed", self.output.getvalue())

    def test_macos_ejects_when_the_checks_fail(self):
        diskutil = self.macos()
        with self.assertRaises(KeyboardInterrupt), check.test_temp_dir():
            raise KeyboardInterrupt
        self.assertEqual(diskutil.ejected(), [["diskutil", "eject", "/dev/disk9"]])

    def test_sigterm_exits_through_the_cleanup_and_restores_the_handler(self):
        diskutil = self.macos()
        before = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(SystemExit) as stopped, check.test_temp_dir():
            signal.raise_signal(signal.SIGTERM)
        self.assertEqual(stopped.exception.code, 128 + signal.SIGTERM)
        self.assertEqual(diskutil.ejected(), [["diskutil", "eject", "/dev/disk9"]])
        self.assertEqual(signal.getsignal(signal.SIGTERM), before)

    def test_macos_setup_failure_ejects_the_attached_device_and_falls_back(self):
        diskutil = self.macos(fail={"erasevolume"})
        with check.test_temp_dir() as directory:
            self.assertIsNone(directory)
        self.assertEqual(diskutil.ejected(), [["diskutil", "eject", "/dev/disk9"]])
        self.assertIn("default temporary directory", self.output.getvalue())
        self.assertEqual(len(self.output.getvalue().strip().splitlines()), 1)

    def test_macos_without_diskutil_falls_back(self):
        self.macos()
        with patch.object(check.subprocess, "run", side_effect=FileNotFoundError("diskutil")):
            with check.test_temp_dir() as directory:
                self.assertIsNone(directory)
        self.assertIn("default temporary directory", self.output.getvalue())

    def test_busy_volume_is_retried_then_force_unmounted(self):
        diskutil = self.macos(eject_failures=3)
        with check.test_temp_dir():
            pass
        self.assertIn(["diskutil", "unmountDisk", "force", "/dev/disk9"], diskutil.calls)
        self.assertEqual(len(diskutil.ejected()), 4)

    def test_linux_uses_a_private_shm_directory_and_removes_it(self):
        (self.scratch / "shm").mkdir()
        with patch.object(check.sys, "platform", "linux"), check.test_temp_dir() as directory:
            self.assertEqual(Path(directory).parent, self.scratch / "shm")
            (Path(directory) / "workspace").mkdir()
        self.assertFalse(Path(directory).exists())

    def test_linux_without_usable_shm_falls_back(self):
        with patch.object(check.sys, "platform", "linux"), check.test_temp_dir() as directory:
            self.assertIsNone(directory)
        (self.scratch / "shm").mkdir()
        noexec = os.statvfs_result((0, 4096, 0, 0, 1 << 20, 0, 0, 0, getattr(os, "ST_NOEXEC", 8), 255))
        with patch.object(check.sys, "platform", "linux"), patch.object(check.os, "statvfs", return_value=noexec):
            with check.test_temp_dir() as directory:
                self.assertIsNone(directory)
        self.assertEqual(list((self.scratch / "shm").iterdir()), [])

    def test_other_platforms_and_the_opt_out_keep_the_default_directory(self):
        with patch.object(check.subprocess, "run") as run:
            with patch.object(check.sys, "platform", "win32"), check.test_temp_dir() as directory:
                self.assertIsNone(directory)
            with patch.object(check.sys, "platform", "darwin"), patch.dict(os.environ, {check.DISK_TMP_OPT_OUT: "1"}):
                with check.test_temp_dir() as directory:
                    self.assertIsNone(directory)
            run.assert_not_called()

    def test_only_cargo_test_gets_the_temporary_directory(self):
        seen = {}

        @contextlib.contextmanager
        def ram():
            yield "/ram"

        def record(command, **kwargs):
            seen[command[1]] = kwargs["env"]

        with patch.object(check, "test_temp_dir", ram), patch.object(check.subprocess, "run", record), \
                patch.object(check, "native_target", return_value="test-host"):
            self.assertEqual(check.main(["core"]), 0)
        for name in ("TMPDIR", "TEMP", "TMP"):
            self.assertEqual(seen["test"][name], "/ram")
            for other in ("fmt", "clippy", "build", "ci/check_native_sources.py"):
                self.assertNotEqual(seen[other].get(name), "/ram")
        self.assertEqual(seen["test"]["MAGNESIUM_TEST_BINARY"], seen["build"]["MAGNESIUM_TEST_BINARY"])

    def test_list_and_other_scopes_never_create_a_ram_disk(self):
        with patch.object(check, "test_temp_dir") as ram, patch.object(check.subprocess, "run"), \
                patch.object(check, "native_target", return_value="test-host"):
            check.main(["--list"])
            check.main(["python", "docs"])
        ram.assert_not_called()


if __name__ == "__main__":
    unittest.main()
