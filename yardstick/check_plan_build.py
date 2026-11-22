from __future__ import annotations

import json
import shutil
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import planscore
from claude_code import SignedOut, add_usage
from common import Fixture, load, moment, now_local, table
from judge import Judge

FOLLOW_UPS = [
    "Use your defaults for anything I have not answered, and write the plan now.",
    "Please write the plan file now. I will answer anything else later.",
    "Go ahead and finish the plan with what you have.",
]


MARKING_RUNS = 3


def judge_for(judge: Judge | None):
    if judge is None:
        return None

    def run(folder, lines, sections, uncovered, cut_lines):
        prompt = planscore.judge_prompt(folder, uncovered)
        answer = judge.ask(prompt, planscore.SCHEMA)
        for _ in range(2):
            if len(answer.get("carried") or []) == len(uncovered):
                break
            answer = judge.ask(prompt + f"\n\nGive exactly {len(uncovered)} entries under carried, one for each idea listed above, "
                               "in that order.", planscore.SCHEMA)
        judged = planscore.read_judged(answer)
        judged["attempts_matched"] = len(answer.get("carried") or []) == len(uncovered)
        return judged

    return run


def marker_for(judge: Judge | None, lists: dict, runs: int = MARKING_RUNS):
    if judge is None:
        return None
    section_ids = [s["id"] for s in (lists.get("order") or {}).get("sections", [])]
    listed_ids = [n for e in (lists.get("exercises") or {}).get("exercises", []) if e.get("in_scope", True) for n in planscore.names(e)]

    def one(folder, units):
        prompt = planscore.block_prompt(folder, lists, units)
        marks = planscore.read_marks(judge.ask(prompt, planscore.BLOCK_SCHEMA), units, section_ids, listed_ids)
        if any(m["missing"] for m in marks):
            again = planscore.read_marks(judge.ask(prompt + f"\n\nGive exactly {len(units)} entries, one per block above, "
                                                   "with `block` copied exactly.", planscore.BLOCK_SCHEMA), units, section_ids, listed_ids)
            if sum(m["missing"] for m in again) < sum(m["missing"] for m in marks):
                marks = again
        return marks

    def run(folder, units):
        if not units:
            return []
        with ThreadPoolExecutor(max_workers=runs) as pool:
            return list(pool.map(lambda _: one(folder, units), range(runs)))

    return run


def key_for(fixture: Fixture, name: str) -> list[dict] | None:
    folder = fixture.data.get("plan_key")
    if not folder:
        return None
    path = fixture.path / folder / f"{name}.json"
    if not path.is_file():
        return None
    lists = fixture.lists() or {}
    section_ids = [s["id"] for s in (lists.get("order") or {}).get("sections", [])]
    listed_ids = [n for e in (lists.get("exercises") or {}).get("exercises", []) if e.get("in_scope", True) for n in planscore.names(e)]
    return [planscore.clean(m, section_ids, listed_ids) for m in load(path)["blocks"]]


def scored(folder: Path, fixture: Fixture, name: str, lists: dict, judge: Judge | None) -> dict:
    result = planscore.score(folder, lists, judge_for(judge), marker_for(judge, lists))
    key = key_for(fixture, name)
    if key is not None:
        sections = (lists.get("order") or {}).get("sections", [])
        listed_ids = [n for e in (lists.get("exercises") or {}).get("exercises", []) if e.get("in_scope", True) for n in planscore.names(e)]
        result["key_shape"] = planscore.shape(key, sections, listed_ids)
        if result.get("blocks"):
            result["agreement"] = planscore.agree(key, result["blocks"])
            result["agreement_runs"] = [planscore.agree(key, run) for run in result["block_runs"]]
    return result


def shape_lines(result: dict) -> list[str]:
    if not result.get("shape") and not result.get("key_shape"):
        return []
    lines = []
    if result.get("shape"):
        rows = planscore.shape_text(result)
        if result.get("key_shape"):
            for row, item in zip(rows, result["key_shape"]):
                row.insert(2, f"{item['value']} of {item['of']}" if item["of"] is not None else ("yes" if item["value"] else "no") if isinstance(item["value"], bool) else str(item["value"]))
            lines.append(table(["Shape, block by block", "Judged (majority of 3)", "Hand-marked key", "Spread over the 3 runs"], rows))
        else:
            lines.append(table(["Shape, block by block", "Judged (majority of 3)", "Spread over the 3 runs"], rows))
        spread = result["block_spread"]
        same = spread["identical_across_runs"]
        lines.append(f"Blocks marked: {spread['blocks']}. Marked the same in all {spread['runs']} runs: "
                     + ", ".join(f"{f} {v}" for f, v in same.items()) + ".")
    else:
        lines.append(table(["Shape, block by block", "Hand-marked key"], [[i["label"], f"{i['value']} of {i['of']}" if i["of"] is not None else str(i["value"])] for i in result["key_shape"]]))
    if result.get("agreement"):
        a = result["agreement"]
        lines.append("Agreement with the key, majority of 3: " + ", ".join(f"{f} {a[f][0]} of {a[f][1]}" for f in ("done", "kind", "sections", "listed", "unseen")) + ".")
        for d in a["differ"]:
            lines.append(f"  block {d['block']}: " + "; ".join(f"{f} key {v[0]} judged {v[1]}" for f, v in d.items() if f != "block"))
    return lines


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


def live(args, adapter, fixture: Fixture, judge: Judge | None) -> list[dict]:
    count = max(1, args.builds)
    with ThreadPoolExecutor(max_workers=min(count, args.parallel)) as pool:
        return list(pool.map(lambda n: live_one(args, adapter, fixture, judge, n, count), range(1, count + 1)))


def live_one(args, adapter, fixture: Fixture, judge: Judge | None, number: int, count: int) -> dict:
    suffix = f"-{number}" if count > 1 else ""
    place = adapter.place(fixture.folder, f"plan-build-{fixture.name}{suffix}", live=True)
    try:
        return build(args, adapter, fixture, judge, place, suffix)
    finally:
        adapter.release(place)


def searches(log: Path) -> list[dict]:
    asked, out = {}, []
    if not log.is_file():
        return out
    for line in log.read_text(encoding="utf-8").splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        for block in (value.get("message") or {}).get("content") or []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use" and str(block.get("name", "")).endswith("search_sources"):
                asked[block.get("id")] = (block.get("input") or {}).get("query")
            elif block.get("type") == "tool_result" and block.get("tool_use_id") in asked:
                content = block.get("content")
                body = content if isinstance(content, str) else "".join(c.get("text", "") for c in content or [] if isinstance(c, dict))
                out.append({"query": asked.pop(block.get("tool_use_id")), "chars": len(body), "start": body[:400]})
    return out


def planning_trace(log: Path) -> dict:
    trace = {"subagent_runs": [], "stage_planning_in_lesson": 0, "stage_planning_in_subagent": 0, "course_map_calls": 0,
             "subagent_output_tokens": 0, "lesson_output_tokens": 0}
    if not log.is_file():
        return trace
    for line in log.read_text(encoding="utf-8").splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        inside = bool(value.get("parent_tool_use_id"))
        message = value.get("message") or {}
        usage = message.get("usage") or {}
        if value.get("type") == "assistant" and usage:
            key = "subagent_output_tokens" if inside else "lesson_output_tokens"
            trace[key] += usage.get("output_tokens") or 0
        for block in message.get("content") or []:
            if not isinstance(block, dict) or block.get("type") != "tool_use":
                continue
            name, data = str(block.get("name", "")), block.get("input") or {}
            if name in ("Task", "Agent"):
                trace["subagent_runs"].append({"subagent_type": data.get("subagent_type"), "brief": str(data.get("prompt", ""))[:1500]})
            if name.endswith("__context") and data.get("stage") == "planning":
                trace["stage_planning_in_subagent" if inside else "stage_planning_in_lesson"] += 1
            if name.endswith("__course_map"):
                trace["course_map_calls"] += 1
    if trace["subagent_runs"]:
        trace["where"] = "a run of its own (subagent " + ", ".join(sorted({str(r["subagent_type"]) for r in trace["subagent_runs"]})) + ")"
    elif trace["stage_planning_in_lesson"]:
        trace["where"] = "inside the lesson (stage planning loaded in the conversation)"
    else:
        trace["where"] = "inside the lesson, without the planning stage"
    return trace


def build(args, adapter, fixture: Fixture, judge: Judge | None, place, suffix: str = "") -> dict:
    removed = None
    if fixture.target and (place.folder / fixture.target).exists() and not args.keep_plan:
        shutil.rmtree(place.folder / fixture.target)
        removed = fixture.target
    settled = adapter.settle_index(place) if args.settle_index else None
    before = plans_in(place.folder)
    before_any = plans_in(place.folder, "*/*.md")
    log = place.root / "stream.jsonl"
    conversation = adapter.live(place, args.model, args.effort, log, args.planning)
    usage: dict = {}
    turns = []
    started = time.monotonic()
    message = request_for(fixture, adapter)
    built: list[str] = []
    has_map = adapter.cli_json(place, "map", "--json") is not None
    try:
        for n in range(max(1 + len(FOLLOW_UPS), args.max_turns)):
            reply = conversation.say(message, timeout=args.turn_timeout)
            add_usage(usage, reply.usage)
            waited = conversation.wait_background(timeout=args.turn_timeout)
            if waited is not None:
                add_usage(usage, waited.usage)
                reply.tools += waited.tools
                reply.text += waited.text
                reply.cost_usd += waited.cost_usd
                reply.seconds += waited.seconds
                reply.error = reply.error or waited.error
            said = "\n".join(t for t in (adapter.tutor_text(x["name"], x["input"]) for x in reply.tools) if t) or reply.text
            turns.append({"learner": message, "tutor": said, "tools": [x["name"] for x in reply.tools], "seconds": round(reply.seconds, 1),
                          "usage": reply.usage, "cost_usd": reply.cost_usd, "error": reply.error})
            now = plans_in(place.folder)
            built = [k for k, v in now.items() if before.get(k) != v]
            view = adapter.map_view(place, fixture.target) if has_map else None
            scheduled = bool(view and view.get("schedule"))
            if (built and (scheduled or not has_map)) or reply.error:
                break
            message = FOLLOW_UPS[n] if n < len(FOLLOW_UPS) else "Go on."
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
    trace = planning_trace(log)
    view = adapter.map_view(place, fixture.target) if hasattr(adapter, "map_view") else None
    map_saved = None
    if view is not None:
        map_saved = Path(args.out) / f"plan-build-{fixture.name}-built{suffix}-map.json"
        map_saved.parent.mkdir(parents=True, exist_ok=True)
        map_saved.write_text(json.dumps(view, indent=1, ensure_ascii=False), encoding="utf-8")
        study = place.folder / ".magnesium" / "map.sqlite"
        if study.is_file():
            shutil.copy2(study, Path(args.out) / f"plan-build-{fixture.name}-built{suffix}-map.sqlite")
    scores = {}
    for key in built:
        source = place.root / "chat-plan" if key == "chat" else place.folder / key
        saved = Path(args.out) / f"plan-build-{fixture.name}-built{suffix}" / key
        if saved.exists():
            shutil.rmtree(saved)
        shutil.copytree(source, saved)
        if fixture.lists():
            scores[key] = scored(source, fixture, f"built{suffix}", fixture.lists(), judge)
    return {"removed_before_build": removed, "index_settled_first": settled, "plan_found_in": where, "request": request_for(fixture, adapter),
            "planning": trace, "map": {"saved": str(map_saved) if map_saved else None,
                                       "coverage": (view or {}).get("coverage"), "blocks": len((view or {}).get("schedule") or []),
                                       "items": len((view or {}).get("items") or []),
                                       "links": sum(len(i.get("uses") or []) for i in (view or {}).get("items") or [])},
            "place": str(place.root),
            "turns": turns, "minutes": minutes, "usage": usage, "tokens": sum(usage.values()), "cost_usd": round(sum(t["cost_usd"] for t in turns), 3),
            "built": built, "scores": scores, "session_id": conversation.session_id, "searches": searches(log),
            "transcript": str(adapter.transcript_of(place, conversation.session_id) or "")}


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    lists = fixture.lists() or {}
    if not lists and args.mode != "live":
        raise SystemExit(f"{fixture.name} has no lists/ folder, which the plan-build scorer needs")
    judge = Judge(adapter.work, args.model, args.effort) if (args.judge or (args.mode == "live" and lists)) else None
    try:
        return measured(args, adapter, fixture, lists, judge)
    finally:
        if judge:
            judge.close()


def measured(args, adapter, fixture: Fixture, lists: dict, judge: Judge | None) -> tuple[dict, str]:
    lines = []
    result = {"check": "plan-build", "fixture": fixture.name, "adapter": adapter.name, "mode": args.mode, "planning": args.planning}
    if args.mode == "live":
        builds = live(args, adapter, fixture, judge)
        result["live"] = builds[0] if len(builds) == 1 else builds
        signed = next((b for b in builds if b.get("signed_out")), None)
        if signed:
            return result, f"Check 5, plan build, live: Claude Code reported it is signed out, so the check stopped.\n{signed['signed_out']}"
        lines.append(f"Check 5, plan build, live on {fixture.name}: Magnesium alone, planning {'as a run of its own' if args.planning == 'own-run' else 'inside the lesson'}, model {args.model} at {args.effort}, "
                     f"{len(builds)} build{'s' if len(builds) > 1 else ''}.")
        lines.append(f"Request: {builds[0]['request']}")
        if builds[0].get("removed_before_build"):
            lines.append(f"The fixture's own {builds[0]['removed_before_build']} was removed from each copy first, so the plan is built from nothing.")
        if args.settle_index:
            lines.append("Before each build the source index was let finish: " + "; ".join(
                f"{b['index_settled_first'].get('seconds')} s, {b['index_settled_first'].get('state')}" for b in builds) + ".")
        for n, built in enumerate(builds, 1):
            lines += ["", f"Build {n}. Turns: {len(built['turns'])}. Minutes: {built['minutes']}. Tokens: {built['tokens']:,}. Reported cost ${built['cost_usd']}. "
                      f"Plan scored from: {built['plan_found_in']}."]
            trace = built.get("planning") or {}
            lines.append(f"  Planning ran {trace.get('where', 'unknown')}. course_map calls: {trace.get('course_map_calls', 0)}. "
                         f"Output tokens: lesson {trace.get('lesson_output_tokens', 0):,}, subagent {trace.get('subagent_output_tokens', 0):,}.")
            mapped = built.get("map") or {}
            if mapped.get("items"):
                cov = mapped.get("coverage") or {}
                lines.append(f"  Map: {mapped['items']} items in scope, {mapped['links']} use links, {mapped['blocks']} blocks; "
                             f"asked by no planned task {cov.get('asked_by_no_planned_task')}, cut {cov.get('cut')}, cuts not agreed {cov.get('unconfirmed_cuts')}.")
            for search in built.get("searches", [])[:3]:
                lines.append(f"  search {search['query']!r}: {search['chars']} characters back, starting {search['start'][:160]!r}")
            for key, score in built["scores"].items():
                lines += [f"Plan built: {key}", table(["Count", "Value", "How"], planscore.counts_text(score))] + shape_lines(score)
        if len(builds) > 1:
            lines += ["", across(builds)]
    else:
        plans = {"plan": Path(args.plan)} if args.plan else fixture.plans()
        if not plans:
            lines.append(f"{fixture.name} has no plans to score.")
        scores = {}
        for name, folder in plans.items():
            scores[name] = scored(folder, fixture, name, lists, judge)
            lines += [f"Plan: {name} ({folder.name})", table(["Count", "Value", "How"], planscore.counts_text(scores[name]))] + shape_lines(scores[name]) + [""]
        result["scores"] = scores
        lines.insert(0, f"Check 5, plan build, scorer: {fixture.name}.\n")
    if judge:
        result["judge"] = judge.cost()
        lines.append(f"\nJudge: {judge.calls} calls on {args.model}, {sum(judge.usage.values()):,} tokens.")
    lines.append("Mechanical counts match words and numbers in the plan files. Judged counts come from one model call with a fixed rubric. "
                 f"The shape comes from marking each block of the schedule with the codebook in plan-blocks.md, {MARKING_RUNS} times, "
                 "and taking the majority.")
    return result, "\n".join(lines)


def across(builds: list[dict]) -> str:
    shapes = [next(iter(b["scores"].values())).get("shape") or [] for b in builds]
    if not shapes or not shapes[0]:
        return ""
    rows = []
    for n, item in enumerate(shapes[0]):
        values = []
        for shape in shapes:
            v = shape[n]["value"]
            values.append(f"{v} of {shape[n]['of']}" if shape[n]["of"] is not None else ("yes" if v is True else "no" if v is False else str(v)))
        rows.append([item["label"], *values])
    return table(["Shape, block by block", *[f"Build {n}" for n in range(1, len(builds) + 1)]], rows)
