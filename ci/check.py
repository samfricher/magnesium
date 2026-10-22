#!/usr/bin/env python3
"""Run Magnesium's ordinary local checks without installing tools or calling a model."""
from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from typing import Iterator

ROOT = Path(__file__).resolve().parents[1]
Step = tuple[Path, list[str]]
SCOPES = ("core", "python", "docs")
# Set to anything but 0 to keep the default temporary directory, e.g. to reproduce a disk-specific failure.
DISK_TMP_OPT_OUT = "MAGNESIUM_CI_DISK_TMP"
RAM_DISK_SECTORS = 1048576  # 512 MiB; the whole suite peaks near 7 MB
RAM_DISK_MIN_FREE_BYTES = 32 * 1024 * 1024
SHM_DIR = Path("/dev/shm")
VOLUMES_DIR = Path("/Volumes")
# This test target waits on timers in a Node harness (about 20 s of wall time, almost no CPU),
# so it can run beside the other Rust tests without disturbing them.
SIDE_TEST = "opencode_adapter"
STOP_SIGNALS = [getattr(signal, name) for name in ("SIGTERM", "SIGHUP") if hasattr(signal, name)]


def commands(scope: str, target: str | None = None) -> list[Step]:
    """Keep targeted and aggregate validation on the same command definitions."""
    python = sys.executable
    native = ["--target", target or "<native-target>"]
    groups = {
        "core": [
            (ROOT, ["cargo", "fmt", "--all", "--check"]),
            (ROOT, ["cargo", "clippy", "--locked", "--all-targets", "--all-features", *native, "--", "-D", "warnings"]),
            (ROOT, ["cargo", "build", "--locked", "--release", *native]),
            (ROOT, ["cargo", "test", "--locked", "--all-targets", "--all-features", *native]),
            (ROOT, [python, "ci/check_native_sources.py"]),
        ],
        "python": [
            (ROOT, [python, "-m", "unittest", "discover", "-s", "ci", "-p", "test_*.py"]),
            (ROOT, [python, "-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py"]),
            (ROOT, [python, "workers/docling/tests/test_gate.py"]),
            (ROOT, [python, "workers/docling/tests/test_worker.py"]),
        ],
        "docs": [
            (ROOT, [python, "ci/check_markdown_links.py"]),
            (ROOT, [python, "ci/check_help_text.py"]),
            (ROOT, [python, "ci/check_learning_bundle_release.py", "--source-root", str(ROOT), "--status", str(ROOT / "release/learning-status.json")]),
            (ROOT, [python, "ci/check_brand.py"]),
        ],
    }
    return groups[scope]


def selected_scopes(requested: list[str]) -> list[str]:
    selected = set(SCOPES) if "all" in requested else set(requested)
    return [scope for scope in SCOPES if scope in selected]


def pick(steps: list[Step], *prefix: str) -> Step:
    """The one step whose command starts with prefix."""
    return next(step for step in steps if step[1][:len(prefix)] == list(prefix))


def plan(scopes: list[str], target: str | None = None) -> tuple[list[list[Step]], list[Step]]:
    """Split the selected checks into build-phase lanes and the steps that must run alone.

    Lanes run side by side, each in order. The Rust tests are load-sensitive and the
    native-source smoke writes durably, so both wait for every lane and then run one at
    a time, tests first (run_test_phase lets only the SIDE_TEST target overlap the other
    Rust tests). Clippy and the test-binary build share a lane because Cargo makes builds
    that share a build directory take turns anyway.
    """
    lanes: list[list[Step]] = []
    alone: list[Step] = []
    for scope in scopes:
        steps = commands(scope, target)
        if scope != "core":
            lanes.append(steps)
            continue
        cwd, test = pick(steps, "cargo", "test")
        lanes.append([pick(steps, "cargo", "build")])
        lanes.append([pick(steps, "cargo", "fmt"), pick(steps, "cargo", "clippy"), (cwd, [*test, "--no-run"])])
        alone += [(cwd, test), pick(steps, sys.executable, "ci/check_native_sources.py")]
    return lanes, alone


def native_target() -> str:
    output = subprocess.check_output(["rustc", "-vV"], cwd=ROOT, text=True, stderr=subprocess.STDOUT)
    for line in output.splitlines():
        if line.startswith("host: ") and line[6:].strip():
            return line[6:].strip()
    raise RuntimeError("rustc did not report its native host target")


def environment(*, offline: bool, target: str | None = None) -> dict[str, str]:
    env = os.environ.copy()
    executable = "magnesium.exe" if os.name == "nt" else "magnesium"
    # Build and subprocess tests must agree even when the caller overrides Cargo output.
    env["CARGO_TARGET_DIR"] = str(ROOT / "target")
    output = ROOT / "target"
    if target:
        output /= target
    env["MAGNESIUM_TEST_BINARY"] = str(output / "release" / executable)
    if offline:
        env["CARGO_NET_OFFLINE"] = "true"
    return env


def uses_test_temp_dir(command: list[str]) -> bool:
    """Only running the Rust tests creates many durable temporary workspaces; building them does not."""
    return command[:2] == ["cargo", "test"] and "--no-run" not in command


@contextlib.contextmanager
def stop_signals_raise() -> Iterator[None]:
    """Turn SIGTERM and SIGHUP into an exit so the cleanup in `finally` blocks still runs.

    Commands run in their own sessions, so a stopped run or a closed terminal reaches them only
    through that cleanup. A signal the caller ignores, as nohup does, stays ignored.
    """
    def handler(signum: int, frame: object) -> None:
        raise SystemExit(128 + signum)

    previous = {}
    try:
        for signum in STOP_SIGNALS:
            if signal.getsignal(signum) != signal.SIG_IGN:
                previous[signum] = signal.signal(signum, handler)
    except ValueError:  # not the main thread, so there is no handler to install
        pass
    try:
        yield
    finally:
        for signum, before in previous.items():
            signal.signal(signum, before or signal.SIG_DFL)


def eject_ram_disk(device: str) -> None:
    """Eject exactly the device this run attached; a busy volume gets retries, then a forced unmount."""
    eject = ["diskutil", "eject", device]
    try:
        for attempt in range(3):
            if subprocess.run(eject, capture_output=True).returncode == 0:
                return
            time.sleep(1)
        subprocess.run(["diskutil", "unmountDisk", "force", device], capture_output=True)
        if subprocess.run(eject, capture_output=True).returncode == 0:
            return
    except OSError:
        pass
    print(f"Could not eject the temporary RAM disk. Run: {shlex.join(eject)}", file=sys.stderr)


def attach_macos_ram_disk(stack: contextlib.ExitStack) -> str:
    attach = subprocess.run(["diskutil", "image", "attach", "--noMount", f"ram://{RAM_DISK_SECTORS}"],
                            check=True, capture_output=True, text=True)
    device = re.search(r"/dev/disk\d+", attach.stdout)
    if device is None:
        raise RuntimeError("diskutil did not report the RAM disk device")
    stack.callback(eject_ram_disk, device.group(0))
    name = f"magnesium-ci-{os.getpid()}"
    subprocess.run(["diskutil", "erasevolume", "APFS", name, device.group(0)],
                   check=True, capture_output=True, text=True)
    mount = VOLUMES_DIR / name
    if not os.path.ismount(mount):
        raise RuntimeError(f"{mount} is not mounted")
    (mount / ".metadata_never_index").touch()  # keep Spotlight off the churn and off the eject
    # The default macOS temporary directory sits behind the /var -> /private/var symlink, so the tests
    # have always covered workspace roots whose written and resolved paths differ. Reaching the RAM
    # disk through a symlink in that directory keeps that coverage. The link goes before the eject.
    links = tempfile.mkdtemp(prefix="magnesium-ci-")
    stack.callback(shutil.rmtree, links, ignore_errors=True)
    link = Path(links) / "ram"
    link.symlink_to(mount, target_is_directory=True)
    return str(link)


def create_shm_dir(stack: contextlib.ExitStack) -> str:
    if not (SHM_DIR.is_dir() and os.access(SHM_DIR, os.W_OK | os.X_OK)):
        raise RuntimeError(f"{SHM_DIR} is not a writable directory")
    stats = os.statvfs(SHM_DIR)
    if stats.f_flag & getattr(os, "ST_NOEXEC", 8):
        raise RuntimeError(f"{SHM_DIR} is mounted noexec")
    if stats.f_bavail * stats.f_frsize < RAM_DISK_MIN_FREE_BYTES:
        raise RuntimeError(f"{SHM_DIR} has too little free space")
    directory = tempfile.mkdtemp(prefix="magnesium-ci-", dir=SHM_DIR)
    stack.callback(shutil.rmtree, directory, ignore_errors=True)
    return directory


@contextlib.contextmanager
def test_temp_dir() -> Iterator[str | None]:
    """Yield a RAM-backed directory for test workspaces, or None to keep the default.

    The tests exercise durable writes (fsync) heavily; a physical disk makes that
    the dominant cost. Production behavior is untouched. Setup problems never fail
    the run, and the RAM disk is always released, including on Ctrl-C, SIGTERM and SIGHUP.
    """
    if os.environ.get(DISK_TMP_OPT_OUT, "0") not in ("", "0"):
        print(f"  Tests use the default temporary directory ({DISK_TMP_OPT_OUT} is set).", flush=True)
        yield None
        return
    with stop_signals_raise(), contextlib.ExitStack() as stack:
        directory = None
        try:
            if sys.platform == "darwin":
                directory = attach_macos_ram_disk(stack)
            elif sys.platform.startswith("linux"):
                directory = create_shm_dir(stack)
            else:
                raise RuntimeError(f"no RAM-backed directory for {sys.platform}")
        except (OSError, subprocess.SubprocessError, RuntimeError) as error:
            stack.close()
            detail = (getattr(error, "stderr", None) or str(error)).strip().splitlines()
            print(f"  Tests use the default temporary directory ({detail[-1] if detail else type(error).__name__}).", flush=True)
        else:
            print(f"  Tests use RAM-backed temporary files in {directory}", flush=True)
        yield directory


def with_temp_dir(env: dict[str, str], directory: str | None) -> dict[str, str]:
    """env with every temporary-file variable pointing at directory (unchanged when there is none)."""
    if directory is None:
        return env
    return {**env, "TMPDIR": directory, "TEMP": directory, "TMP": directory}


def run_command(cwd: Path, command: list[str], env: dict[str, str]) -> None:
    """Run one check; cargo test also gets its temporary files on RAM-backed storage."""
    if not uses_test_temp_dir(command):
        subprocess.run(command, cwd=cwd, env=env, check=True)
        return
    with test_temp_dir() as directory:
        subprocess.run(command, cwd=cwd, env=with_temp_dir(env, directory), check=True)


def test_targets() -> list[str]:
    """Names of the package's integration-test targets, as Cargo lists them."""
    output = subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
                                     cwd=ROOT, text=True, stderr=subprocess.DEVNULL)
    packages = json.loads(output)["packages"]
    if len(packages) != 1:
        raise RuntimeError(f"expected one Cargo package, found {len(packages)}")
    return [target["name"] for target in packages[0]["targets"] if "test" in target["kind"]]


def split_tests(command: list[str], names: list[str]) -> tuple[list[str], list[str]] | None:
    """Split `cargo test --all-targets` into every other test, then the SIDE_TEST target alone.

    --all-targets means the library, the binaries, benches, examples and every integration-test
    target, so naming each target except SIDE_TEST in the first command and SIDE_TEST in the second
    runs each of them exactly once. None means the split is not possible; the caller keeps the single
    command, so coverage never shrinks.
    """
    if SIDE_TEST not in names or command.count("--all-targets") != 1:
        return None
    start = command.index("--all-targets")
    others = [flag for name in names if name != SIDE_TEST for flag in ("--test", name)]
    return ([*command[:start], "--lib", "--bins", "--benches", "--examples", *others, *command[start + 1:]],
            [*command[:start], "--test", SIDE_TEST, *command[start + 1:]])


def split_for_overlap(command: list[str]) -> tuple[list[str], list[str]] | None:
    """split_tests with the package's real test targets; says why when it keeps the single command."""
    try:
        split = split_tests(command, test_targets())
        reason = f"there is no {SIDE_TEST} test target to run on its own"
    except (OSError, subprocess.SubprocessError, RuntimeError, ValueError, LookupError) as error:
        split, reason = None, f"Cargo could not list the test targets ({type(error).__name__})"
    if split is None:
        print(f"  Running every Rust test in one command: {reason}.", flush=True)
    return split


def echo(text: str) -> None:
    """Print child output even where the console cannot encode some of its characters."""
    encoding = getattr(sys.stdout, "encoding", None) or "utf-8"
    print(text.encode(encoding, "replace").decode(encoding), flush=True)


def spawn(command: list[str], cwd: Path, env: dict[str, str], **streams: object) -> subprocess.Popen:
    """Start a command in its own session, so terminate_group reaches every process it starts."""
    return subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True, **streams)


def terminate_group(child: subprocess.Popen, *, kill: bool = False) -> None:
    """Stop a command started by spawn and every process it started.

    Cargo does not pass SIGTERM on to the test binary it runs, and tests start processes of their
    own, such as the Node harness, so signaling only the command would leave those running.
    """
    if child.returncode is not None:  # reaped, so its process ID may belong to another process by now
        return
    if os.name == "nt":
        if kill:
            child.kill()
        else:
            child.terminate()
        return
    try:
        os.killpg(child.pid, signal.SIGKILL if kill else signal.SIGTERM)
    except (ProcessLookupError, PermissionError):  # nothing left that can be signaled (macOS says EPERM for zombies)
        pass


def run_build_phase(lanes: list[list[Step]], env: dict[str, str]) -> str | None:
    """Run the lanes at the same time, each in order, printing a command's whole output once it finishes.

    The first failure stops the phase: its output is shown, commands still running are
    terminated, and the returned message names the failed command. None means all passed.
    """
    lock = threading.Lock()
    running: list[subprocess.Popen] = []
    failures: list[str] = []

    def fail(message: str) -> None:  # the caller holds the lock
        failures.append(message)
        for child in running:
            terminate_group(child)

    def run_lane(lane: list[Step]) -> None:
        for cwd, command in lane:
            shown = shlex.join(command)
            with tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace") as log:
                with lock:
                    if failures:
                        return
                    try:
                        child = spawn(command, cwd, env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                    except FileNotFoundError:
                        fail(f"Missing prerequisite: {command[0]}. See CONTRIBUTING.md.")
                        return
                    running.append(child)
                code = child.wait()
                log.seek(0)
                output = log.read()
            with lock:
                running.remove(child)
                if failures:
                    return  # stopped because another command failed first
                print(f"\n  ({cwd.relative_to(ROOT)}) {shown}", flush=True)
                if output.strip():
                    echo(output.rstrip("\n"))
                if code:
                    fail(f"Failed: {shown} (exit {code})")
                    return

    def guarded(lane: list[Step]) -> None:
        try:
            run_lane(lane)
        except Exception as error:  # a crash here must fail the run, never pass it
            with lock:
                fail(f"Could not run the build phase: {error!r}")

    threads = [threading.Thread(target=guarded, args=(lane,)) for lane in lanes]
    try:
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
    except BaseException:  # Ctrl-C or a stop signal reaches only this process, so stop the commands here
        with lock:
            fail("interrupted")
        for thread in threads:
            if thread.is_alive():
                thread.join()
        raise
    return failures[0] if failures else None


def run_tests_together(cwd: Path, main: list[str], side: list[str], env: dict[str, str]) -> str | None:
    """Run the two cargo test commands at the same time on one RAM-backed temporary directory.

    The main command streams as it runs. The side command's output waits for the main one to end, so
    it cannot land in the middle of a line the main command is printing. The first failure is returned
    as a message and the other command is terminated; a side command stopped that way prints nothing.
    None means both passed.
    """
    lock = threading.Lock()
    failures: list[tuple[list[str], int]] = []
    children: list[subprocess.Popen] = []

    def wait_for(child: subprocess.Popen, command: list[str]) -> None:
        code = child.wait()
        with lock:
            if code and not failures:
                failures.append((command, code))
                for other in children:
                    if other is not child:
                        terminate_group(other)

    def stop(child: subprocess.Popen) -> None:
        """Reap a command that is still running because the run was interrupted."""
        if child.returncode is None:
            terminate_group(child)
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                terminate_group(child, kill=True)
                child.wait()

    with test_temp_dir() as directory, tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace") as log:
        env = with_temp_dir(env, directory)
        watcher = threading.Thread(target=lambda: wait_for(children[1], side))
        try:
            print(f"\n  ({cwd.relative_to(ROOT)}) {shlex.join(main)}", flush=True)
            children.append(spawn(main, cwd, env, stdin=subprocess.DEVNULL))
            children.append(spawn(side, cwd, env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT))
            watcher.start()
            wait_for(children[0], main)
            watcher.join()
        finally:
            for child in children:
                stop(child)
            if watcher.is_alive():
                watcher.join()
        failed, code = failures[0] if failures else (None, 0)
        if failed is not main:  # otherwise the side command was stopped half way
            print(f"\n  ({cwd.relative_to(ROOT)}) {shlex.join(side)}", flush=True)
            log.seek(0)
            output = log.read()
            if output.strip():
                echo(output.rstrip("\n"))
    return f"Failed: {shlex.join(failed)} (exit {code})" if failed else None


def run_test_phase(alone: list[Step], env: dict[str, str]) -> str | None:
    """Run the steps one at a time, except that the Rust tests run beside the SIDE_TEST target.

    None means everything passed; otherwise the message names what failed.
    """
    for cwd, command in alone:
        try:
            split = split_for_overlap(command) if uses_test_temp_dir(command) else None
            if split:
                failure = run_tests_together(cwd, *split, env)
                if failure:
                    return failure
                continue
            print(f"\n  ({cwd.relative_to(ROOT)}) {shlex.join(command)}", flush=True)
            run_command(cwd, command, env)
        except FileNotFoundError:
            return f"Missing prerequisite: {command[0]}. See CONTRIBUTING.md."
        except subprocess.CalledProcessError as error:
            return f"Failed: {shlex.join(command)} (exit {error.returncode})"
    return None


def show_plan(lanes: list[list[Step]], alone: list[Step]) -> None:
    print("\n[build phase] lanes run at the same time; a lane runs its commands in order")
    for number, lane in enumerate(lanes, 1):
        print(f"  lane {number}")
        for cwd, command in lane:
            print(f"    ({cwd.relative_to(ROOT)}) {shlex.join(command)}")
    if alone:
        print("\n[test phase] one step at a time, in order, after every build-phase command has finished")
        for cwd, command in alone:
            print(f"  ({cwd.relative_to(ROOT)}) {shlex.join(command)}")
            if uses_test_temp_dir(command):
                main, side = split_tests(command, [SIDE_TEST, f"<every-test-target-except-{SIDE_TEST}>"])
                print(f"    runs as these two commands at the same time, so the {SIDE_TEST} test, which mostly waits, overlaps the rest")
                print("    (if Cargo cannot list the test targets, the command above runs by itself instead):")
                print(f"      {shlex.join(main)}")
                print(f"      {shlex.join(side)}")
                print(f"    (test temporary files go to a RAM disk or /dev/shm when available; set {DISK_TMP_OPT_OUT}=1 to opt out)")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scopes", nargs="*", metavar="SCOPE", help="all (default), core, python, or docs")
    parser.add_argument("--list", action="store_true", help="show commands without running them")
    parser.add_argument("--offline", action="store_true", help="require Cargo's cached dependencies")
    args = parser.parse_args(argv)
    unknown = set(args.scopes) - {"all", *SCOPES}
    if unknown:
        parser.error(f"unknown scope: {', '.join(sorted(unknown))}")
    scopes = selected_scopes(args.scopes or ["all"])
    try:
        target = ("<native-target>" if args.list else native_target()) if "core" in scopes else None
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"Cannot determine the native Rust target: {error}", file=sys.stderr)
        return 1
    lanes, alone = plan(scopes, target)
    if args.list:
        show_plan(lanes, alone)
        return 0
    env = environment(offline=args.offline, target=target)
    with stop_signals_raise():
        print("\n[build phase] commands run at the same time; each one's output appears when it finishes", flush=True)
        failure = run_build_phase(lanes, env)
        if failure:
            print(failure, file=sys.stderr)
            return 1
        if alone:
            print(f"\n[test phase] runs alone: the Rust tests are sensitive to CPU load, so only the {SIDE_TEST} test runs beside them", flush=True)
        failure = run_test_phase(alone, env)
    if failure:
        print(failure, file=sys.stderr)
        return 1
    print("\nSelected local checks passed. Installer and live-model checks are separate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
