#!/usr/bin/env python3
"""Fail when a repository-local Markdown link names a missing path."""

from pathlib import Path
import re
import sys
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")


def local_target(raw_target: str) -> str | None:
    target = raw_target.strip().strip("<>").split(maxsplit=1)[0]
    if not target or target.startswith(("#", "http://", "https://", "mailto:")):
        return None
    return unquote(target.split("#", 1)[0])


def main() -> int:
    failures: list[str] = []
    for markdown in ROOT.rglob("*.md"):
        if any(part in {".git", "target", "node_modules", "runtime", "dist", "generated", "agent-workspace"} for part in markdown.relative_to(ROOT).parts):
            continue
        for line_number, line in enumerate(markdown.read_text(encoding="utf-8").splitlines(), 1):
            for match in LINK.finditer(line):
                target = local_target(match.group(1))
                if target and not (markdown.parent / target).resolve().exists():
                    failures.append(
                        f"{markdown.relative_to(ROOT)}:{line_number}: missing {target}"
                    )
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
