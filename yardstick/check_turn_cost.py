from __future__ import annotations

from pathlib import Path

import transcript
from common import Fixture, load, spread, spread_text, table

HEAD = ["Day", "Learner messages", "Magnesium calls", "Record calls", "Record calls per message",
        "Chars back per call (median)", "Calls with a marked answer", "Chars back on those, as sent", "The same, pretty-printed"]


def row(day: str, d: dict) -> list:
    return [day, d["learner_messages"], d["runtime_calls"], d["record_calls"], d["record_calls_per_message"],
            f"{d['chars_per_call'].get('median', 0):,}", d["marked_calls"], spread_text(d["chars_per_marked_call"]),
            spread_text(d["chars_per_marked_call_pretty"])]


def from_transcripts(args, adapter) -> tuple[dict, str]:
    sources = []
    if args.transcript:
        sources.append({"path": str(Path(args.transcript).expanduser()), "label": Path(args.transcript).stem, "days": args.days or []})
    else:
        fixture = Fixture(args.fixture)
        for entry in fixture.transcripts:
            if args.host and entry.get("host") != args.host:
                continue
            path = Path(entry["path"]).expanduser()
            if not path.is_absolute():
                path = fixture.path / path
            sources.append({"path": str(path), "label": entry.get("label", path.stem), "days": args.days or entry.get("days", [])})
    results, lines = [], []
    for source in sources:
        path = Path(source["path"])
        if not path.is_file():
            lines.append(f"{source['label']}: no file at {path}")
            results.append({"source": source, "missing": True})
            continue
        days = transcript.turn_cost(path, adapter.is_runtime_tool, adapter.is_record_tool, adapter.carries_mark, adapter.tutor_text)
        shown = {d: v for d, v in days.items() if not source["days"] or d in source["days"]}
        results.append({"source": source, "days": days})
        lines += [f"{source['label']} ({path.name})", "", table(HEAD, [row(d, v) for d, v in shown.items()]), ""]
    lines.append("Record calls are learning_step calls. 'As sent' is the length of the text the tutor received; "
                 "'pretty-printed' is the same JSON with two-space indents, the way FINDINGS.md counted 7,000 to 9,000.")
    name = Path(args.transcript).stem if args.transcript else Fixture(args.fixture).name
    return {"check": "turn-cost", "mode": "transcript", "name": name, "adapter": adapter.name, "sources": results}, "Check 3, turn cost from a transcript.\n\n" + "\n".join(lines)


def scripted(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    script_path = fixture.file("lesson_script", "lesson-script.json")
    if not script_path:
        raise SystemExit(f"{fixture.name} has no lesson script")
    script = load(script_path)
    place = adapter.place(fixture.folder, f"turn-cost-{fixture.name}", surface="window")
    turns = adapter.scripted_lesson(place, script)
    calls = [c for t in turns for c in t.calls]
    record = [c for c in calls if c.record]
    marked = [c for c in calls if c.marked]
    messages = len(turns)
    summary = {
        "learner_messages": messages,
        "marked_answers": sum(1 for s in script["turns"] if s.get("attempt")),
        "runtime_calls": len(calls),
        "record_calls": len(record),
        "by_tool": {tool: sum(1 for c in calls if c.tool == tool) for tool in sorted({c.tool for c in calls})},
        "record_calls_per_message": round(len(record) / messages, 2),
        "runtime_calls_per_message": round(len(calls) / messages, 2),
        "chars_per_call": spread([c.chars for c in calls]),
        "chars_per_call_after_start": spread([c.chars for t in turns[1:] for c in t.calls]),
        "chars_per_marked_call": spread([c.chars for c in marked]),
        "chars_per_marked_call_pretty": spread([c.pretty_chars for c in marked]),
        "hook_context_chars": spread([t.hook_chars for t in turns]),
    }
    per_turn = [[n, t.text[:38], len(t.calls), ", ".join(f"{c.tool} {c.chars:,}" for c in t.calls)] for n, t in enumerate(turns)]
    lines = [
        f"Check 3, turn cost, scripted: {fixture.name}. {messages} learner messages, {summary['marked_answers']} marked answers, "
        "driven through the real binary as the window's instructions prescribe (observation and attempt, reply, then a checkpoint when the reply sets a task).",
        "",
        table(["#", "Learner message", "Calls", "Calls and characters back"], per_turn, {3: 120}),
        "",
        table(["Learner messages", "Magnesium calls", "Record calls", "Record calls per message", "Chars back per call (median)",
               "Marked", "Chars back on a marked call", "Pretty-printed"],
              [[messages, len(calls), len(record), summary["record_calls_per_message"], f"{summary['chars_per_call']['median']:,}",
                len(marked), spread_text(summary["chars_per_marked_call"]), spread_text(summary["chars_per_marked_call_pretty"])]]),
        "",
        f"The first message also loads the teaching instructions: {turns[0].calls[0].chars:,} characters back." if turns and turns[0].calls else "",
    ]
    result = {"check": "turn-cost", "mode": "scripted", "fixture": fixture.name, "adapter": adapter.name, "summary": summary,
              "turns": [{"text": t.text, "hook_chars": t.hook_chars, "calls": [c.__dict__ for c in t.calls]} for t in turns]}
    return result, "\n".join(lines)


def run(args, adapter) -> tuple[dict, str]:
    if args.mode == "scripted":
        return scripted(args, adapter)
    return from_transcripts(args, adapter)
