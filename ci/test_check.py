"""The local check entrypoint must run real checks and propagate failures."""
import contextlib
import io
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

from ci import check


DEFAULT_TARGETS = ("digest", "opencode_adapter", "smoke")


def label(command):
    """Short name for a command: 'cargo test --no-run', 'cargo test opencode_adapter', 'cargo fmt', or the script or module a Python command runs."""
    if command[0] == "cargo":
        return " ".join(command[:2]) + (" --no-run" if "--no-run" in command else "") + \
            (f" {check.SIDE_TEST}" if check.SIDE_TEST in command else "")
    return " ".join(command[1:6]) if command[1] == "-m" else command[1]


class FakeProcess:
    def __init__(self, fake, command, kwargs, meets):
        self.fake, self.command, self.kwargs, self.meets = fake, command, kwargs, meets
        self.label = label(command)
        self.terminated = threading.Event()
        self.returncode = None
        self.interrupted = False
        self.finishing = threading.Lock()
        fake.record("start", self)

    def wait(self, timeout=None):
        fake = self.fake
        with self.finishing:  # one caller does the work; a later one gets the same answer
            if self.returncode is not None:
                return self.returncode
            if self.meets:
                fake.barrier.wait()
            if self.label in fake.interrupt and not self.interrupted:
                self.interrupted = True
                raise KeyboardInterrupt
            if self.label in fake.hang:
                self.terminated.wait(10)
            if self.terminated.is_set():
                self.returncode = -signal.SIGTERM
            else:
                self.returncode = fake.exit_codes.get(self.label, 0)
                log = self.kwargs.get("stdout")
                if log is not None:
                    log.write(f"{self.label} line 1\n{self.label} line 2\n")
                    log.flush()
            fake.record("end", self)
            return self.returncode

    def terminate(self):
        self.terminated.set()


class FakeSubprocess:
    """Stands in for subprocess.Popen and subprocess.run, so no test starts cargo or a real check.

    Every start and end is recorded in order. exit_codes maps a label to its exit status, hang lists
    labels that only end when terminated, missing lists programs (or labels) that cannot be started,
    interrupt lists labels whose first wait raises KeyboardInterrupt, and meet makes the first N Popen
    processes (or meet_labels, the processes with those labels) wait for each other, which only works
    if they run at the same time.
    """

    def __init__(self, *, exit_codes=None, hang=(), missing=(), meet=0, meet_labels=(), interrupt=()):
        self.exit_codes, self.hang, self.missing = exit_codes or {}, set(hang), set(missing)
        self.meet_labels, self.interrupt = set(meet_labels), set(interrupt)
        parties = meet or len(self.meet_labels)
        self.barrier = threading.Barrier(parties, timeout=2) if parties else None
        self.events, self.processes, self.lock = [], [], threading.Lock()

    def record(self, kind, process):
        with self.lock:
            self.events.append((kind, process.label))
            if kind == "start":
                self.processes.append(process)

    def Popen(self, command, **kwargs):
        if command[0] in self.missing or label(command) in self.missing:
            raise FileNotFoundError(command[0])
        with self.lock:
            if self.meet_labels:
                meets = label(command) in self.meet_labels
            else:
                meets = self.barrier is not None and sum(p.meets for p in self.processes) < self.barrier.parties
        return FakeProcess(self, command, kwargs, meets)

    def run(self, command, **kwargs):
        if command[0] in self.missing or label(command) in self.missing:
            raise FileNotFoundError(command[0])
        code = FakeProcess(self, command, kwargs, False).wait()
        if kwargs.get("check") and code:
            raise subprocess.CalledProcessError(code, command)
        return subprocess.CompletedProcess(command, code)

    def started(self):
        return [name for kind, name in self.events if kind == "start"]

    def process(self, name):
        return next(p for p in self.processes if p.label == name)


def fake_ram_disk(fake):
    """Stands in for test_temp_dir, noting in the fake's events when the one RAM disk is made and released."""
    @contextlib.contextmanager
    def disk():
        fake.events.append(("ram disk", "up"))
        try:
            yield "/ram"
        finally:
            fake.events.append(("ram disk", "down"))
    return disk


class CheckRunnerTests(unittest.TestCase):
    def run_main(self, args, *, targets=DEFAULT_TARGETS, fake=None, **options):
        """Run check.main with every subprocess faked; returns (exit code, fake, stdout, stderr).

        targets is what Cargo reports as test targets, or an exception for Cargo failing to say.
        """
        fake = fake or FakeSubprocess(**options)
        listing = patch.object(check, "test_targets", side_effect=targets) if isinstance(targets, Exception) \
            else patch.object(check, "test_targets", return_value=list(targets))
        out, err = io.StringIO(), io.StringIO()
        with patch.object(check.subprocess, "Popen", fake.Popen), patch.object(check.subprocess, "run", fake.run), \
                patch.object(check, "test_temp_dir", fake_ram_disk(fake)), patch.object(check, "native_target", return_value="test-host"), \
                listing, contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = check.main(args)
        return code, fake, out.getvalue(), err.getvalue()

    def test_list_shows_every_command_by_phase_without_running_any(self):
        with patch.object(check.subprocess, "run") as run, patch.object(check.subprocess, "Popen") as popen, \
                patch.object(check.subprocess, "check_output") as check_output:
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(check.main(["--list"]), 0)
            run.assert_not_called()
            popen.assert_not_called()
            check_output.assert_not_called()
        listing = output.getvalue()
        for scope in check.SCOPES:
            for _, command in check.commands(scope):
                self.assertIn(shlex.join(command), listing)
        test_phase = listing.index("[test phase]")
        self.assertLess(listing.index("[build phase]"), test_phase)
        self.assertIn("--no-run", listing[:test_phase])
        self.assertNotIn("--no-run", listing[test_phase:])
        self.assertEqual(listing.count("RAM disk"), 1)
        # The one test command is shown with the two it becomes, which run at the same time.
        tests = listing[test_phase:]
        self.assertIn("at the same time", tests)
        self.assertIn(f"--test {check.SIDE_TEST} ", tests)
        self.assertIn(f"<every-test-target-except-{check.SIDE_TEST}>", tests)
        self.assertEqual(tests.count("--lib --bins --benches --examples"), 1)

    def test_list_without_core_has_no_test_phase(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(check.main(["--list", "python", "docs"]), 0)
        self.assertIn("[build phase]", output.getvalue())
        self.assertNotIn("[test phase]", output.getvalue())
        self.assertNotIn("cargo", output.getvalue())

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
        lanes, alone = check.plan(["core"], "native-host")
        for _, command in [step for lane in lanes for step in lane] + alone:
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

    def test_plan_keeps_every_check_and_only_adds_the_test_binary_build(self):
        for scopes in (["core"], ["python"], ["docs"], list(check.SCOPES)):
            lanes, alone = check.plan(scopes, "test-host")
            planned = [step for lane in lanes for step in lane] + alone
            expected = [step for scope in scopes for step in check.commands(scope, "test-host")]
            build_only = [step for step in planned if step not in expected]
            if "core" in scopes:
                cwd, test = check.pick(expected, "cargo", "test")
                self.assertEqual(build_only, [(cwd, [*test, "--no-run"])])
            else:
                self.assertEqual(build_only, [])
            self.assertCountEqual([step for step in planned if step not in build_only], expected)

    def test_plan_runs_the_tests_and_native_smoke_alone_in_that_order(self):
        lanes, alone = check.plan(["core", "python", "docs"], "test-host")
        self.assertEqual([command[:2] for _, command in alone], [["cargo", "test"], [check.sys.executable, "ci/check_native_sources.py"]])
        self.assertNotIn("--no-run", alone[0][1])
        in_lanes = [command for lane in lanes for _, command in lane]
        self.assertNotIn(alone[0][1], in_lanes)
        self.assertIn([*alone[0][1], "--no-run"], in_lanes)
        self.assertEqual(check.plan(["python", "docs"])[1], [])

    def test_release_build_does_not_wait_for_the_lint_and_test_build_lane(self):
        lanes, _ = check.plan(["core"], "test-host")
        lane_of = {command[1]: number for number, lane in enumerate(lanes) for _, command in lane}
        self.assertNotEqual(lane_of["build"], lane_of["clippy"])
        self.assertEqual(lane_of["clippy"], lane_of["test"])
        self.assertEqual(lane_of["clippy"], lane_of["fmt"])

    def test_the_tests_start_only_after_every_build_command_has_finished(self):
        code, fake, out, _ = self.run_main([])
        self.assertEqual(code, 0)
        lanes, alone = check.plan(list(check.SCOPES), "test-host")
        build_phase = [label(command) for lane in lanes for _, command in lane]
        self.assertEqual(sorted(fake.events[:2 * len(build_phase)]),
                         sorted([(kind, name) for name in build_phase for kind in ("start", "end")]))
        tests = fake.events[2 * len(build_phase):]
        self.assertEqual(tests[0], ("ram disk", "up"))
        self.assertCountEqual(tests[1:5], [(kind, name) for name in ("cargo test", "cargo test opencode_adapter")
                                           for kind in ("start", "end")])
        self.assertEqual(tests[5:], [
            ("ram disk", "down"),
            ("start", "ci/check_native_sources.py"), ("end", "ci/check_native_sources.py")])
        self.assertIn("Selected local checks passed", out)

    def test_build_phase_commands_run_at_the_same_time(self):
        lanes, _ = check.plan(list(check.SCOPES), "test-host")
        # Each lane's first command waits for the others to start; a sequential runner times out.
        code, fake, _, err = self.run_main([], meet=len(lanes))
        self.assertEqual((code, err), (0, ""))

    def test_each_command_prints_its_whole_output_under_its_own_header(self):
        _, _, out, _ = self.run_main(["core"])
        lanes, alone = check.plan(["core"], "test-host")
        for cwd, command in [step for lane in lanes for step in lane]:
            self.assertIn(f"\n  (.) {shlex.join(command)}\n{label(command)} line 1\n{label(command)} line 2\n", out)

    def test_a_failed_build_command_is_named_and_stops_the_other_commands_and_the_tests(self):
        lanes, _ = check.plan(list(check.SCOPES), "test-host")
        code, fake, out, err = self.run_main([], exit_codes={"cargo fmt": 3}, hang={"cargo build"}, meet=len(lanes))
        self.assertEqual(code, 1)
        self.assertIn("Failed: cargo fmt --all --check (exit 3)", err)
        self.assertIn("cargo fmt line 1", out)
        self.assertTrue(fake.process("cargo build").terminated.is_set())
        self.assertNotIn("cargo build line 1", out)
        for skipped in ("cargo clippy", "cargo test --no-run", "cargo test", "cargo test opencode_adapter",
                        "ci/check_native_sources.py"):
            self.assertNotIn(skipped, fake.started())

    def test_a_failed_test_binary_build_shows_its_output_and_skips_the_test_run(self):
        code, fake, out, err = self.run_main(["core"], exit_codes={"cargo test --no-run": 101})
        self.assertEqual(code, 1)
        self.assertIn("Failed: cargo test", err)
        self.assertIn("--no-run (exit 101)", err)
        self.assertIn("cargo test --no-run line 2", out)
        self.assertEqual(fake.started().count("cargo test"), 0)
        self.assertNotIn("ci/check_native_sources.py", fake.started())

    def test_a_failed_lint_skips_the_rest_of_its_lane(self):
        code, fake, _, err = self.run_main(["core"], exit_codes={"cargo clippy": 1})
        self.assertEqual(code, 1)
        self.assertIn("Failed: cargo clippy", err)
        for skipped in ("cargo test --no-run", "cargo test", "cargo test opencode_adapter", "ci/check_native_sources.py"):
            self.assertNotIn(skipped, fake.started())

    def test_a_failed_test_run_is_named_and_the_native_smoke_does_not_run(self):
        code, fake, _, err = self.run_main(["core"], exit_codes={"cargo test": 101})
        self.assertEqual(code, 1)
        self.assertIn("Failed: cargo test", err)
        self.assertNotIn("--no-run", err)
        self.assertNotIn("ci/check_native_sources.py", fake.started())

    def test_a_failed_python_check_stops_the_phase(self):
        code, fake, _, err = self.run_main(["python"], exit_codes={"-m unittest discover -s ci": 1})
        self.assertEqual(code, 1)
        self.assertIn("Failed:", err)
        self.assertEqual(len(fake.started()), 1)

    def test_a_missing_prerequisite_is_a_failure_in_either_phase(self):
        code, fake, _, err = self.run_main(["python"], missing={check.sys.executable})
        self.assertEqual(code, 1)
        self.assertIn(f"Missing prerequisite: {check.sys.executable}", err)
        code, fake, _, err = self.run_main(["core"], missing={"cargo"})
        self.assertEqual(code, 1)
        self.assertIn("Missing prerequisite: cargo", err)
        self.assertEqual(fake.events, [])

    def test_a_missing_tool_in_the_test_phase_is_a_failure(self):
        for missing, targets in [({"cargo test"}, DEFAULT_TARGETS),  # the main command cannot start
                                 ({"cargo test opencode_adapter"}, DEFAULT_TARGETS),  # the side command cannot start
                                 ({"cargo test"}, ("smoke",))]:  # the single command cannot start
            with self.subTest(missing=missing, targets=targets):
                code, fake, _, err = self.run_main(["core"], missing=missing, targets=targets)
                self.assertEqual(code, 1)
                self.assertIn("Missing prerequisite: cargo", err)
                self.assertNotIn("ci/check_native_sources.py", fake.started())
                self.assertEqual(fake.events[-1], ("ram disk", "down"))
                for process in fake.processes:  # nothing is left running
                    self.assertIsNotNone(process.returncode)

    def test_a_crash_while_running_the_build_phase_fails_the_run(self):
        with patch.object(check.subprocess, "Popen", side_effect=PermissionError("denied")), \
                patch.object(check, "native_target", return_value="test-host"), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(check.main(["docs"]), 1)
        self.assertIn("PermissionError", err.getvalue())

    def test_scopes_without_core_run_no_cargo_and_no_test_phase(self):
        code, fake, out, _ = self.run_main(["python", "docs", "--offline"])
        self.assertEqual(code, 0)
        expected = [label(command) for scope in ("python", "docs") for _, command in check.commands(scope)]
        self.assertCountEqual(fake.started(), expected)
        self.assertNotIn("[test phase]", out)

    def test_core_alone_runs_only_the_rust_checks_and_native_smoke(self):
        code, fake, _, _ = self.run_main(["core"])
        self.assertEqual(code, 0)
        self.assertCountEqual(fake.started(), [
            "cargo fmt", "cargo clippy", "cargo build", "cargo test --no-run", "cargo test", "cargo test opencode_adapter",
            "ci/check_native_sources.py"])
        self.assertLess(fake.started().index("cargo test --no-run"), fake.started().index("cargo test"))

    def test_docs_scope_runs_link_help_text_bundle_and_brand_checks_in_the_repository(self):
        code, fake, _, _ = self.run_main(["docs", "--offline"])
        self.assertEqual(code, 0)
        commands = [process.command for process in fake.processes]
        for script in ("ci/check_markdown_links.py", "ci/check_help_text.py",
                       "ci/check_learning_bundle_release.py", "ci/check_brand.py"):
            self.position(commands, check.sys.executable, script)
        for process in fake.processes:
            self.assertEqual(process.kwargs["cwd"], check.ROOT)
            self.assertEqual(process.kwargs["env"]["CARGO_NET_OFFLINE"], "true")

    def test_unknown_scope_is_rejected_before_any_execution(self):
        with patch.object(check.subprocess, "run") as run, patch.object(check.subprocess, "Popen") as popen, \
                self.assertRaises(SystemExit) as error:
            self.run_main(["typo"])
        self.assertEqual(error.exception.code, 2)
        run.assert_not_called()
        popen.assert_not_called()

    def test_only_the_test_runs_get_the_temporary_directory(self):
        _, fake, _, _ = self.run_main(["core"])
        env = {process.label: process.kwargs["env"] for process in fake.processes}
        for name in ("TMPDIR", "TEMP", "TMP"):
            self.assertEqual(env["cargo test"][name], "/ram")
            self.assertEqual(env["cargo test opencode_adapter"][name], "/ram")
            for other in ("cargo fmt", "cargo clippy", "cargo build", "cargo test --no-run", "ci/check_native_sources.py"):
                self.assertNotEqual(env[other].get(name), "/ram")
        self.assertEqual(env["cargo test"]["MAGNESIUM_TEST_BINARY"], env["cargo build"]["MAGNESIUM_TEST_BINARY"])
        self.assertEqual(env["cargo test"]["MAGNESIUM_TEST_BINARY"], env["cargo test --no-run"]["MAGNESIUM_TEST_BINARY"])
        self.assertEqual(env["cargo test opencode_adapter"]["MAGNESIUM_TEST_BINARY"], env["cargo test"]["MAGNESIUM_TEST_BINARY"])

    def test_cargo_metadata_gives_the_integration_test_target_names(self):
        targets = [("lib", "magnesium"), ("bin", "magnesium"), ("test", "smoke"), ("test", "opencode_adapter"),
                   ("custom-build", "build-script-build")]
        metadata = json.dumps({"packages": [{"targets": [{"kind": [kind], "name": name} for kind, name in targets]}]})
        with patch.object(check.subprocess, "check_output", return_value=metadata) as output:
            self.assertEqual(check.test_targets(), ["smoke", "opencode_adapter"])
        self.assertEqual(output.call_args.args[0][:3], ["cargo", "metadata", "--no-deps"])
        with patch.object(check.subprocess, "check_output", return_value=json.dumps({"packages": [{"targets": []}] * 2})):
            with self.assertRaises(RuntimeError):
                check.test_targets()

    def test_the_split_runs_every_target_of_the_single_command_exactly_once(self):
        _, test = check.pick(check.commands("core", "test-host"), "cargo", "test")
        names = ["digest", "opencode_adapter", "smoke", "init"]
        main, side = check.split_tests(test, names)

        def tests_named(command):
            return [command[index + 1] for index, flag in enumerate(command) if flag == "--test"]

        self.assertEqual(tests_named(side), ["opencode_adapter"])
        self.assertCountEqual(tests_named(main) + tests_named(side), names)
        for flag in ("--lib", "--bins", "--benches", "--examples"):
            self.assertIn(flag, main)
            self.assertNotIn(flag, side)
        selection = {"--all-targets", "--lib", "--bins", "--benches", "--examples", "--test", *names}
        rest = [[word for word in command if word not in selection] for command in (main, side, test)]
        self.assertEqual(rest[0], rest[1])  # same lock, features, target and profile
        self.assertEqual(rest[0], rest[2])
        self.assertNotIn("--all-targets", main + side)

    def test_the_tests_are_not_split_when_the_side_target_or_the_target_flag_is_missing(self):
        _, test = check.pick(check.commands("core", "test-host"), "cargo", "test")
        self.assertIsNone(check.split_tests(test, ["digest", "smoke"]))
        self.assertIsNone(check.split_tests([*test, "--all-targets"], ["opencode_adapter"]))
        self.assertIsNone(check.split_tests([word for word in test if word != "--all-targets"], ["opencode_adapter"]))

    def test_without_the_side_target_one_command_runs_every_test(self):
        code, fake, out, _ = self.run_main(["core"], targets=("digest", "smoke"))
        self.assertEqual(code, 0)
        self.assertEqual(fake.started().count("cargo test"), 1)
        self.assertNotIn("cargo test opencode_adapter", fake.started())
        self.assertIn("--all-targets", fake.process("cargo test").command)
        self.assertEqual(fake.process("cargo test").kwargs["env"]["TMPDIR"], "/ram")
        self.assertIn("Running every Rust test in one command", out)

    def test_when_cargo_cannot_list_the_targets_one_command_runs_every_test(self):
        for error in (RuntimeError("two packages"), subprocess.CalledProcessError(101, "cargo"),
                      OSError("no cargo"), ValueError("not JSON"), KeyError("packages")):
            with self.subTest(error=error):
                code, fake, out, _ = self.run_main(["core"], targets=error)
                self.assertEqual(code, 0)
                self.assertEqual(fake.started().count("cargo test"), 1)
                self.assertNotIn("cargo test opencode_adapter", fake.started())
                self.assertIn("--all-targets", fake.process("cargo test").command)
                self.assertIn("Cargo could not list the test targets", out)

    def test_the_two_test_commands_run_at_the_same_time(self):
        # Each waits for the other to start; a runner that ran them one after the other would time out.
        code, fake, _, err = self.run_main(["core"], meet_labels={"cargo test", "cargo test opencode_adapter"})
        self.assertEqual((code, err), (0, ""))

    def test_both_test_commands_share_one_ram_disk_that_outlives_them(self):
        code, fake, _, _ = self.run_main(["core"])
        self.assertEqual(code, 0)
        events = fake.events
        self.assertEqual(events.count(("ram disk", "up")), 1)
        up, down = events.index(("ram disk", "up")), events.index(("ram disk", "down"))
        for name in ("cargo test", "cargo test opencode_adapter"):
            self.assertLess(up, events.index(("start", name)))
            self.assertLess(events.index(("end", name)), down)
        self.assertLess(down, events.index(("start", "ci/check_native_sources.py")))

    def test_the_main_test_command_streams_and_the_side_one_prints_whole_afterwards(self):
        _, fake, out, _ = self.run_main(["core"])
        main, side = fake.process("cargo test"), fake.process("cargo test opencode_adapter")
        self.assertIsNone(main.kwargs.get("stdout"))  # straight to the console as it runs
        self.assertIsNotNone(side.kwargs["stdout"])
        self.assertIs(side.kwargs["stdin"], check.subprocess.DEVNULL)
        shown_main, shown_side = shlex.join(main.command), shlex.join(side.command)
        self.assertIn(f"\n  (.) {shown_side}\ncargo test opencode_adapter line 1\ncargo test opencode_adapter line 2\n", out)
        self.assertLess(out.index(f"(.) {shown_main}"), out.index(f"(.) {shown_side}"))

    def test_a_failed_main_test_command_is_named_and_stops_the_side_one_and_the_native_smoke(self):
        code, fake, out, err = self.run_main(["core"], exit_codes={"cargo test": 101}, hang={"cargo test opencode_adapter"})
        self.assertEqual(code, 1)
        self.assertEqual(err.strip(), f"Failed: {shlex.join(fake.process('cargo test').command)} (exit 101)")
        self.assertTrue(fake.process("cargo test opencode_adapter").terminated.is_set())
        self.assertNotIn("cargo test opencode_adapter line", out)  # a stopped command prints nothing
        self.assertNotIn("ci/check_native_sources.py", fake.started())
        self.assertEqual(fake.events[-1], ("ram disk", "down"))

    def test_a_failed_side_test_command_is_named_and_shown_and_stops_the_main_one(self):
        code, fake, out, err = self.run_main(["core"], exit_codes={"cargo test opencode_adapter": 1}, hang={"cargo test"})
        self.assertEqual(code, 1)
        self.assertEqual(err.strip(), f"Failed: {shlex.join(fake.process('cargo test opencode_adapter').command)} (exit 1)")
        self.assertIn("cargo test opencode_adapter line 2", out)  # the failing test's output
        self.assertTrue(fake.process("cargo test").terminated.is_set())
        self.assertNotIn("ci/check_native_sources.py", fake.started())
        self.assertEqual(fake.events[-1], ("ram disk", "down"))

    def test_an_interrupt_stops_both_test_commands_and_still_releases_the_ram_disk(self):
        fake = FakeSubprocess(interrupt={"cargo test"}, hang={"cargo test opencode_adapter"})
        with self.assertRaises(KeyboardInterrupt):
            self.run_main(["core"], fake=fake)
        for name in ("cargo test", "cargo test opencode_adapter"):
            self.assertTrue(fake.process(name).terminated.is_set())
            self.assertIsNotNone(fake.process(name).returncode)
        self.assertEqual(fake.events[-1], ("ram disk", "down"))
        self.assertNotIn("ci/check_native_sources.py", fake.started())

    def test_list_and_other_scopes_never_create_a_ram_disk(self):
        with patch.object(check, "test_temp_dir") as ram:
            self.run_main(["--list"])
            self.run_main(["python", "docs"])
        ram.assert_not_called()


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


if __name__ == "__main__":
    unittest.main()
