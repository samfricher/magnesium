#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import adapter as adapters
import check_coverage
import check_evidence
import check_plan_build
import check_scenes
import check_turn_cost
from common import default_out, emit

CHECKS = {
    "coverage": check_coverage,
    "evidence": check_evidence,
    "turn-cost": check_turn_cost,
    "scenes": check_scenes,
    "plan-build": check_plan_build,
}
LIVE = {"scenes", "plan-build"}


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="yardstick/run.py", description="Measure Magnesium with the same five checks every phase.")
    p.add_argument("check", choices=sorted(CHECKS))
    p.add_argument("--fixture", type=Path, help="a fixture folder with fixture.json")
    p.add_argument("--binary", type=Path, required=True, help="the magnesium binary to measure")
    p.add_argument("--plugin", type=Path, help="the Claude Code plugin folder for live runs (default: the one init installs)")
    p.add_argument("--out", type=Path, default=default_out(), help="where results are written")
    p.add_argument("--work", type=Path, help="where scratch run folders go (default: a runs folder beside --out)")
    p.add_argument("--adapter", default="today", choices=sorted(adapters.ADAPTERS), help="how the checks reach the runtime")
    p.add_argument("--model", default="claude-sonnet-5-5")
    p.add_argument("--effort", default="medium")
    p.add_argument("--mode", default=None, help="turn-cost: transcript or scripted; plan-build: score or live")
    p.add_argument("--transcript", help="turn-cost transcript mode: a lesson .jsonl, instead of the fixture's transcripts")
    p.add_argument("--days", nargs="*", help="turn-cost transcript mode: only these days (YYYY-MM-DD)")
    p.add_argument("--runs", type=int, default=3, help="scenes: runs per scene")
    p.add_argument("--scene", nargs="*", help="scenes: only these scene ids")
    p.add_argument("--parallel", type=int, default=3, help="scenes: live runs at a time")
    p.add_argument("--rejudge", type=Path, help="scenes: apply the current rules to the runs saved in this result file, with no new live runs")
    p.add_argument("--host", help="turn-cost transcript mode: only the fixture's transcripts from this host (window or claude-code)")
    p.add_argument("--turn-timeout", type=float, default=1800, help="seconds to wait for one live reply")
    p.add_argument("--plan", help="plan-build score mode: score this plan folder instead of the fixture's plans")
    p.add_argument("--planning", default="lesson", choices=["lesson", "own-run"], help="plan-build: where planning happens")
    p.add_argument("--judge", action="store_true", help="plan-build score mode: also run the judged counts")
    p.add_argument("--keep-plan", action="store_true", help="plan-build live: keep the fixture's own plan folder in the copy")
    p.add_argument("--builds", type=int, default=1, help="plan-build live: how many separate builds, run --parallel at a time")
    p.add_argument("--settle-index", action="store_true", help="plan-build live: let the source index finish before Claude Code starts")
    p.add_argument("--refresh-attempts", action="store_true", help="evidence: rewrite the fixture's attempts.json from a replay of its scripted record")
    return p


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    if args.mode is None:
        args.mode = {"turn-cost": "transcript", "plan-build": "score"}.get(args.check)
    if args.check == "turn-cost" and args.mode not in ("transcript", "scripted"):
        raise SystemExit("turn-cost --mode is transcript or scripted")
    if args.check == "plan-build" and args.mode not in ("score", "live"):
        raise SystemExit("plan-build --mode is score or live")
    if not args.fixture and not (args.check == "turn-cost" and args.transcript):
        raise SystemExit("--fixture is needed")
    if "fable" in args.model.lower():
        raise SystemExit("Live runs never use a Fable model")
    args.out = args.out.resolve()
    work = (args.work or args.out.parent / "runs").resolve()
    runtime = adapters.make(args.adapter, args.binary, args.plugin, work)
    result, text = CHECKS[args.check].run(args, runtime)
    result.setdefault("binary", str(args.binary))
    result.setdefault("binary_version", runtime.version())
    name = result.get("fixture") or result.get("name") or "result"
    suffix = f"-{args.mode}" if args.check in ("turn-cost", "plan-build") else ""
    emit(args.out, args.check + suffix, name, result, text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
