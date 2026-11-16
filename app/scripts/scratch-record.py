#!/usr/bin/env python3
"""Give a scratch course (scripts/scratch-course.sh) a record of several
skills, answered over ten days, so a photograph can show where each stands.

    python3 scripts/scratch-record.py <dir>

<dir> is the one given to scratch-course.sh. The answers go in as Claude
Code's would, through the runtime's own hooks and MCP tool, one lesson a
day; between days every recorded time is moved back with scratch-course.sh
--days-ago. Every skill and answer is made up. With today as day 0:

  standard error of a mean       right on its own on days -9, -6 and -2: proven
  pooled variance                wrong on day -2: a quick check due since day -1
  reading a confidence interval  right on its own on day -1: can count today
  margin of error                wrong on day -1, its check passed today: next in 3 days
  degrees of freedom of a t statistic   wrong today: a check due tomorrow
  a long name                    right on its own today: can count tomorrow
"""

import asyncio
import json
import os
import re
import subprocess
import sys
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent))
from ci.native_mcp import NativeMcpClient, TransportLimits  # noqa: E402

HOST = ["--plugin-version", "0.2.0", "--adapter-protocol-version", "2", "--host-kind", "claude"]
LONG = "when to use the t distribution rather than the normal, for a small sample whose standard deviation is not known"

# (days before the next batch, [(skill, mark, cold)]), oldest first.
DAYS = [
    (3, [("standard error of a mean", "right", True)]),
    (4, [("standard error of a mean", "right", True)]),
    (1, [("standard error of a mean", "right", True), ("pooled variance", "wrong", False)]),
    (1, [("reading a confidence interval", "right", True), ("margin of error", "wrong", False)]),
    (0, [("margin of error", "right", True), ("degrees of freedom of a t statistic", "wrong", False), (LONG, "right", True)]),
]


def main(root: Path):
    course = root / "Intro-Statistics"
    runtime = root / "magnesium"
    for day, (gap, answers) in enumerate(DAYS):
        asyncio.run(lesson(course, runtime, f"scratch-day-{day}", answers))
        if gap:
            subprocess.run(["sh", str(APP / "scripts/scratch-course.sh"), str(root), "--days-ago", str(gap)], check=True, stdout=subprocess.DEVNULL)


def environment(course: Path, conversation: str) -> dict:
    """Nothing from the shell: the scratch home, and the conversation as Claude Code names it."""
    home = course.parent / "home"
    return {
        "PATH": "/usr/bin:/bin",
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "HOME": str(home),
        "CLAUDE_CONFIG_DIR": str(home / ".claude"),
        "CLAUDE_CODE_SESSION_ID": conversation,
    }


def hook(course: Path, runtime: Path, conversation: str, event: str, **fields) -> str:
    names = {"session-start": "SessionStart", "user-prompt-submit": "UserPromptSubmit", "stop": "Stop"}
    payload = {"session_id": conversation, "cwd": str(course), "hook_event_name": names[event], **fields}
    if event == "session-start":
        payload["source"] = "startup"
    if event == "stop":
        payload["stop_hook_active"] = False
    out = subprocess.run([str(runtime), "_internal", "hook", event, *HOST], cwd=course, env=environment(course, conversation),
                         input=json.dumps(payload), capture_output=True, text=True, timeout=30, check=True).stdout
    return ((json.loads(out) if out.strip() else {}).get("hookSpecificOutput") or {}).get("additionalContext", "")


async def lesson(course: Path, runtime: Path, conversation: str, answers: list):
    hook(course, runtime, conversation, "session-start")
    opening = "Teach me how sure an estimate is."
    said = hook(course, runtime, conversation, "user-prompt-submit", prompt_id=f"{conversation}-0", prompt=opening)
    ticket = re.search(r"native_capture\.ticket=([0-9a-f]+)", said).group(1)
    client = NativeMcpClient([str(runtime), "_internal", "mcp-server", *HOST], cwd=str(course), env=environment(course, conversation),
                             limits=TransportLimits(request_timeout=30.0))
    async with client:
        await call(client, "context", {"schema": "learner-model/v2", "action": "prepare", "goal": "Estimates", "original_query": opening})
        await call(client, "learning_step", {"schema": "learner-model/v2", "action": "update", "request_id": f"{conversation}-start",
                                             "session": {"operation": "start", "client_ref": "lesson", "goal": "Estimates"},
                                             "native_capture": {"ticket": ticket, "text": opening}})
        hook(course, runtime, conversation, "stop", prompt_id=f"{conversation}-0", last_message="Here is a question.")
        for at, (skill, mark, cold) in enumerate(answers, start=1):
            turn = f"{conversation}-{at}"
            said = hook(course, runtime, conversation, "user-prompt-submit", prompt_id=turn, prompt=f"My answer on {skill}.")
            event = re.search(r"current event (e-[0-9a-f]+)", said).group(1)
            await call(client, "learning_step", {
                "schema": "learner-model/v2", "action": "update", "request_id": turn,
                "observations": [{"client_ref": "work", "kind": "learner_work", "description": f"Answered on {skill}.",
                                  "events": [{"event_id": event, "role": "learner_response"}],
                                  "attempt": {"label": skill, "question": f"A question on {skill}.", "first_try": mark,
                                              "help": "none", "cold": cold}}]})
            hook(course, runtime, conversation, "stop", prompt_id=turn, last_message="Marked.")


async def call(client, tool: str, arguments: dict):
    result = await client.call(tool, arguments)
    if isinstance(result, dict) and "error" in result:
        raise SystemExit(f"scratch-record: {tool} refused: {json.dumps(result['error'])[:600]}")
    return result


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: python3 scripts/scratch-record.py <dir>")
    main(Path(sys.argv[1]).resolve())
