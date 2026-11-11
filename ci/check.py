#!/usr/bin/env python3
"""Run Magnesium's local checks. They install nothing and call no model."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SCOPES = ("core", "python", "docs")


def commands(scope: str) -> list[list[str]]:
    """The commands of one scope, in the order they run."""
    python = sys.executable
    return {
        "core": [
            ["cargo", "fmt", "--all", "--check"],
            ["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
            ["cargo", "test", "--locked"],
        ],
        "python": [
            [python, "-m", "unittest", "discover", "-s", "ci", "-p", "test_*.py"],
            [python, "workers/docling/tests/test_gate.py"],
            [python, "workers/docling/tests/test_worker.py"],
        ],
        "docs": [
            [python, "ci/check_markdown_links.py"],
            [python, "ci/check_help_text.py"],
        ],
    }[scope]


def selected_scopes(requested: list[str]) -> list[str]:
    selected = set(SCOPES) if not requested or "all" in requested else set(requested)
    return [scope for scope in SCOPES if scope in selected]


def environment(*, offline: bool) -> dict[str, str]:
    env = os.environ.copy()
    # The tests start the `magnesium` that Cargo builds for them. A leftover
    # override would test some other build.
    env.pop("MAGNESIUM_TEST_BINARY", None)
    if offline:
        env["CARGO_NET_OFFLINE"] = "true"
    return env


def run(command: list[str], env: dict[str, str]) -> str | None:
    """Run one command from the repository root. Return what failed, or None."""
    shown = shlex.join(command)
    print(f"\n$ {shown}", flush=True)
    started = time.monotonic()
    try:
        code = subprocess.run(command, cwd=ROOT, env=env).returncode
    except FileNotFoundError:
        return f"Missing prerequisite: {command[0]}. See CONTRIBUTING.md."
    if code:
        return f"Failed: {shown} (exit {code})"
    print(f"  passed in {time.monotonic() - started:.1f} s", flush=True)
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scopes", nargs="*", metavar="SCOPE", help="all (default), core, python, or docs")
    parser.add_argument("--list", action="store_true", help="show the commands without running them")
    parser.add_argument("--offline", action="store_true", help="require Cargo's cached dependencies")
    args = parser.parse_args(argv)
    unknown = set(args.scopes) - {"all", *SCOPES}
    if unknown:
        parser.error(f"unknown scope: {', '.join(sorted(unknown))}")
    steps = [command for scope in selected_scopes(args.scopes) for command in commands(scope)]
    if args.list:
        for command in steps:
            print(shlex.join(command))
        return 0
    env = environment(offline=args.offline)
    started = time.monotonic()
    for command in steps:
        failure = run(command, env)
        if failure:
            print(failure, file=sys.stderr)
            return 1
    print(f"\nSelected local checks passed in {time.monotonic() - started:.0f} s. Live-model checks are separate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
