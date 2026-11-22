from __future__ import annotations

import shutil
import time
from pathlib import Path

import planscore
from claude_code import SignedOut, add_usage
from common import Fixture, moment, now_local, table
from judge import Judge

FOLLOW_UPS = [
    "Use your defaults for anything I have not answered, and write the plan now.",
    "Please write the plan file now. I will answer anything else later.",
    "Go ahead and finish the plan with what you have.",
]


def judge_for(judge: Judge | None):
    if judge is None:
        return None

    def run(folder, lines, sections, uncovered, cut_lines):
        prompt = planscore.judge_prompt(folder, sections, uncovered)
        answer = judge.ask(prompt, planscore.SCHEMA)
        for _ in range(2):
            if len(answer.get("sections") or []) == len(sections) and len(answer.get("carried") or []) == len(uncovered):
                break
            answer = judge.ask(prompt + f"\n\nGive exactly {len(sections)} entries under sections and {len(uncovered)} under carried, "
                               "one for each item listed above, in that order.", planscore.SCHEMA)
        judged = planscore.read_judged(answer)
        judged["attempts_matched"] = len(answer.get("sections") or []) == len(sections)
        return judged

    return run


def plans_in(folder: Path, pattern: str = "*/plan.md") -> dict[str, float]:
    study = folder / "Study"
    if not study.is_dir():
        return {}
    return {str(p.parent.relative_to(folder)): p.stat().st_mtime for p in study.glob(pattern)}


def request_for(fixture: Fixture, adapter) -> str:
    test = moment(fixture.test_at)
    start = moment(fixture.as_of)
    if fixture.data.get("plan_request"):
        text = fixture.data["plan_request"]
    elif test and test < now_local() and start:
        hours = round((test - start).total_seconds() / 3600)
        text = f"Make me a study plan for {fixture.test_label}. The test starts {hours} hours from now."
    elif fixture.test_at:
        text = f"Make me a study plan for {fixture.test_label} on {fixture.test_at}."
    else:
        text = f"Make me a study plan for this goal: {fixture.goal or 'the material in this folder'}"
    return adapter.papers_line(text) if fixture.test_at else adapter.start_line(text)


def live(args, adapter, fixture: Fixture, judge: Judge | None) -> dict:
    place = adapter.place(fixture.folder, f"plan-build-{fixture.name}", live=True)
    try:
        return build(args, adapter, fixture, judge, place)
    finally:
        adapter.release(place)


def build(args, adapter, fixture: Fixture, judge: Judge | None, place) -> dict:
    removed = None
    if fixture.target and (place.folder / fixture.target).exists() and not args.keep_plan:
        shutil.rmtree(place.folder / fixture.target)
        removed = fixture.target
    before = plans_in(place.folder)
    before_any = plans_in(place.folder, "*/*.md")
    log = place.root / "stream.jsonl"
    conversation = adapter.live(place, args.model, args.effort, log)
    usage: dict = {}
    turns = []
    started = time.monotonic()
    message = request_for(fixture, adapter)
    built: list[str] = []
    try:
        for n in range(1 + len(FOLLOW_UPS)):
            reply = conversation.say(message, timeout=args.turn_timeout)
            add_usage(usage, reply.usage)
            said = "\n".join(t for t in (adapter.tutor_text(x["name"], x["input"]) for x in reply.tools) if t) or reply.text
            turns.append({"learner": message, "tutor": said, "tools": [x["name"] for x in reply.tools], "seconds": round(reply.seconds, 1),
                          "usage": reply.usage, "cost_usd": reply.cost_usd, "error": reply.error})
            now = plans_in(place.folder)
            built = [k for k, v in now.items() if before.get(k) != v]
            if built or reply.error:
                break
            message = FOLLOW_UPS[n] if n < len(FOLLOW_UPS) else FOLLOW_UPS[-1]
    except SignedOut as signed:
        conversation.close()
        return {"signed_out": str(signed), "turns": turns}
    conversation.close()
    minutes = round((time.monotonic() - started) / 60, 1)
    where = "plan.md under Study/"
    if not built:
        built = [k for k, v in plans_in(place.folder, "*/*.md").items() if before_any.get(k) != v]
        where = "other new files under Study/" if built else where
    if not built:
        chat = place.root / "chat-plan"
        chat.mkdir(exist_ok=True)
        (chat / "plan.md").write_text("\n\n".join(t["tutor"] for t in turns), encoding="utf-8")
        built, where = ["chat"], "the tutor's replies in the chat (no plan file was written)"
    scores = {}
    for key in built:
        source = place.root / "chat-plan" if key == "chat" else place.folder / key
        saved = Path(args.out) / f"plan-build-{fixture.name}-built" / key
        if saved.exists():
            shutil.rmtree(saved)
        shutil.copytree(source, saved)
        scores[key] = planscore.score(source, fixture.lists() or {}, judge_for(judge))
    return {"removed_before_build": removed, "plan_found_in": where, "request": request_for(fixture, adapter), "turns": turns, "minutes": minutes,
            "usage": usage, "tokens": sum(usage.values()), "cost_usd": round(sum(t["cost_usd"] for t in turns), 3),
            "built": built, "scores": scores, "session_id": conversation.session_id,
            "transcript": str(adapter.transcript_of(place, conversation.session_id) or "")}


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    if args.planning == "own-run":
        text = ("Check 5, plan build, planning as a run of its own: not implemented. Main builds plans only inside the lesson "
                "(the papers skill). This option is here so a later phase can fill it in.")
        return {"check": "plan-build", "fixture": fixture.name, "planning": "own-run", "implemented": False}, text
    lists = fixture.lists()
    if not lists:
        raise SystemExit(f"{fixture.name} has no lists/ folder, which the plan-build scorer needs")
    judge = Judge(adapter.work, args.model, args.effort) if (args.judge or args.mode == "live") else None
    try:
        return measured(args, adapter, fixture, lists, judge)
    finally:
        if judge:
            judge.close()


def measured(args, adapter, fixture: Fixture, lists: dict, judge: Judge | None) -> tuple[dict, str]:
    lines = []
    result = {"check": "plan-build", "fixture": fixture.name, "adapter": adapter.name, "mode": args.mode, "planning": "lesson"}
    if args.mode == "live":
        built = live(args, adapter, fixture, judge)
        result["live"] = built
        if built.get("signed_out"):
            return result, f"Check 5, plan build, live: Claude Code reported it is signed out, so the check stopped.\n{built['signed_out']}"
        lines.append(f"Check 5, plan build, live on {fixture.name}: Magnesium alone, planning inside the lesson, model {args.model} at {args.effort}.")
        lines.append(f"Request: {built['request']}")
        lines.append(f"Turns: {len(built['turns'])}. Minutes: {built['minutes']}. Tokens: {built['tokens']:,} "
                     f"(input {built['usage'].get('input_tokens', 0):,}, output {built['usage'].get('output_tokens', 0):,}, "
                     f"cache read {built['usage'].get('cache_read_input_tokens', 0):,}, cache write {built['usage'].get('cache_creation_input_tokens', 0):,}). "
                     f"Reported cost ${built['cost_usd']}.")
        if built.get("removed_before_build"):
            lines.append(f"The fixture's own {built['removed_before_build']} was removed from the copy first, so the plan is built from nothing.")
        lines.append(f"Plan scored from: {built['plan_found_in']}.")
        for key, score in built["scores"].items():
            lines += ["", f"Plan built: {key}", table(["Count", "Value", "How"], planscore.counts_text(score))]
    else:
        plans = {"plan": Path(args.plan)} if args.plan else fixture.plans()
        if not plans:
            lines.append(f"{fixture.name} has no plans to score.")
        scores = {}
        for name, folder in plans.items():
            scores[name] = planscore.score(folder, lists, judge_for(judge))
            lines += [f"Plan: {name} ({folder.name})", table(["Count", "Value", "How"], planscore.counts_text(scores[name])), ""]
        result["scores"] = scores
        lines.insert(0, f"Check 5, plan build, scorer: {fixture.name}.\n")
    if judge:
        result["judge"] = judge.cost()
        lines.append(f"\nJudge: {judge.calls} calls on {args.model}, {sum(judge.usage.values()):,} tokens.")
    lines.append("Mechanical counts match words and numbers in the plan files; judged counts come from one model call with a fixed rubric.")
    return result, "\n".join(lines)
