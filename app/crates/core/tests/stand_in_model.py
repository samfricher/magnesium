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
  BACK         (or the window's "I'm back") pose quick check 1 when the
               state lists one or says it is still due, or else the first
               paper redo it lists
  I'm lost     ask a question that finds the cause, and leave the task open;
               on a due quick check or paper redo, warn first (see HINT)
  HINT         on a due quick check or paper redo, the first request gets a
               one-line warning with help none, and the next the hint
  Km = 2       a right answer to a paper redo: mark it cold, give its next
               question, and pose the next paper redo the state listed
  Km = 0.5     a wrong answer to a paper redo
  OVERTIME     with "Km = 2": mark the answer slow
  TAUGHT       with "Km = 2": mark the answer not cold
  REWORD       pose the open paper redo again in other words
  MENTION      speak of paper redo 2 and pose an ordinary task
  COLD         with "b = -5": mark the answer cold
  REPAIRED     finish the first repair the state listed (or REPAIR <n>:
               repair n), with its next question
  BADREPAIR    name a repair with no number
  CONTROLREPAIR  ask for a pause and finish the first listed repair in one block
  CHECK AGAIN  pose quick check 1 again
  RIGHTCHECK   a right answer, cold, under the skill "reading coefficients"
  RECORDING OFF / RECORDING ON / PAUSE   ask for that control
  VERYSLOW     write for about ten seconds
  CITETASK     set a task with two links beside it: a page of the past paper
               (opens at once) and a page of slides (shut until answered)
  CITE         cite page 2 of the scratch course's past paper as a link, name
               its practice paper as a path, and link the paper's answer key
  RESTART      stream part of the reply, start the same message again, then
               stream it whole
  CUTSHORT     stream only the start of the reply; the finished message has
               all of it, block included
  MEMORY found <text> / MEMORY cleared <text>   note a misunderstanding of
               "sign of b", revising the last memory id the state showed
  change the plan   (any case) move the exam in the plan the state names a
               week later and swap the two items after the one in hand
               (the first not done), by an Edit of the
               plan file as Claude Code would make it: only when the
               arguments give the Edit tool and allow Edit on that file;
               otherwise say the plan cannot be changed from here
  NEW GOAL <goal>   set the record's goal to <goal>
  by the way   (any case) answer an off-plan question: no task, no mark
  back to      (any case) pose the open task again, in the same words
  The stats course: when the state names the plan "Midterm 2" (the scratch
  course of scripts/scratch-course.sh), a lesson's first message (or words
  with no number while no task is open) with no other trigger poses "What is the standard error of the mean?" for a sample
  of 25 with s = 10; an answer of 2 is right (cold), any other is wrong; the
  next task asks for the 95% margin of error (t* = 2.064): about 4.13 is
  right.

The hidden request (it says the block "could not be read") is answered with
the block alone, after STAND_IN_REPAIR_DELAY seconds.
"""

import datetime
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


# The list a real Claude Code sends in its reply to `initialize`: aliases
# first, then a full id each for older models. The names and lines follow
# Claude Code 2.1.322's own; an account's real list differs.
ALL_EFFORTS = ["low", "medium", "high", "xhigh", "max"]
MODELS = [
    {"value": "default", "resolvedModel": "claude-opus-5-5", "displayName": "Default (recommended)",
     "description": "Use the default model (currently Opus 5.5)", "supportsEffort": True, "supportedEffortLevels": ALL_EFFORTS},
    {"value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5",
     "description": "Best for everyday, complex tasks", "supportsEffort": True, "supportedEffortLevels": ALL_EFFORTS},
    {"value": "fable", "resolvedModel": "claude-fable-5-2", "displayName": "Fable 5.2",
     "description": "Most capable for your hardest and longest-running tasks", "supportsEffort": True, "supportedEffortLevels": ALL_EFFORTS},
    {"value": "sonnet", "resolvedModel": "claude-sonnet-5-5", "displayName": "Sonnet 5.5",
     "description": "Efficient for routine tasks", "supportsEffort": True, "supportedEffortLevels": ALL_EFFORTS},
    {"value": "haiku", "resolvedModel": "claude-haiku-5-5", "displayName": "Haiku 5.5",
     "description": "Fastest for quick answers", "supportsEffort": True, "supportedEffortLevels": ["low", "medium", "high"]},
    {"value": "claude-opus-4-6", "resolvedModel": "claude-opus-4-6", "displayName": "Opus 4.6",
     "description": "Newer version available \u00b7 select opus for Opus 5.5", "supportsEffort": True, "supportedEffortLevels": ["low", "medium", "high", "max"]},
    {"value": "claude-haiku-4-5", "resolvedModel": "claude-haiku-4-5", "displayName": "Haiku 4.5",
     "description": "Newer version available \u00b7 select haiku for Haiku 5.5"},
]


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


def say(text, slow=False, mode=None):
    """Stream text in pieces of five characters, then the finished message."""
    message_id = "msg_" + uuid.uuid4().hex[:12]
    stream({"type": "message_start", "message": {"id": message_id}})
    if mode == "RESTART":
        stream({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
        for at in range(0, 20, 5):
            stream({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text[at:at + 5]}})
        stream({"type": "message_start", "message": {"id": message_id}})
    if mode == "CUTSHORT":
        stream({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
        stream({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text[:12]}})
        stream({"type": "content_block_stop", "index": 0})
        emit({"type": "assistant", "message": {"id": message_id, "content": [{"type": "text", "text": text}]},
              "parent_tool_use_id": None, "session_id": session})
        return True
    stream({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    for at in range(0, len(text), 5):
        if interrupted.is_set():
            return False
        stream({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text[at:at + 5]}})
        if slow:
            time.sleep(0.05 if slow is True else slow)
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


def flag_value(name):
    """The value after a flag, as the engine passes it: `--flag value`."""
    if name in args:
        at = args.index(name)
        if at + 1 < len(args):
            return args[at + 1]
    return ""


def rule_matches(rule, tool, path):
    """Whether one of Claude Code's permission rules, such as
    Edit(//root/Study/*/plan.md), covers `tool` on the absolute `path`. `**`
    takes any depth, `*` one folder name; `//` starts an absolute path."""
    found = re.fullmatch(r"(\w+)\((.*)\)", rule.strip())
    if not found or found.group(1) != tool:
        return False
    pattern = found.group(2)
    if not pattern.startswith("//"):
        return False
    pattern = pattern[1:]
    regex = ""
    at = 0
    while at < len(pattern):
        if pattern.startswith("**", at):
            regex += ".*"
            at += 2
        elif pattern[at] == "*":
            regex += "[^/]*"
            at += 1
        else:
            regex += re.escape(pattern[at])
            at += 1
    return re.fullmatch(regex, path) is not None


def split_rules(text):
    """A comma-separated list of rules, with no split inside parentheses."""
    rules, depth, current = [], 0, ""
    for c in text:
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        if c == "," and depth == 0:
            rules.append(current)
            current = ""
        else:
            current += c
    if current:
        rules.append(current)
    return rules


def may_edit(path):
    """Whether Claude Code, started with these arguments, would let the
    model Edit `path` without asking: Edit among the tools, an allow rule
    for it, and no deny rule."""
    tools = flag_value("--tools").replace(" ", ",").split(",")
    if "Edit" not in tools:
        return False
    allowed = split_rules(flag_value("--allowedTools"))
    denied = split_rules(flag_value("--disallowedTools"))
    if any(rule_matches(rule, "Edit", path) for rule in denied):
        return False
    return any(rule_matches(rule, "Edit", path) for rule in allowed)


def study_root():
    """The study root, as the Read rule names it, else the folder."""
    for rule in split_rules(flag_value("--allowedTools")):
        found = re.fullmatch(r"Read\(/(/.*)/\*\*\)", rule.strip())
        if found:
            return found.group(1)
    return os.getcwd()


def tool_call(name, tool_input):
    """A tool call as Claude Code streams it, and its result."""
    tool_id = "toolu_" + uuid.uuid4().hex[:12]
    message_id = "msg_" + uuid.uuid4().hex[:12]
    stream({"type": "message_start", "message": {"id": message_id}})
    stream({"type": "content_block_start", "index": 0,
            "content_block": {"type": "tool_use", "id": tool_id, "name": name, "input": {}}})
    stream({"type": "content_block_stop", "index": 0})
    emit({"type": "assistant", "message": {"id": message_id, "content": [
        {"type": "tool_use", "id": tool_id, "name": name, "input": tool_input}]},
        "parent_tool_use_id": None, "session_id": session})
    emit({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": tool_id, "content": "ok"}]},
        "parent_tool_use_id": None, "session_id": session})


def change_plan():
    """Move the exam a week later and swap the next two items of the plan
    the state named, as the real tutor would with Read and Edit. Returns the
    line that says what changed, or None when the plan cannot be edited."""
    if not plan_path[0]:
        return None
    path = os.path.join(study_root(), plan_path[0])
    if not os.path.isfile(path) or not may_edit(os.path.realpath(path)) and not may_edit(path):
        return None
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    tool_call("Read", {"file_path": path})
    lines = text.split("\n")
    moved_to = None
    for at, line in enumerate(lines):
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) >= 2 and cells[0].lower() == "when":
            found = re.search(r"\d{4}-\d{2}-\d{2}", line)
            if found:
                day = datetime.date.fromisoformat(found.group(0)) + datetime.timedelta(days=7)
                moved_to = day
                rest = line[found.end():]
                rest = re.sub(r"^ \((Mon|Tue|Wed|Thu|Fri|Sat|Sun)\)", " (" + day.strftime("%a") + ")", rest)
                lines[at] = line[:found.start()] + day.isoformat() + rest
            break
    # The Blocks table: the next two rows not done change places.
    swapped = None
    in_blocks = False
    header = None
    rows = []
    for at, line in enumerate(lines):
        if line.startswith("## "):
            in_blocks = "blocks" in line.lower()
            continue
        if in_blocks and line.strip().startswith("|"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if header is None:
                header = [c.lower() for c in cells]
            elif not all(set(c) <= set("-: ") for c in cells):
                rows.append((at, cells))
    if header and "block" in header:
        status_at = header.index("status") if "status" in header else None
        open_rows = [(at, cells) for at, cells in rows
                     if status_at is None or "done" not in cells[status_at].lower()]
        # The item in hand stays first; the two after it change places.
        if len(open_rows) >= 3:
            open_rows = open_rows[1:]
        if len(open_rows) >= 2:
            (first_at, first), (second_at, second) = open_rows[0], open_rows[1]
            number_at = header.index("#") if "#" in header else None
            if number_at is not None:
                first[number_at], second[number_at] = second[number_at], first[number_at]
            # The days stay in order: each row takes the day of the place it moves to.
            day_at = header.index("day") if "day" in header else None
            if day_at is not None:
                first[day_at], second[day_at] = second[day_at], first[day_at]
            lines[first_at] = "| " + " | ".join(second) + " |"
            lines[second_at] = "| " + " | ".join(first) + " |"
            name_at = header.index("block")
            swapped = (first[name_at], second[name_at])
    today = datetime.date.today().isoformat()
    change = f"- {today}: exam moved a week later and two items swapped, because the learner skipped days and the midterm moved."
    if lines and lines[-1] == "":
        lines.insert(len(lines) - 1, change)
    else:
        lines.append(change)
    new_text = "\n".join(lines)
    tool_call("Edit", {"file_path": path, "old_string": text, "new_string": new_text})
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(new_text)
    said = "Done:"
    if moved_to:
        said += f" the exam is now on {moved_to.strftime('%a')} {moved_to.day} {moved_to.strftime('%b')}"
    if swapped:
        said += f", and {swapped[1]} comes before {swapped[0]}"
    return said + ". Nothing is lost by the days off: we go on from where you are."


STATS_FIRST = "A sample of 25 students has mean study time 52 minutes and s = 10 minutes. What is the standard error of the mean?"
STATS_SECOND = "With t* = 2.064 for 24 degrees of freedom, what is the margin of error of a 95% interval for the mean?"
STATS_STEPS = [{"step": "The standard error", "status": "doing"}, {"step": "A 95% interval", "status": "todo"},
               {"step": "A test of the mean", "status": "todo"}]
STATS_DONE = [{"step": "The standard error", "status": "done"}, {"step": "A 95% interval", "status": "doing"},
              {"step": "A test of the mean", "status": "todo"}]

STEPS = [{"step": "Read a, b, c", "status": "doing"}, {"step": "Use the formula", "status": "todo"}]
DONE = [{"step": "Read a, b, c", "status": "done"}, {"step": "Use the formula", "status": "doing"}]
FIRST = "What is b in x² − 5x + 6 = 0?"
SECOND = "What is b in x² + 3x − 4 = 0?"
THIRD = "Solve x² − 5x + 6 = 0 with the formula."


def redo_question(number):
    return f"A Lineweaver-Burk plot crosses the x axis at −{number / 2}: what is Km?"


NEXT_REDO = {"question": "A second plot crosses at −0.25: what is Km?", "key": "4"}
# The numbers of the paper redos the state listed and this model has not posed.
redos_listed = []
# The numbers of the repairs the state listed, first listed first.
repairs_listed = []


def reply_for(learner, state):
    listed = re.findall(r'^(\d+)\. Tag "', state, re.M)
    if listed:
        redos_listed[:] = [int(number) for number in listed]
    for number in re.findall(r'^Repair (\d+)\. Tag "', state, re.M):
        if int(number) not in repairs_listed:
            repairs_listed.append(int(number))
    if "could not be read" in state:
        delay = float(os.environ.get("STAND_IN_REPAIR_DELAY", "0"))
        time.sleep(delay)
        if "NEVERBLOCK" in last_learner[0]:
            return "I cannot.", False
        return "<record>\n" + json.dumps({"answer": None, "help": "none", "task": None}) + "\n</record>", False
    last_learner[0] = learner
    for found in re.findall(r"- (m\d+): ", state):
        memory_ids.append(found)
    found = re.search(r"^Study plan: (Study/[^:\n]+/plan\.md): ", state, re.M)
    if found:
        plan_path[0] = found.group(1)
    if re.search(r"^Study plan: Study/[^:\n]+/plan\.md: Midterm 2,", state, re.M):
        course[0] = "stats"
    if plan_path[0] is None:
        # A resumed conversation's state may name no plan: look, as the
        # tutor could with Read.
        study = os.path.join(study_root(), "Study")
        for name in sorted(os.listdir(study)) if os.path.isdir(study) else []:
            path = os.path.join(study, name, "plan.md")
            if os.path.isfile(path):
                plan_path[0] = f"Study/{name}/plan.md"
                with open(path, encoding="utf-8") as handle:
                    if re.search(r"^\| *Target *\| *Midterm 2 *\|", handle.read(), re.M):
                        course[0] = "stats"
                break
    found = re.search(r"^Open task: (.+)$", state, re.M)
    if found:
        open_task[0] = None if found.group(1) == "none" else found.group(1)
    lower = learner.lower()
    if "change the plan" in lower:
        said = change_plan()
        if said is None:
            return "I can't change the plan file from here." + block(note="Asked to change the plan."), False
        return said + block(note="Changed the plan."), 0.03
    if learner.startswith("NEW GOAL "):
        goal = learner[len("NEW GOAL "):].strip()
        return f"From now on: {goal}." + block(note="A new goal.", goal=goal, steps=STATS_STEPS), False
    if "by the way" in lower:
        return ("Good question, and not on the plan, so nothing here is marked. Dividing by n − 1 makes "
                "s² an unbiased estimate of σ²: the deviations are measured from x̄, which sits closer to "
                "the data than μ does. Say when you want to go back to the task."
                + block(note="An off-plan question.")), False
    if "back to" in lower:
        if open_task[0]:
            return f"Back to it. {open_task[0]}" + block(note="Back to the task.", task=open_task[0]), False
        return "Back to the plan." + block(note="Back to the plan."), False
    if learner.startswith("MEMORY "):
        cleared = learner.startswith("MEMORY cleared ")
        text = learner.split(" ", 2)[2]
        note = {"id": memory_ids[-1] if memory_ids else None, "skill": "sign of b", "text": text, "cleared": cleared}
        return "Noted." + block(note="A misunderstanding.", memory=note), False
    if "RECORDING OFF" in learner:
        return "Recording is off now." + block(control="recording_off"), False
    if "RECORDING ON" in learner:
        return "Recording is on again." + block(control="recording_on"), False
    if "PAUSE" in learner:
        return "Paused." + block(control="pause"), False
    asks_help = learner == "I'm lost" or "hint" in learner.lower()
    if asks_help and due_open[0] and not warned[0]:
        warned[0] = True
        last_kind[0] = "warn"
        return "A hint on this question means it earns no pass today. Ask again if you want one." + block(
            note="Warned before a hint."), False
    if learner == "I'm lost":
        last_kind[0] = "help"
        return "What do you see in front of the 5?" + block(note="Lost: asked what they see."), False
    if "NEVERBLOCK" in learner or "NOBLOCK" in learner:
        return "A reply with no notes.", False
    if "CONTROLREPAIR" in learner:
        number = repairs_listed[0] if repairs_listed else 1
        return "Paused, and that finishes the repair." + block(control="pause", repair={
            "number": number, "done": "taught the step", "next": None}), False
    if "CHECK AGAIN" in learner:
        return "Quick check 1: What is b in 2x² − 7x − 4 = 0?" + block(task="What is b in 2x² − 7x − 4 = 0?"), False
    if "RIGHTCHECK" in learner:
        return f"Right. {THIRD}" + block(answer="right", skill="reading coefficients", cold=True,
                                         note="Read b with its sign.", task=THIRD), False
    if "BADREPAIR" in learner:
        return "That finishes the repair." + block(repair={"done": "taught the step"}), False
    if "REPAIRED" in learner or re.search(r"REPAIR \d+", learner):
        named = re.search(r"REPAIR (\d+)", learner)
        number = int(named.group(1)) if named else (repairs_listed[0] if repairs_listed else 1)
        if number in repairs_listed:
            repairs_listed.remove(number)
        return "Right: that is the step that was missing. That finishes the repair." + block(
            note="The last check of the repair.",
            repair={"number": number, "done": "taught reading Km off the x intercept; three checks",
                    "next": {"question": "A plot crosses the x axis at −0.2: what is Km?", "key": "5"}}), False
    if "CITETASK" in learner:
        return ("Take [Midterm 2023, p. 3](Study/midterm-2/papers/Midterm%202023.pdf#page=3) as it is, and read "
                "[Week 6 slides, p. 3](Study/materials/week6-slides.pdf#page=3) after. "
                "**Task:** What makes a confidence interval wider?") + block(task="What makes a confidence interval wider?"), False
    if "CITE" in learner:
        return ("The past paper asks it: [Midterm 2023, p. 2](Study/midterm-2/papers/Midterm%202023.pdf#page=2). "
                "The practice paper is `Study/midterm-2/papers/Practice midterm #2.pdf`, and the past paper's key is "
                "[the answers](Study/midterm-2/papers/Midterm%202023%20key.pdf).") + block(), False
    if "VERYSLOW" in learner:
        return "Ten seconds of careful words, written one small piece at a time, so that it takes a while to finish." + block(), 0.5
    if "SLOW" in learner:
        return "Let me think about that slowly, one small word after another, for a while." + block(), True
    if "TEACH" in learner:
        return f"Let's start. {FIRST}" + block(note="Lesson start.", task=FIRST, steps=STEPS), False
    if ("BACK" in learner or learner.startswith("I'm back")) and "Quick check" not in learner:
        if re.search(r'1\. Skill "([^"]+)"', state) or "Quick check 1 is still due" in state:
            return "Quick check 1: What is b in 2x² − 7x − 4 = 0?" + block(task="What is b in 2x² − 7x − 4 = 0?"), False
        if redos_listed:
            number = redos_listed.pop(0)
            return f"Paper redo {number}: {redo_question(number)}" + block(task=redo_question(number)), False
        if "BACK" in learner:
            return "Welcome back. " + THIRD + block(task=THIRD), False
    if course[0] == "stats":
        if open_task[0] == STATS_FIRST:
            if re.search(r"(?<![\d.])2(?:\.0+)?(?![\d.])", learner):
                return f"Right: s/√n = 10/5 = 2 minutes. Next step. {STATS_SECOND}" + block(
                    answer="right", skill="standard error of a mean", note="Divided s by the root of n.",
                    task=STATS_SECOND, steps=STATS_DONE, cold=True), False
            if re.search(r"\d", learner):
                return f"Not quite: the standard error is s/√n, here 10/√25 = 2. Try it once more. {STATS_FIRST}" + block(
                    answer="wrong", skill="standard error of a mean", note="Did not divide by the root of n.",
                    task=STATS_FIRST), False
        if open_task[0] == STATS_SECOND and re.search(r"4\.1", learner):
            return "Right: 2.064 × 2 ≈ 4.13 minutes, so the interval is 47.9 to 56.1." + block(
                answer="right", skill="margin of error", note="Multiplied t* by the standard error.", cold=True), False
        # A lesson's first message (the state says no task is open), or
        # any words with no number while none is.
        if open_task[0] is None and ("Open task: none" in state or not re.search(r"\d", learner)):
            return f"Let's start with the standard error. {STATS_FIRST}" + block(
                note="Lesson start.", task=STATS_FIRST, steps=STATS_STEPS), False
    if "Km = 2" in learner:
        marked = dict(answer="right", skill="reading Km from a plot", note="Read Km off the x intercept.",
                      slow="OVERTIME" in learner, next=NEXT_REDO, cold="TAUGHT" not in learner)
        if redos_listed:
            number = redos_listed.pop(0)
            return f"Right: Km is 2. Paper redo {number}: {redo_question(number)}" + block(
                task=redo_question(number), **marked), False
        return f"Right: Km is 2. {THIRD}" + block(task=THIRD, **marked), False
    if "REWORD" in learner:
        return "Paper redo 1: Read Km off the plot that crosses the x axis at −0.5." + block(
            task="Read Km off the plot that crosses the x axis at −0.5."), False
    if "MENTION" in learner:
        return f"First a warm-up, then paper redo 2. {FIRST}" + block(task=FIRST), False
    if "Km = 0.5" in learner:
        return "Not quite: Km is minus one over the intercept. This one comes back after a repair." + block(
            answer="wrong", skill="reading Km from a plot", note="Took the intercept itself as Km."), False
    if "hint" in learner.lower():
        last_kind[0] = "help"
        return "Look at the sign in front of the 5." + block(help="hint"), False
    if "b = -5" in learner or "b = −5" in learner:
        return f"Right: the sign goes with the number. {THIRD}" + block(
            answer="right", skill="sign of b", note="Kept the sign.", task=THIRD, steps=DONE,
            cold="COLD" in learner), False
    if "b = 5" in learner:
        return f"Not quite: b keeps its minus sign, so b = −5. Try this one: {SECOND}" + block(
            answer="wrong", skill="sign of b", kind="reading coefficients", note="Dropped the sign of b.",
            task=SECOND, memory={"skill": "sign of b", "text": "Drops the minus sign of b."}), False
    return "Go on." + block(note="A comment."), False


last_learner = [""]
# The plan the state named, the course it belongs to, and the task open.
plan_path = [None]
course = [None]
open_task = [None]
# A due quick check or paper redo is the task on screen; the learner was
# warned once that help on it means no pass; what the last reply was.
due_open = [False]
warned = [False]
last_kind = [None]
memory_ids = []


def turn(content):
    text = content if isinstance(content, str) else "".join(c.get("text", "") for c in content)
    learner = re.search(r"<learner>\n?(.*?)\n?</learner>", text, re.S)
    learner = learner.group(1) if learner else ""
    state = re.search(r"<lesson-state>\n?(.*?)\n?</lesson-state>", text, re.S)
    state = state.group(1) if state else ""
    log("message", {"learner": learner, "state": state})
    init()
    last_kind[0] = None
    reply, slow = reply_for(learner, state)
    posed = re.search(r"<record>\n(.*)\n</record>", reply, re.S)
    if posed and "could not be read" not in state:
        try:
            record = json.loads(posed.group(1))
            if record.get("task"):
                open_task[0] = record["task"]
            elif record.get("answer") and record.get("answer") != "declined":
                open_task[0] = None
        except ValueError:
            pass
    if last_kind[0] is None and "could not be read" not in state:
        due_open[0] = bool(re.search(r"(Quick check|Paper redo) \d+:", reply))
        warned[0] = False
    mode = next((m for m in ("RESTART", "CUTSHORT") if m in learner), None)
    if not say(reply, slow, mode):
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
            subtype = message["request"].get("subtype")
            if subtype == "interrupt":
                interrupted.set()
                with lock:
                    pending.clear()
            response = {"commands": [], "models": MODELS} if subtype == "initialize" else {}
            emit({"type": "control_response",
                  "response": {"subtype": "success", "request_id": message["request_id"], "response": response}})
        elif message.get("type") == "user":
            with lock:
                pending.append(message["message"]["content"])
                lock.notify()
    with lock:
        pending.append(None)
        lock.notify()
    thread.join()


main()
