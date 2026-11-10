#!/usr/bin/env python3
"""A stand-in for the `claude` program, for tests.

It speaks the same lines the window expects from Claude Code: one JSON object
per line each way, with control requests for permission prompts and
interrupts. It starts no model and reaches no network.

It was written from the open-source Agent SDK and the public documentation,
not from a recording. So a test that passes against it shows that the window
is consistent with that reading of the protocol. It does not show that the
real program behaves the same.

What a turn does is chosen by a word in the learner's message:
  PERMISSION  ask to run a command, then say what was decided
  QUESTION    ask the learner to choose, then say what was chosen
  SLOW        write slowly, so the reply can be interrupted
  WAIT        think for a while before the first word
  HELPER      a helper agent writes text that must never be shown
  LIMIT       end the turn at a usage limit
  HISTORY     say how many turns this conversation has had
"""

import json
import os
import sys
import threading
import time
import uuid

LOG = os.environ.get("STAND_IN_LOG")
STATE = os.environ.get("STAND_IN_STATE")
NO_PLUGIN = os.environ.get("STAND_IN_NO_PLUGIN") == "1"

write_lock = threading.Lock()
replies = {}
reply_ready = threading.Condition()
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
log("start", {"args": args, "cwd": os.getcwd(), "path": os.environ.get("PATH", "")})

if "--bare" in args:
    sys.stderr.write("stand-in: --bare would skip the plugin\n")
    sys.exit(2)
for needed in ("-p", "--input-format", "--output-format", "--verbose", "--permission-prompt-tool"):
    if needed not in args:
        sys.stderr.write(f"error: missing required argument {needed}\n")
        sys.exit(2)

session = None
for arg in args:
    if arg.startswith("--resume="):
        session = arg.split("=", 1)[1]
resumed = session is not None
if session is None:
    session = str(uuid.uuid4())
elif STATE and not os.path.exists(os.path.join(STATE, session + ".json")):
    # The real program refuses an id it has no transcript for.
    sys.stderr.write(f"No conversation found with session ID: {session}\n")
    sys.exit(1)


def history_path():
    return os.path.join(STATE, session + ".json") if STATE else None


def load_history():
    path = history_path()
    if path and os.path.exists(path):
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    return []


def save_history(history):
    path = history_path()
    if path:
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(history, handle)


def init_message():
    message = {
        "type": "system",
        "subtype": "init",
        "session_id": session,
        "model": "stand-in",
        "cwd": os.getcwd(),
        "tools": ["Read", "Bash", "AskUserQuestion"],
        "mcp_servers": [],
        "plugins": [],
        "slash_commands": ["help"],
    }
    if not NO_PLUGIN:
        message["tools"].append("mcp__plugin_magnesium_magnesium__learning_step")
        message["mcp_servers"] = [{"name": "plugin:magnesium:magnesium", "status": "connected"}]
        message["plugins"] = [{"name": "magnesium", "path": "/stand-in/magnesium"}]
        message["slash_commands"] += ["magnesium:learn", "magnesium:papers"]
    return message


def stream(event, parent=None):
    emit({
        "type": "stream_event",
        "uuid": str(uuid.uuid4()),
        "session_id": session,
        "parent_tool_use_id": parent,
        "event": event,
    })


def assistant(blocks, message_id, parent=None):
    emit({
        "type": "assistant",
        "uuid": str(uuid.uuid4()),
        "session_id": session,
        "parent_tool_use_id": parent,
        "message": {"id": message_id, "role": "assistant", "model": "stand-in", "content": blocks},
    })


def say(text, slow=False):
    """Write one text block as deltas, then as a whole message."""
    message_id = "msg_" + uuid.uuid4().hex[:12]
    stream({"type": "message_start", "message": {"id": message_id}})
    stream({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    words = text.split(" ")
    for index, word in enumerate(words):
        if interrupted.is_set():
            return False
        piece = word if index == 0 else " " + word
        stream({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}})
        if slow:
            time.sleep(0.15)
    stream({"type": "content_block_stop", "index": 0})
    assistant([{"type": "text", "text": text}], message_id)
    return True


def use_tool(name, tool_input):
    message_id = "msg_" + uuid.uuid4().hex[:12]
    tool_id = "toolu_" + uuid.uuid4().hex[:12]
    stream({"type": "message_start", "message": {"id": message_id}})
    stream({"type": "content_block_start", "index": 0,
            "content_block": {"type": "tool_use", "id": tool_id, "name": name, "input": {}}})
    stream({"type": "content_block_stop", "index": 0})
    assistant([{"type": "tool_use", "id": tool_id, "name": name, "input": tool_input}], message_id)
    return tool_id


def ask_permission(name, tool_input, suggestions):
    tool_id = use_tool(name, tool_input)
    request_id = "perm_" + uuid.uuid4().hex[:8]
    emit({
        "type": "control_request",
        "request_id": request_id,
        "request": {
            "subtype": "can_use_tool",
            "tool_name": name,
            "input": tool_input,
            "permission_suggestions": suggestions,
            "blocked_path": None,
            "tool_use_id": tool_id,
        },
    })
    with reply_ready:
        while request_id not in replies and not interrupted.is_set():
            reply_ready.wait(0.05)
        return replies.pop(request_id, None)


def result(text, error=None):
    emit({
        "type": "result",
        "subtype": error or "success",
        "is_error": error is not None,
        "duration_ms": 10,
        "duration_api_ms": 5,
        "num_turns": 1,
        "session_id": session,
        "result": text,
        "total_cost_usd": 0,
    })


def stopped():
    """What the real program sent for a turn interrupted before its reply
    ended, in the live trace of 2026-11-10: a user line, then a result with
    no text and only an internal diagnostic as its error."""
    emit({"type": "user", "parent_tool_use_id": None, "session_id": session,
          "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}})
    emit({
        "type": "result",
        "subtype": "error_during_execution",
        "is_error": True,
        "duration_ms": 10,
        "duration_api_ms": 5,
        "num_turns": 1,
        "session_id": session,
        "errors": ["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"],
        "terminal_reason": "aborted_streaming",
        "total_cost_usd": 0,
    })


def turn(text):
    emit(init_message())
    history = load_history()
    history.append(text)
    save_history(history)

    if "LIMIT" in text:
        emit({"type": "rate_limit_event", "uuid": str(uuid.uuid4()), "session_id": session,
              "rate_limit_info": {"status": "rejected", "resetsAt": 1794300000, "rateLimitType": "five_hour"}})
        result("You've hit your limit · resets 3pm", error="error_during_execution")
        return

    if "WAIT" in text:
        # A long silence before the first word, as when a plan is being built.
        for _ in range(100):
            if interrupted.is_set():
                stopped()
                return
            time.sleep(0.05)

    if "HELPER" in text:
        stream({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "SECRET HELPER TEXT"}}, parent="toolu_helper")
        assistant([{"type": "text", "text": "SECRET HELPER TEXT"}], "msg_helper", parent="toolu_helper")

    if "PERMISSION" in text:
        reply = ask_permission(
            "Bash",
            {"command": "tectonic paper.tex"},
            [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "tectonic:*"}],
              "behavior": "allow", "destination": "localSettings"}],
        )
        if reply is None:
            stopped()
            return
        if reply.get("behavior") == "allow":
            kept = " and it is remembered" if reply.get("updatedPermissions") else ""
            answer = "The command ran" + kept + "."
        else:
            answer = "The command was refused: " + reply.get("message", "")
    elif "QUESTION" in text:
        reply = ask_permission(
            "AskUserQuestion",
            {"questions": [{
                "question": "Which did you have in mind?",
                "header": "Your idea",
                "multiSelect": False,
                "options": [
                    {"label": "the intercept is Vmax", "description": "as read"},
                    {"label": "the intercept is 1/Vmax", "description": "its reciprocal"},
                ],
            }]},
            None,
        )
        if reply is None:
            stopped()
            return
        answers = reply.get("updatedInput", {}).get("answers", {})
        answer = "You chose: " + json.dumps(answers, sort_keys=True)
    elif "HISTORY" in text:
        answer = f"This is turn {len(history)} of {session}."
    elif "SLOW" in text:
        answer = " ".join(f"word{n}" for n in range(60))
    else:
        answer = "You said: " + text

    finished = say(answer, slow="SLOW" in text)
    if not finished:
        stopped()
        return
    result(answer)


def main():
    worker = None
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        message = json.loads(line)
        log("received", message)
        kind = message.get("type")
        if kind == "control_request":
            subtype = message["request"].get("subtype")
            response = {}
            if subtype == "initialize":
                commands = [{"name": "help"}]
                if not NO_PLUGIN:
                    commands += [{"name": "magnesium:learn"}, {"name": "magnesium:papers"}]
                response = {"commands": commands}
            elif subtype == "interrupt":
                interrupted.set()
                with reply_ready:
                    reply_ready.notify_all()
            emit({"type": "control_response",
                  "response": {"subtype": "success", "request_id": message["request_id"], "response": response}})
        elif kind == "control_response":
            inner = message["response"]
            with reply_ready:
                replies[inner["request_id"]] = inner.get("response", {})
                reply_ready.notify_all()
        elif kind == "user":
            content = message["message"]["content"]
            if worker is not None:
                worker.join()
            # Cleared here, not in the turn, so an interrupt that arrives
            # before the turn's thread starts is not lost.
            interrupted.clear()
            worker = threading.Thread(target=turn, args=(content,), daemon=True)
            worker.start()
    if worker is not None:
        worker.join(timeout=5)
    log("end", {"resumed": resumed})


if __name__ == "__main__":
    main()
