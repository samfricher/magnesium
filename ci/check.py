#!/usr/bin/env python3
"""Run Magnesium's ordinary local checks without installing tools or calling a model."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SCOPES = ("core", "python", "docs")


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
                continue
            try:
                subprocess.run(command, cwd=cwd, env=env, check=True)
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
