#!/usr/bin/env python3
"""A stand-in for `claude` started as the direct engine's model, for tests.

It speaks Claude Code's stream-json lines, starts no model and reaches no
network. Each reply ends with a record block, cut into small pieces so the
block's marker is split across them, as a real stream can split it.

What a turn does is chosen by words in the learner's message (inside
<learner>):
  TEACH        start: pose "What is b in x² − 5x + 6 = 0?" and set two steps
  b = 5        a wrong answer: mark it and pose a new task
  b = -5       a right answer: mark it, pose the next task, step one done
  HINT         ("Can I have a hint?") give a hint and leave the task open
  NOBLOCK      reply with no record block; the hidden request then gets one
  NEVERBLOCK   reply with no record block, and none when asked again
  SLOW         write slowly, so the reply can be interrupted
  BACK         ("I'm back") pose quick check 1 when the state lists one

The hidden request (it says the block "could not be read") is answered with
the block alone, after STAND_IN_REPAIR_DELAY seconds.
"""

import json
import os
import re
import sys
import threading
import time
import uuid

LOG = os.environ.get("STAND_IN_LOG")
write_lock = threading.Lock()
interrupted = threading.Event()


def log(kind, value):
    if LOG:
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write(json.dumps({"kind": kind, "value": value}) + "\n")


def emit(value):
    with write_lock:
        sys.stdout.write(json.dumps(value) + "\n")
        sys.stdout.flush()


args = sys.argv[1:]
log("start", {"args": args, "cwd": os.getcwd(), "env": {k: os.environ.get(k) for k in (
    "CLAUDE_CODE_DISABLE_CLAUDE_MDS", "CLAUDE_CODE_DISABLE_AUTO_MEMORY", "ENABLE_CLAUDEAI_MCP_SERVERS",
    "MAGNESIUM_SURFACE", "CLAUDE_CODE_SESSION_ID")}})

if args[:2] == ["auth", "status"]:
    print(json.dumps({"loggedIn": True, "authMethod": "claude.ai", "apiProvider": "firstParty", "subscriptionType": "max"}))
    sys.exit(0)

session = None
for arg in args:
    for flag in ("--resume=", "--session-id="):
        if arg.startswith(flag):
            session = arg.split("=", 1)[1]
if session is None or "--system-prompt" not in args:
    sys.stderr.write("stand-in model: a session id and a system prompt are needed\n")
    sys.exit(2)

init_sent = False


def init():
    global init_sent
    if init_sent:
        return
    init_sent = True
    emit({"type": "system", "subtype": "init", "session_id": session, "model": "claude-fable-5-2",
          "apiKeySource": "none",
          "tools": ["Read", "mcp__magnesium__search_sources", "mcp__magnesium__sources"],
          "mcp_servers": [{"name": "magnesium", "status": "connected"}], "plugins": [], "slash_commands": []})


def stream(event):
    emit({"type": "stream_event", "event": event, "parent_tool_use_id": None, "session_id": session})


def say(text, slow=False):
    """Stream text in pieces of five characters, then the finished message."""
    message_id = "msg_" + uuid.uuid4().hex[:12]
    stream({"type": "message_start", "message": {"id": message_id}})
    stream({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    for at in range(0, len(text), 5):
        if interrupted.is_set():
            return False
        stream({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text[at:at + 5]}})
        if slow:
            time.sleep(0.05)
    stream({"type": "content_block_stop", "index": 0})
    emit({"type": "assistant", "message": {"id": message_id, "content": [{"type": "text", "text": text}]},
          "parent_tool_use_id": None, "session_id": session})
    return True


def result(text, subtype="success"):
    emit({"type": "result", "subtype": subtype, "is_error": subtype != "success", "result": text,
          "session_id": session})


def block(**record):
    base = {"answer": None, "help": "none", "skill": "", "kind": None, "no_start": False,
            "note": "", "task": None, "guess": False, "steps": None}
    base.update(record)
    return "\n\n<record>\n" + json.dumps(base, ensure_ascii=False) + "\n</record>"


STEPS = [{"step": "Read a, b, c", "status": "doing"}, {"step": "Use the formula", "status": "todo"}]
DONE = [{"step": "Read a, b, c", "status": "done"}, {"step": "Use the formula", "status": "doing"}]
FIRST = "What is b in x² − 5x + 6 = 0?"
SECOND = "What is b in x² + 3x − 4 = 0?"
THIRD = "Solve x² − 5x + 6 = 0 with the formula."


def reply_for(learner, state):
    if "could not be read" in state:
        delay = float(os.environ.get("STAND_IN_REPAIR_DELAY", "0"))
        time.sleep(delay)
        if "NEVERBLOCK" in last_learner[0]:
            return "I cannot.", False
        return "<record>\n" + json.dumps({"answer": None, "help": "none", "task": None}) + "\n</record>", False
    last_learner[0] = learner
    if "NEVERBLOCK" in learner or "NOBLOCK" in learner:
        return "A reply with no notes.", False
    if "SLOW" in learner:
        return "Let me think about that slowly, one small word after another, for a while." + block(), True
    if "TEACH" in learner:
        return f"Let's start. {FIRST}" + block(note="Lesson start.", task=FIRST, steps=STEPS), False
    if "BACK" in learner and "Quick check" not in learner:
        match = re.search(r'1\. Skill "([^"]+)"', state)
        if match:
            return "Quick check 1: What is b in 2x² − 7x − 4 = 0?" + block(task="What is b in 2x² − 7x − 4 = 0?"), False
        return "Welcome back. " + THIRD + block(task=THIRD), False
    if "hint" in learner.lower():
        return "Look at the sign in front of the 5." + block(help="hint"), False
    if "b = -5" in learner or "b = −5" in learner:
        return f"Right: the sign goes with the number. {THIRD}" + block(
            answer="right", skill="sign of b", note="Kept the sign.", task=THIRD, steps=DONE), False
    if "b = 5" in learner:
        return f"Not quite: b keeps its minus sign, so b = −5. Try this one: {SECOND}" + block(
            answer="wrong", skill="sign of b", kind="reading coefficients", note="Dropped the sign of b.",
            task=SECOND, memory={"skill": "sign of b", "text": "Drops the minus sign of b."}), False
    return "Go on." + block(note="A comment."), False


last_learner = [""]


def turn(content):
    text = content if isinstance(content, str) else "".join(c.get("text", "") for c in content)
    learner = re.search(r"<learner>\n?(.*?)\n?</learner>", text, re.S)
    learner = learner.group(1) if learner else ""
    state = re.search(r"<lesson-state>\n?(.*?)\n?</lesson-state>", text, re.S)
    state = state.group(1) if state else ""
    log("message", {"learner": learner, "state": state})
    init()
    reply, slow = reply_for(learner, state)
    if not say(reply, slow):
        emit({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}})
        result(None, "error_during_execution")
        return
    result(reply)


def main():
    pending = []
    lock = threading.Condition()

    def worker():
        while True:
            with lock:
                while not pending:
                    lock.wait()
                content = pending.pop(0)
            if content is None:
                return
            interrupted.clear()
            turn(content)

    thread = threading.Thread(target=worker)
    thread.start()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        message = json.loads(line)
        if message.get("type") == "control_request":
            if message["request"].get("subtype") == "interrupt":
                interrupted.set()
                with lock:
                    pending.clear()
            emit({"type": "control_response",
                  "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif message.get("type") == "user":
            with lock:
                pending.append(message["message"]["content"])
                lock.notify()
    with lock:
        pending.append(None)
        lock.notify()
    thread.join()


main()
