from __future__ import annotations

import json
import re
from collections import defaultdict
from pathlib import Path

from common import local_day, mentions, spread

PREFIX = "mcp__plugin_magnesium_magnesium__"
RECORD = PREFIX + "learning_step"
REPLY = PREFIX + "reply"

ASKS_EXPLANATION = re.compile(
    r"\b(explain|explanation|why does|why is|why do|how does|how do you get|walk me through|show me how|"
    r"i don'?t (get|understand)|still (don'?t|not) (get|understand)|what does .{1,40} mean|disregard the \d+ word)", re.I)
STALL = re.compile(r"\b(i don'?t know|no idea|i'?m stuck|stuck on|not sure how|can'?t start|no clue|idk)\b", re.I)
DETOUR = re.compile(r"\b(step back|go back to|first,? recall|recall that|let'?s revisit|before we (can|go)|earlier step|refresher)\b", re.I)
TASK_VERB = re.compile(
    r"^\s*(\*\*)?(find|compute|calculate|show|write|try|solve|work out|give|state|say|what|which|how|is|are|does|can|use|explain|predict|name|check|sketch|derive)\b",
    re.I | re.M)


def default_runtime(tool: str) -> bool:
    return tool.startswith(PREFIX)


def default_record(tool: str) -> bool:
    return tool == RECORD


def default_marked(tool: str, tool_input: dict) -> bool:
    return tool == RECORD and any(o.get("attempt") or o.get("attempts") for o in tool_input.get("observations") or [])


def default_tutor(tool: str, tool_input: dict) -> str | None:
    return tool_input.get("text") if tool == REPLY else None


def result_text(content) -> str:
    if isinstance(content, str):
        return content
    return "".join(part.get("text", "") for part in content or [] if isinstance(part, dict))


def pretty_length(text: str) -> int:
    try:
        return len(json.dumps(json.loads(text), indent=2, ensure_ascii=False))
    except (json.JSONDecodeError, TypeError):
        return len(text)


def is_learner(record: dict) -> bool:
    if record.get("type") != "user" or record.get("isMeta") or record.get("isCompactSummary"):
        return False
    content = (record.get("message") or {}).get("content")
    if isinstance(content, str):
        return bool(content.strip())
    if isinstance(content, list):
        if any(isinstance(b, dict) and b.get("type") == "tool_result" for b in content):
            return False
        return any(isinstance(b, dict) and b.get("type") in ("text", "image") for b in content)
    return False


def learner_text(record: dict) -> str:
    content = (record.get("message") or {}).get("content")
    if isinstance(content, str):
        return content
    return " ".join(b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text")


def read(path: Path):
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue


def events(path: Path, runtime=default_runtime, tutor=default_tutor):
    calls: dict[str, dict] = {}
    order: list = []
    for record in read(path):
        kind = record.get("type")
        stamp = record.get("timestamp")
        if is_learner(record) and stamp:
            order.append(("learner", local_day(stamp), stamp, learner_text(record)))
        elif kind == "assistant" and stamp:
            for block in (record.get("message") or {}).get("content") or []:
                if not isinstance(block, dict):
                    continue
                if block.get("type") == "tool_use" and block.get("id") not in calls:
                    call = {"id": block["id"], "tool": block.get("name", ""), "input": block.get("input") or {},
                            "day": local_day(stamp), "at": stamp, "chars": None, "pretty": None}
                    calls[block["id"]] = call
                    order.append(("call", call["day"], stamp, call))
                    said = tutor(call["tool"], call["input"])
                    if said:
                        order.append(("tutor", call["day"], stamp, said))
                elif block.get("type") == "text" and block.get("text", "").strip():
                    order.append(("text", local_day(stamp), stamp, block["text"]))
        elif kind == "user":
            content = (record.get("message") or {}).get("content")
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict) and block.get("type") == "tool_result" and block.get("tool_use_id") in calls:
                        call = calls[block["tool_use_id"]]
                        if call["chars"] is None:
                            text = result_text(block.get("content"))
                            call["chars"] = len(text)
                            call["pretty"] = pretty_length(text)
                            call["error"] = bool(block.get("is_error"))
    return order


def turn_cost(path: Path, runtime=default_runtime, record=default_record, marked=default_marked, tutor=default_tutor) -> dict:
    days: dict[str, dict] = defaultdict(lambda: {"learner_messages": 0, "runtime_calls": 0, "record_calls": 0,
                                                 "by_tool": defaultdict(int), "chars": [], "marked_chars": [], "marked_pretty": []})
    for kind, day, _, value in events(path, runtime, tutor):
        row = days[day]
        if kind == "learner":
            row["learner_messages"] += 1
        elif kind == "call" and runtime(value["tool"]):
            row["runtime_calls"] += 1
            row["by_tool"][value["tool"].removeprefix(PREFIX)] += 1
            if record(value["tool"]):
                row["record_calls"] += 1
            if value["chars"] is not None:
                row["chars"].append(value["chars"])
                if marked(value["tool"], value["input"]) and not value.get("error"):
                    row["marked_chars"].append(value["chars"])
                    row["marked_pretty"].append(value["pretty"])
    out = {}
    for day in sorted(days):
        row = days[day]
        messages = row["learner_messages"]
        out[day] = {
            "learner_messages": messages,
            "runtime_calls": row["runtime_calls"],
            "record_calls": row["record_calls"],
            "by_tool": dict(row["by_tool"]),
            "record_calls_per_message": round(row["record_calls"] / messages, 2) if messages else None,
            "runtime_calls_per_message": round(row["runtime_calls"] / messages, 2) if messages else None,
            "chars_per_call": spread(row["chars"]),
            "marked_calls": len(row["marked_chars"]),
            "chars_per_marked_call": spread(row["marked_chars"]),
            "chars_per_marked_call_pretty": spread(row["marked_pretty"]),
        }
    return out


def sets_task(reply: str) -> bool:
    tail = reply.strip()[-400:]
    if "?" in tail:
        return True
    last = [line for line in tail.splitlines() if line.strip()][-2:]
    return any(TASK_VERB.search(line) for line in last)


def section_of(text: str, sections: list[dict]) -> int | None:
    found = None
    for n, section in enumerate(sections):
        if any(mentions(text, term) for term in section.get("terms", [])):
            found = n
    return found


def real_use_counts(path: Path, sections: list[dict] | None = None, tutor=default_tutor) -> dict:
    sections = sections or []
    names = [s.get("label") or s.get("id") for s in sections]
    explain = defaultdict(int)
    stalls = defaultdict(int)
    detours = defaultdict(int)
    replies = 0
    no_task = 0
    current: int | None = None
    pending_text: list[str] = []
    stream = events(path, tutor=tutor)
    has_reply_tool = any(kind == "tutor" for kind, *_ in stream)

    def close_turn():
        nonlocal replies, no_task, current
        if not pending_text:
            return
        reply = "\n".join(pending_text)
        pending_text.clear()
        replies += 1
        if not sets_task(reply):
            no_task += 1
        here = section_of(reply, sections)
        if here is not None and current is not None and here < current and DETOUR.search(reply):
            detours[names[current]] += 1
        if here is not None:
            current = max(current or 0, here) if current is not None else here

    for kind, _, _, value in stream:
        if kind == "learner":
            close_turn()
            where = names[current] if current is not None and names else "unplaced"
            if ASKS_EXPLANATION.search(value):
                explain[where] += 1
            if STALL.search(value):
                stalls[where] += 1
        elif kind == "tutor" or (kind == "text" and not has_reply_tool):
            pending_text.append(value)
    close_turn()
    return {
        "method": "mechanical (word match on the transcript; sections by their terms)",
        "explanation_requests": dict(explain),
        "stalls": dict(stalls),
        "teaching_replies": replies,
        "teaching_replies_setting_no_task": no_task,
        "detours_to_earlier_step": dict(detours),
        "planned_items_against_parts_marked": "no data",
    }


DUE = re.compile(r"Sitting quick checks: due (\d+), posed (\d+)")


def stated_checks(path: Path) -> dict:
    hook_due = None
    opener = False
    for record in read(path):
        if record.get("type") == "attachment":
            attachment = record.get("attachment") or {}
            if attachment.get("type") == "hook_additional_context":
                for text in attachment.get("content") or []:
                    found = DUE.search(str(text))
                    if found and hook_due is None:
                        hook_due = int(found.group(1))
        elif record.get("type") == "user":
            content = (record.get("message") or {}).get("content")
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict) and block.get("type") == "tool_result" and '"sitting_opener"' in result_text(block.get("content")):
                        opener = True
    return {"hook_due": hook_due, "opener_delivered": opener}
