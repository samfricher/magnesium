#!/usr/bin/env python3
"""Fail on leftover names from before the product was called Magnesium.

Usage: check_brand.py [--root DIR]. Scans the Git-tracked text files under DIR
(default: this repository), prints one line per leftover and exits 1 when any exist.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EXEMPT = {"ci/check_brand.py", "ci/test_check_brand.py"}

OLD_NAMES = {
    "old command": re.compile(r"(?<![\w./:@-])learn (?:init|digest|export|info|update|adapter|memory|_internal|help)\b|(?<![\w./:@-])learn (?=\d|--version)|\binstall learn\b|\blearn\(1\)"),
    "old plugin id": re.compile(r"@learn\b|\bcodex-plugin@"),
    "old namespace": re.compile(r"(?<![\w-])learn:(?=[\w{])"),
    "old MCP rule": re.compile(r"\bmcp__plugin_learn"),
    "old control folder": re.compile(r"(?<![\w.])\.learn(?!\w)"),
    "old ignore file": re.compile(r"learnignore", re.I),
    "old environment prefix": re.compile(r"(?<![A-Za-z0-9])LEARN_"),
    "old repository": re.compile(r"\bsamfricher/[Ll]earn\b"),
    "old name literal": re.compile(r"""(?<![\w-])["'`]learn["'`]"""),
    "old file name": re.compile(r"\blearn\.(?:exe|js|rb|wxs)\b|\bLEARN\.md\b"),
    "old name prefix": re.compile(r"(?<![\w-])learn-\w"),
    "old path segment": re.compile(r"(?<=/)learn(?![\w./:-])|\bui://learn\b"),
    "old name domain": re.compile(r"(?<![\w/.-])learn/(?=\w)"),
    "old identifier": re.compile(r"(?<=[a-z])Learn(?=[A-Z])|\bLearn(?=[A-Z])|\blearn(?=[A-Z])|(?<![A-Za-z])learn_|_learn(?![a-z])"),
    "old product noun": re.compile(r"\blearn (?:runtime|binary|command|plugin|server|marketplace|executable|package|CLI)\b"),
    "old product name": re.compile(r"\bLearn\b"),
}

# A leftover inside one of these spans is not a brand name. The patterns above never
# match learner, learning, learner-model/v2 or learning_step, so those need no entry.
ALLOWED = (
    # The tutoring skill is named learn: /magnesium:learn, Skill(magnesium:learn), skills/learn/.
    re.compile(r"\bmagnesium:learn\b"),
    re.compile(r"\bskills/learn\b"),
    re.compile(r"""\.join\("skills"\)\.join\("learn"\)"""),
    # The verb, where it starts a sentence or a goal.
    re.compile(r"\bLearn (?:coefficient roles|one week\.|from your materials\.)"),
)


def leftovers(text: str) -> list[tuple[int, str, str]]:
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        allowed = [match.span() for pattern in ALLOWED for match in pattern.finditer(line)]
        for name, pattern in OLD_NAMES.items():
            for match in pattern.finditer(line):
                start, end = match.span()
                if not any(low <= start and end <= high for low, high in allowed):
                    found.append((number, name, match.group()))
    return found


def tracked_text(root: Path):
    listing = subprocess.run(["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True).stdout
    for name in sorted(filter(None, listing.decode().split("\0"))):
        path = root / name
        if name in EXEMPT or path.is_symlink() or not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        try:
            yield name, data.decode("utf-8")
        except UnicodeDecodeError:
            continue


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    problems = [
        f"{name}:{number}: {kind}: {text!r}"
        for name, content in tracked_text(args.root)
        for number, kind, text in leftovers(content)
    ]
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
