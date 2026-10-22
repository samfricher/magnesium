#!/usr/bin/env python3
"""Run Magnesium's ordinary local checks without installing tools or calling a model."""
from __future__ import annotations

import argparse
import contextlib
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from typing import Iterator

ROOT = Path(__file__).resolve().parents[1]
SCOPES = ("core", "python", "docs")
# Set to anything but 0 to keep the default temporary directory, e.g. to reproduce a disk-specific failure.
DISK_TMP_OPT_OUT = "MAGNESIUM_CI_DISK_TMP"
RAM_DISK_SECTORS = 1048576  # 512 MiB; the whole suite peaks near 7 MB
RAM_DISK_MIN_FREE_BYTES = 32 * 1024 * 1024
SHM_DIR = Path("/dev/shm")
VOLUMES_DIR = Path("/Volumes")


def commands(scope: str, target: str | None = None) -> list[tuple[Path, list[str]]]:
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
    """Only the Rust tests create many durable temporary workspaces."""
    return command[:2] == ["cargo", "test"]


@contextlib.contextmanager
def sigterm_raises() -> Iterator[None]:
    """Turn SIGTERM into an exit so the cleanup in `finally` blocks still runs."""
    def handler(signum: int, frame: object) -> None:
        raise SystemExit(128 + signum)

    try:
        previous = signal.signal(signal.SIGTERM, handler)
    except ValueError:  # not the main thread, so there is no handler to install
        yield
        return
    try:
        yield
    finally:
        signal.signal(signal.SIGTERM, previous or signal.SIG_DFL)


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
    return str(mount)


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
    the run, and the RAM disk is always released, including on Ctrl-C and SIGTERM.
    """
    if os.environ.get(DISK_TMP_OPT_OUT, "0") not in ("", "0"):
        print(f"  Tests use the default temporary directory ({DISK_TMP_OPT_OUT} is set).", flush=True)
        yield None
        return
    with sigterm_raises(), contextlib.ExitStack() as stack:
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


def run_command(cwd: Path, command: list[str], env: dict[str, str]) -> None:
    """Run one check; cargo test also gets its temporary files on RAM-backed storage."""
    if not uses_test_temp_dir(command):
        subprocess.run(command, cwd=cwd, env=env, check=True)
        return
    with test_temp_dir() as directory:
        if directory is not None:
            env = {**env, "TMPDIR": directory, "TEMP": directory, "TMP": directory}
        subprocess.run(command, cwd=cwd, env=env, check=True)


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
    env = environment(offline=args.offline, target=target)
    for scope in scopes:
        print(f"\n[{scope}]", flush=True)
        for cwd, command in commands(scope, target):
            print(f"  ({cwd.relative_to(ROOT)}) {shlex.join(command)}", flush=True)
            if args.list:
                if uses_test_temp_dir(command):
                    print(f"    (test temporary files go to a RAM disk or /dev/shm when available; set {DISK_TMP_OPT_OUT}=1 to opt out)", flush=True)
                continue
            try:
                run_command(cwd, command, env)
            except FileNotFoundError:
                print(f"Missing prerequisite: {command[0]}. See CONTRIBUTING.md.", file=sys.stderr)
                return 1
            except subprocess.CalledProcessError as error:
                print(f"Failed: {shlex.join(command)} (exit {error.returncode})", file=sys.stderr)
                return 1
    if not args.list:
        print("\nSelected local checks passed. Installer and live-model checks are separate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
