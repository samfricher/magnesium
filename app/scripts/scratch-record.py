#!/usr/bin/env python3

import asyncio
import json
import os
import re
import subprocess
import sys
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP.parent))
from ci.native_mcp import NativeMcpClient, TransportLimits

HOST = ["--plugin-version", "0.2.0", "--adapter-protocol-version", "2", "--host-kind", "claude"]
LONG = "when to use the t distribution rather than the normal, for a small sample whose standard deviation is not known"

NEXT = {
    "pooled variance": ("Two samples of 10 students have variances $s_1^2 = 4$ and $s_2^2 = 9$. What is their pooled variance?",
                        "$s_p^2 = (9 \\cdot 4 + 9 \\cdot 9) / 18 = 6.5$"),
    "margin of error": ("A 95% interval for a mean runs from 12.4 to 15.0. What is its margin of error?", "$(15.0 - 12.4) / 2 = 1.3$"),
    "degrees of freedom of a t statistic": ("A one-sample $t$ test uses 16 observations. How many degrees of freedom does its statistic have?", "15"),
}

DAYS = [
    (3, [("standard error of a mean", "right", True)]),
    (4, [("standard error of a mean", "right", True)]),
    (1, [("standard error of a mean", "right", True), ("pooled variance", "wrong", False)]),
    (1, [("reading a confidence interval", "right", True), ("margin of error", "wrong", False)]),
    (0, [("margin of error", "right", True), ("degrees of freedom of a t statistic", "wrong", False), (LONG, "right", True)]),
]

REPAIR = ["VAR-RATIO", "TCRIT", "CLT-N", "CI-WIDTH", "DF-WELCH", "P-VALUE", "Z-SCORE", "BOXPLOT", "SE-PROP", "CHISQ",
          "ANOVA-1", "F-RATIO", "LEVENE", "BONF", "TUKEY", "R-SQ", "SLOPE-CI", "RESID", "LOGIT", "ODDS", "MLE", "SIMPSON",
          "POWER", "TYPE-II", "EFFECT-D", "WILCOX"]


def write_gaps(course: Path):
    import datetime
    day = (datetime.date.today() - datetime.timedelta(days=12)).isoformat()
    head = "| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |\n" + "|---|" * 11 + "\n"
    rows = "".join(f"| {n} | {day} | Midterm 2023 | Q{n} | {tag} | lost the mark on it | A |  |  |  | open |\n" for n, tag in enumerate(REPAIR, start=1))
    (course / "Study" / "midterm-2" / "gap-log.md").write_text("## Gaps\n\n" + head.replace("||", "|") + rows)


def main(root: Path, new_sitting: bool = False, repair: bool = False):
    course = root / "Intro-Statistics"
    runtime = root / "magnesium"
    days = DAYS
    if repair:
        write_gaps(course)
        days = [(DAYS[0][0], DAYS[0][1] + [(tag, "right", True) for tag in REPAIR]), *DAYS[1:]]
    for day, (gap, answers) in enumerate(days):
        asyncio.run(lesson(course, runtime, f"scratch-day-{day}", answers))
        if gap:
            subprocess.run(["sh", str(APP / "scripts/scratch-course.sh"), str(root), "--days-ago", str(gap)], check=True, stdout=subprocess.DEVNULL)
    if new_sitting:
        import sqlite3
        import time
        conn = sqlite3.connect(course / ".magnesium" / "sittings.sqlite")
        conn.execute("UPDATE meta SET last_learner_ms=?", (int((time.time() - 4 * 3600) * 1000),))
        conn.commit()
        conn.close()


def environment(course: Path, conversation: str) -> dict:
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
            attempt = {"label": skill, "question": f"A question on {skill}.", "first_try": mark, "help": "none", "cold": cold}
            if mark == "wrong" and skill in NEXT:
                attempt["next"] = {"question": NEXT[skill][0], "answer_key": NEXT[skill][1]}
            await call(client, "learning_step", {
                "schema": "learner-model/v2", "action": "update", "request_id": turn,
                "observations": [{"client_ref": "work", "kind": "learner_work", "description": f"Answered on {skill}.",
                                  "events": [{"event_id": event, "role": "learner_response"}],
                                  "attempt": attempt}]})
            hook(course, runtime, conversation, "stop", prompt_id=turn, last_message="Marked.")


async def call(client, tool: str, arguments: dict):
    result = await client.call(tool, arguments)
    if isinstance(result, dict) and "error" in result:
        raise SystemExit(f"scratch-record: {tool} refused: {json.dumps(result['error'])[:600]}")
    return result


if __name__ == "__main__":
    args = [arg for arg in sys.argv[1:] if arg not in ("--new-sitting", "--repair")]
    if len(args) != 1:
        raise SystemExit("usage: python3 scripts/scratch-record.py <dir> [--new-sitting] [--repair]")
    main(Path(args[0]).resolve(), "--new-sitting" in sys.argv[1:], "--repair" in sys.argv[1:])
