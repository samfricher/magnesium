from __future__ import annotations

import re
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from pathlib import Path

import transcript
from claude_code import SignedOut, add_usage
from common import Fixture, load, now_local, table
from judge import Judge

WHEN = re.compile(r"^(\|\s*When\s*\|)\s*[^|]*\|", re.M)


SITTING_GAP_MINUTES = 181


def set_test_date(place, target: str | None, days: int, base=None):
    if not target:
        return
    plan = place.folder / target / "plan.md"
    if not plan.is_file():
        return
    day = ((base or now_local()) + timedelta(days=days)).date()
    text = WHEN.sub(lambda m: f"{m.group(1)} {day.isoformat()} ({day.strftime('%a')}) 10:00 to 10:50 |", plan.read_text(encoding="utf-8"))
    plan.write_text(text, encoding="utf-8")


def seed(adapter, place, fixture: Fixture, spec: dict | None, new_sitting: bool = False) -> dict:
    if not spec:
        return {"kind": "none"}
    if spec["kind"] == "answers":
        if spec.get("test_in_days") is not None:
            set_test_date(place, fixture.target, spec["test_in_days"])
        done = adapter.seed(place, spec["turns"], end_sitting=spec.get("end_sitting", False))
        seeded = {"kind": "answers", "answers": len(done), "test_in_days": spec.get("test_in_days")}
        if new_sitting:
            gap = int((now_local() - timedelta(minutes=SITTING_GAP_MINUTES)).timestamp() * 1000)
            last = min(max(d["at_ms"] for d in done), gap)
            adapter.last_learner_message_at(place, last)
            seeded["last_learner_message"] = datetime.fromtimestamp(last / 1000).isoformat(timespec="minutes")
        seeded["progress_due"] = adapter.progress(place).get("due")
        return seeded
    if spec["kind"] == "lesson":
        turns = adapter.scripted_lesson(place, spec["script"], conversation="earlier-chat")
        return {"kind": "lesson", "turns": len(turns), "open_task": (adapter.lesson(place) or {}).get("lesson", {}).get("open_task")}
    raise SystemExit(f"unknown seed {spec['kind']}")


def exchange(turns: list[dict], upto: int, only: list[int] | None = None) -> str:
    parts = []
    for n, turn in enumerate(turns[:upto], 1):
        if only and n not in only and n != 1:
            parts.append(f"LEARNER ({n}): {turn['learner']}\nTUTOR ({n}): [earlier reply omitted]")
            continue
        parts.append(f"LEARNER ({n}): {turn['learner']}\nTUTOR ({n}): {turn['tutor']}")
    return "\n\n".join(parts)


def open_task(state: dict | None) -> dict | None:
    return ((state or {}).get("lesson") or {}).get("open_task")


def decide(rule: dict, turns: list[dict], adapter, judge: Judge | None) -> dict:
    kind = rule["kind"]
    n = rule.get("turn")
    reply = turns[n - 1]["tutor"] if n and n <= len(turns) else ""
    if kind == "contains":
        words = rule.get("all") or rule.get("any") or []
        hits = [w for w in words if w in reply]
        ok = (len(hits) == len(words)) if rule.get("all") else bool(hits)
        return {"kind": kind, "how": "mechanical", "pass": ok, "fact": f"reply {n} contains {hits or 'none'} of {words}"}
    if kind == "no_open_task":
        task = open_task(turns[-1]["state"])
        return {"kind": kind, "how": "mechanical", "pass": task is None,
                "fact": "magnesium lesson shows no open task" if task is None else f"open task: {task.get('text', '')[:160]}"}
    if kind == "open_task_after":
        task = open_task(turns[n - 1]["state"])
        return {"kind": kind, "how": "mechanical", "pass": task is not None,
                "fact": f"after turn {n}: " + (task.get("text", "")[:160] if task else "no open task")}
    if kind == "open_task_kept":
        a, b = open_task(turns[rule["from"] - 1]["state"]), open_task(turns[rule["to"] - 1]["state"])
        same = bool(a and b and (a.get("posed_at_ms") == b.get("posed_at_ms") or a.get("text") == b.get("text")))
        return {"kind": kind, "how": "mechanical", "pass": bool(a and b),
                "fact": f"open after turn {rule['from']}: {bool(a)}; after turn {rule['to']}: {bool(b)}; same task: {same}"}
    if kind == "posed_check":
        said = adapter.posed_check(reply)
        posed = [ws for ws in rule.get("words", []) if all(w in reply for w in ws)]
        task = open_task(turns[n - 1]["state"]) or {}
        runtime = task.get("kind") == "quick_check"
        named = bool(re.search(r"\bcheck\b", reply, re.I)) and any(re.search(re.escape(w), reply, re.I) for w in rule.get("labels", []))
        ok = said or bool(posed) or runtime or named
        return {"kind": kind, "how": "mechanical", "pass": ok,
                "fact": f"reply {n}: says 'quick check N' {said}; poses a stored check question {bool(posed)}; "
                        f"runtime sees a quick check open {runtime}; names a due skill as a check {named}"}
    if kind == "tool_calls":
        tools = turns[n - 1]["tools"]
        first = next((i for i, t in enumerate(tools) if adapter.is_reply_tool(t)), len(tools))
        before = tools[:first]
        runtime = [t for t in before if adapter.is_runtime_tool(t)]
        return {"kind": kind, "how": "recorded", "pass": None,
                "fact": f"{len(before)} tool calls before the reply ({len(runtime)} Magnesium), {len(tools)} in the turn",
                "tool_calls": len(before), "runtime_calls": len(runtime), "turn_calls": len(tools), "tools": tools}
    if kind == "stated_due_checks":
        stated = rule.get("stated") or {}
        due = stated.get("hook_due")
        return {"kind": kind, "how": "setup", "pass": None, "setup_ok": bool(due) or bool(stated.get("opener_delivered")),
                "fact": f"the runtime stated due checks: {due if due is not None else 'none'}; opener delivered: {stated.get('opener_delivered')}"}
    if kind == "judged":
        if judge is None:
            return {"kind": kind, "how": "judged", "pass": None, "fact": "no judge"}
        verdict = judge.verdict(rule["rubric"], exchange(turns, n, rule.get("context")))
        return {"kind": kind, "how": "judged", "pass": verdict["pass"], "fact": verdict["sentence"], "why": verdict["why"]}
    raise SystemExit(f"unknown rule {kind}")


def one_run(args, adapter, fixture: Fixture, scene: dict, number: int, judge: Judge | None, sections: list[dict]) -> dict:
    name = f"scene-{scene['id']}-{number}"
    started = time.monotonic()
    place = adapter.place(fixture.folder, name, live=True, surface="window")
    seeded = seed(adapter, place, fixture, scene.get("seed"), bool(scene.get("new_sitting")))
    conversation = adapter.live(place, args.model, args.effort, place.root / "stream.jsonl")
    usage: dict = {}
    turns = []
    error = None
    try:
        for message in scene["messages"]:
            reply = conversation.say(message, timeout=args.turn_timeout)
            add_usage(usage, reply.usage)
            said = "\n".join(t for t in (adapter.tutor_text(x["name"], x["input"]) for x in reply.tools) if t) or reply.text
            turns.append({"learner": message, "tutor": said, "tools": [x["name"] for x in reply.tools],
                          "seconds": round(reply.seconds, 1), "usage": reply.usage, "cost_usd": reply.cost_usd,
                          "error": reply.error, "state": adapter.lesson(place)})
            if reply.error:
                error = reply.error
                break
    except SignedOut as signed:
        conversation.close()
        return {"scene": scene["id"], "run": number, "signed_out": str(signed)}
    conversation.close()
    path = adapter.transcript_of(place, conversation.session_id) if conversation.session_id else None
    run = {"scene": scene["id"], "run": number, "seed": seeded, "turns": turns, "error": error,
           "minutes": round((time.monotonic() - started) / 60, 2), "usage": usage, "tokens": sum(usage.values()),
           "cost_usd": round(sum(t["cost_usd"] for t in turns), 3), "transcript": str(path or ""),
           "stated": transcript.stated_checks(path) if path else {},
           "real_use_counts": transcript.real_use_counts(path, sections, adapter.tutor_text) if path else None}
    return settle(run, scene, adapter, judge)


def settle(run: dict, scene: dict, adapter, judge: Judge | None) -> dict:
    rules = [dict(rule, stated=run.get("stated")) if rule["kind"] == "stated_due_checks" else rule for rule in scene["decide"]]
    decisions = [] if run.get("error") else [decide(rule, run["turns"], adapter, judge) for rule in rules]
    verdicts = [d["pass"] for d in decisions if d["pass"] is not None]
    setup = [d for d in decisions if d["how"] == "setup"]
    run["setup_ok"] = all(d["setup_ok"] for d in setup)
    run["decisions"] = decisions
    run["how"] = sorted({d["how"] for d in decisions if d["how"] not in ("recorded", "setup")})
    run["pass"] = (all(verdicts) and bool(verdicts)) if not run.get("error") else False
    run["counted"] = run["setup_ok"] and not run.get("error")
    return run


def merge_counts(runs: list[dict]) -> dict:
    total = {"explanation_requests": defaultdict(int), "stalls": defaultdict(int), "detours_to_earlier_step": defaultdict(int),
             "teaching_replies": 0, "teaching_replies_setting_no_task": 0}
    for run in runs:
        counts = run.get("real_use_counts") or {}
        for key in ("explanation_requests", "stalls", "detours_to_earlier_step"):
            for section, value in (counts.get(key) or {}).items():
                total[key][section] += value
        total["teaching_replies"] += counts.get("teaching_replies", 0)
        total["teaching_replies_setting_no_task"] += counts.get("teaching_replies_setting_no_task", 0)
    return {k: dict(v) if isinstance(v, defaultdict) else v for k, v in total.items()} | {
        "planned_items_against_parts_marked": "no data", "method": "mechanical (word match on the transcripts)"}


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    spec = load(fixture.file("scenes", "scenes.json"))
    scenes = [s for s in spec["scenes"] if not args.scene or s["id"] in args.scene]
    sections = ((fixture.lists() or {}).get("order") or {}).get("sections", [])
    judge = Judge(adapter.work, args.model, args.effort)
    started = time.monotonic()
    if args.rejudge:
        earlier = load(args.rejudge)
        by_id = {s["id"]: s for s in scenes}
        runs = []
        for r in earlier["runs"]:
            if r["scene"] not in by_id:
                continue
            if "stated" not in r and r.get("transcript") and Path(r["transcript"]).is_file():
                r["stated"] = transcript.stated_checks(Path(r["transcript"]))
            runs.append(settle(r, by_id[r["scene"]], adapter, judge))
        kept = {r["scene"] for r in runs}
        scenes = [s for s in scenes if s["id"] in kept]
    else:
        jobs = [(scene, n) for scene in scenes for n in range(1, args.runs + 1)]
        with ThreadPoolExecutor(max_workers=args.parallel) as pool:
            futures = [pool.submit(one_run, args, adapter, fixture, scene, n, judge, sections) for scene, n in jobs]
            runs = [f.result() for f in futures]
    signed = [r for r in runs if r.get("signed_out")]
    if signed:
        text = "Check 4, scenes: Claude Code reported it is signed out, so the check stopped.\n" + signed[0]["signed_out"]
        return {"check": "scenes", "signed_out": signed[0]["signed_out"], "runs": runs}, text
    rows, summary = [], []
    for scene in scenes:
        everything = [r for r in runs if r["scene"] == scene["id"]]
        mine = [r for r in everything if r.get("counted", True)]
        passed = sum(1 for r in mine if r["pass"])
        how = sorted({h for r in mine for h in r["how"]})
        facts = [d for r in mine for d in r["decisions"] if d["kind"] == "tool_calls"]
        extra = f"; tool calls to pick up: {', '.join(str(d['tool_calls']) for d in facts)}" if facts else ""
        errors = [r["error"] for r in mine if r.get("error")]
        unset = len(everything) - len(mine) - len(errors)
        note = (f"errors: {len(errors)}" if errors else "") + extra
        if unset:
            note += f"{'; ' if note else ''}{unset} not counted: the runtime stated no due checks (no new sitting)"
        rows.append([scene["id"], scene["title"], f"{passed} of {len(mine)}", " and ".join(how) or "none", note])
        summary.append({"scene": scene["id"], "title": scene["title"], "passed": passed, "runs": len(mine),
                        "runs_started": len(everything), "not_counted": unset, "how": how,
                        "pickup_tool_calls": [d["tool_calls"] for d in facts], "errors": errors})
    minutes = round((time.monotonic() - started) / 60, 1)
    if args.rejudge:
        minutes = earlier.get("minutes", minutes)
    tokens = sum(r.get("tokens", 0) for r in runs)
    cost = round(sum(r.get("cost_usd", 0) for r in runs), 2)
    counts = merge_counts(runs)
    lines = [
        f"Check 4, scenes on {fixture.name}: {len(scenes)} scenes, {args.runs} runs each, {args.model} at {args.effort}, "
        f"{args.parallel} at a time.",
        "",
        table(["Scene", "What happens", "Passed", "Decided by", "Notes"], rows, {1: 60}),
        "",
        ("Rejudged from saved runs. " if args.rejudge else "") + f"Minutes: {minutes} wall clock. Lesson tokens: {tokens:,}. Reported cost: ${cost}. "
        f"Judge: {judge.calls} calls, {sum(judge.usage.values()):,} tokens.",
        "",
        "Counts kept for later real use (word match, rough):",
        f"  requests for an explanation, by section: {counts['explanation_requests'] or 'none'}",
        f"  stalls, by section: {counts['stalls'] or 'none'}",
        f"  teaching replies: {counts['teaching_replies']}, of which set no task: {counts['teaching_replies_setting_no_task']}",
        f"  detours to an earlier step, by section: {counts['detours_to_earlier_step'] or 'none'}",
        "  planned items against parts marked: no data",
        "",
        "Limit: main has no clock control. 'Yesterday' is a model-reported learner message dated the day before the live chat. "
        "Magnesium opens a new sitting three hours after the last learner message it stored, so a scene that needs one sets that "
        "time in the scratch copy to its last seeded message; a run where the runtime still states no due checks is not counted.",
    ]
    result = {"check": "scenes", "fixture": fixture.name, "adapter": adapter.name, "model": args.model, "effort": args.effort,
              "runs_per_scene": args.runs, "summary": summary, "minutes": minutes, "tokens": tokens, "cost_usd": cost,
              "judge": judge.cost(), "real_use_counts": counts, "runs": runs}
    return result, "\n".join(lines)
