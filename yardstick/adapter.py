from __future__ import annotations

import asyncio
import json
import os
import pwd
import re
import shutil
import sqlite3
import subprocess
import sys
from contextlib import closing
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from pathlib import Path

from common import REPO, now_local

sys.path.insert(0, str(REPO))
from ci.native_mcp import NativeMcpClient, TransportLimits

import claude_code

SCHEMA = "learner-model/v2"
HOST = ["--plugin-version", "0.2.0", "--adapter-protocol-version", "2", "--host-kind", "claude"]
HOOK_EVENTS = {
    "session-start": "SessionStart",
    "user-prompt-submit": "UserPromptSubmit",
    "stop": "Stop",
    "session-end": "SessionEnd",
    "post-tool-use": "PostToolUse",
}
MAP_WORDS = ("map", "course_map", "items", "turn")
LESSON_TOOLS = ["Read", "Write", "Edit", "Glob", "Grep", "AskUserQuestion", "WebSearch", "WebFetch", "Bash"]
DRAWS = (
    "This lesson is shown in Magnesium's study window, which draws your replies as Markdown and draws "
    "mathematics with KaTeX. Write every formula in TeX, even a short one inside a sentence: inline as "
    "`$\\frac{1}{1+e^{-s}}$` (no space after the opening dollar), display as `$$...$$` on its own lines. "
    "Maths written as plain text, such as 1/(1 + e^(−s)), is shown as typed, never as a formula. "
    "A diagram is a fenced `mermaid` block."
)
TOOL_PREFIX = "mcp__plugin_magnesium_magnesium__"


def real_home() -> Path:
    return Path(pwd.getpwuid(os.getuid()).pw_dir)


@dataclass
class Place:
    root: Path
    folder: Path
    home: Path
    tmp: Path
    bin: Path
    surface: str | None = None

    def env(self, **extra) -> dict:
        env = {
            "HOME": str(self.home),
            "USER": os.environ.get("USER", pwd.getpwuid(os.getuid()).pw_name),
            "LOGNAME": os.environ.get("LOGNAME", pwd.getpwuid(os.getuid()).pw_name),
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "TMPDIR": str(self.tmp),
        }
        if self.surface:
            env["MAGNESIUM_SURFACE"] = self.surface
        env.update({k: v for k, v in extra.items() if v is not None})
        return env


@dataclass
class Call:
    tool: str
    chars: int
    pretty_chars: int
    marked: bool = False
    record: bool = False
    error: bool = False
    note: str = ""


@dataclass
class Turn:
    text: str
    calls: list[Call] = field(default_factory=list)
    hook_chars: int = 0


class Today:
    name = "today"
    description = "main as it stands: CLI, hooks and MCP tools, several calls per learner message"

    def __init__(self, binary: Path, plugin: Path | None, work: Path):
        self.source_binary = Path(binary).resolve()
        if not self.source_binary.is_file():
            raise SystemExit(f"No Magnesium binary at {self.source_binary}")
        self.plugin = Path(plugin).resolve() if plugin else None
        self.work = Path(work).resolve()
        self.work.mkdir(parents=True, exist_ok=True)
        self.bin = self.work / "bin"
        self.bin.mkdir(exist_ok=True)
        self.binary = self.bin / "magnesium"
        if not self.binary.exists() or self.binary.stat().st_mtime < self.source_binary.stat().st_mtime:
            shutil.copy2(self.source_binary, self.binary)

    def version(self) -> str:
        out = subprocess.run([str(self.binary), "--version"], capture_output=True, text=True)
        return out.stdout.strip()

    def place(self, source: Path | None, name: str, live: bool = False, surface: str | None = None) -> Place:
        root = self.work / name
        if root.exists():
            shutil.rmtree(root)
        folder = root / "folder"
        if source:
            shutil.copytree(source, folder, symlinks=False, ignore=shutil.ignore_patterns(".magnesium", ".DS_Store"))
        else:
            folder.mkdir(parents=True)
        home, tmp = root / "home", root / "tmp"
        home.mkdir(parents=True)
        tmp.mkdir()
        if live:
            (home / "Library").mkdir()
            os.symlink(real_home() / "Library" / "Keychains", home / "Library" / "Keychains")
        place = Place(root=root, folder=folder, home=home, tmp=tmp, bin=self.bin, surface=surface)
        code, out, err = self.cli(place, "init", "--json")
        if code:
            raise SystemExit(f"magnesium init failed in {folder}: {err or out}")
        return place

    def place_with_record(self, source: Path, name: str) -> Place | None:
        if not (source / ".magnesium").is_dir():
            return None
        root = self.work / name
        if root.exists():
            shutil.rmtree(root)
        folder = root / "folder"
        shutil.copytree(source, folder, symlinks=False)
        home, tmp = root / "home", root / "tmp"
        home.mkdir(parents=True)
        tmp.mkdir()
        return Place(root=root, folder=folder, home=home, tmp=tmp, bin=self.bin)

    def cli(self, place: Place, *args: str) -> tuple[int, str, str]:
        out = subprocess.run([str(self.binary), *args], cwd=place.folder, env=place.env(), capture_output=True, text=True)
        return out.returncode, out.stdout, out.stderr

    def cli_json(self, place: Place, *args: str):
        code, out, err = self.cli(place, *args)
        if code:
            return None
        try:
            return json.loads(out)
        except json.JSONDecodeError:
            return None

    def attempts(self, place: Place) -> list[dict]:
        value = self.cli_json(place, "attempts", "--json")
        return (value or {}).get("attempts", [])

    def progress(self, place: Place) -> dict:
        return self.cli_json(place, "progress", "--json") or {}

    def last_learner_message_at(self, place: Place, at_ms: int):
        store = place.folder / ".magnesium" / "sittings.sqlite"
        with closing(sqlite3.connect(store)) as db, db:
            db.execute("UPDATE meta SET last_learner_ms = ?", (at_ms,))

    def lesson(self, place: Place, conversation: str | None = None) -> dict | None:
        args = ["lesson", "--json"] + (["--conversation", conversation] if conversation else [])
        return self.cli_json(place, *args)

    def tool_names(self, place: Place) -> list[str]:
        async def go():
            async with self.channel(place, "probe-tools") as channel:
                listed = await channel.client.request("tools/list", {})
                return [t["name"] for t in listed.get("tools", [])]

        return asyncio.run(go())

    def course_map(self, place: Place, target: str | None) -> dict:
        tools = self.tool_names(place)
        helptext = subprocess.run([str(self.binary), "--help"], capture_output=True, text=True).stdout
        commands = [line.split()[0] for line in helptext.splitlines() if line.startswith("  ") and line.split()]
        tool_hits = [t for t in tools if any(w in t for w in MAP_WORDS)]
        command_hits = [c for c in commands if c in MAP_WORDS]
        page = place.folder / target / "map.md" if target else None
        found = bool(tool_hits or command_hits or (page and page.is_file()))
        return {
            "state": "present" if found else "none",
            "tools": tools,
            "map_tools": tool_hits,
            "map_commands": command_hits,
            "map_page": str(page.relative_to(place.folder)) if page else None,
            "map_page_exists": bool(page and page.is_file()),
        }

    def flagged_untaught(self, place: Place, target: str | None) -> list[str] | None:
        return None

    def use_links(self, place: Place, target: str | None) -> list | None:
        return None

    def hook(self, place: Place, event: str, conversation: str, **payload) -> str:
        body = {"session_id": conversation, "cwd": str(place.folder), "hook_event_name": HOOK_EVENTS[event], **payload}
        out = subprocess.run(
            [str(self.binary), "_internal", "hook", event, *HOST],
            cwd=place.folder,
            env=place.env(CLAUDE_CODE_SESSION_ID=conversation),
            input=json.dumps(body),
            capture_output=True,
            text=True,
        )
        text = out.stdout.strip()
        if not text:
            return ""
        try:
            value = json.loads(text)
        except json.JSONDecodeError:
            return text
        return (value.get("hookSpecificOutput") or {}).get("additionalContext") or ""

    def channel(self, place: Place, conversation: str):
        return Channel(self, place, conversation)

    def seed(self, place: Place, turns: list[dict], conversation: str = "seed", end_sitting: bool = False,
             base: datetime | None = None) -> list[dict]:
        return asyncio.run(self._seed(place, turns, conversation, end_sitting, base))

    async def _seed(self, place: Place, turns: list[dict], conversation: str, end_sitting: bool = False,
                    base: datetime | None = None) -> list[dict]:
        done = []
        async with self.channel(place, conversation) as channel:
            session = None
            for n, turn in enumerate(turns):
                stamp = when(turn, base)
                events = [{"external_event_key": f"{conversation}-{n}", "speaker": "learner", "kind": "message",
                           "text": turn["learner"], "occurred_at_ms": stamp}]
                request = {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-r{n}", "reported_events": events}
                if session is None:
                    request["session"] = {"operation": "start", "client_ref": "lesson", "goal": turn.get("goal", "Seeded record")}
                else:
                    request["session"] = channel.use(session)
                result = await channel.call("learning_step", request)
                session = session or result["references"]["lesson"]
                channel.note(result)
                event = result["captured_event_ids"][0]
                if turn.get("attempt"):
                    record = {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-a{n}",
                              "observations": [observation(f"w{n}", event, turn)],
                              "event_dispositions": [{"event_id": event, "outcome": "processed"}],
                              "session": channel.use(session)}
                    result = await channel.call("learning_step", record)
                    channel.note(result)
                else:
                    await channel.call("learning_step", {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-n{n}",
                                                         "event_dispositions": [{"event_id": event, "outcome": "no_change"}],
                                                         "session": channel.use(session)})
                done.append({"turn": n, "event": event, "at_ms": stamp})
            if end_sitting:
                await channel.call("learning_step", {"schema": SCHEMA, "action": "end_sitting", "request_id": f"{conversation}-end"})
        return done

    def scripted_lesson(self, place: Place, script: dict, conversation: str = "scripted") -> list[Turn]:
        return asyncio.run(self._scripted(place, script, conversation))

    async def _scripted(self, place: Place, script: dict, conversation: str) -> list[Turn]:
        turns: list[Turn] = []
        self.hook(place, "session-start", conversation, source="startup")
        async with self.channel(place, conversation) as channel:
            session = None
            for n, step in enumerate(script["turns"]):
                prompt_id = f"p{n}"
                text = step["learner"]
                turn = Turn(text=text)
                context = self.hook(place, "user-prompt-submit", conversation, prompt_id=prompt_id, prompt=text)
                turn.hook_chars += len(context)
                channel.calls = turn.calls
                if session is None:
                    ticket = re.search(r"native_capture\.ticket=([0-9a-f]+)", context)
                    for stage in script.get("stages", ["teach_and_practice"]):
                        await channel.call("context", {"action": "instructions", "host": "claude", "stage": stage})
                    start = {"schema": SCHEMA, "action": "update", "request_id": f"start-{conversation}",
                             "session": {"operation": "start", "client_ref": "lesson", "goal": script["goal"]}}
                    if ticket:
                        start["native_capture"] = {"ticket": ticket.group(1), "text": text}
                    else:
                        start["reported_events"] = [{"external_event_key": f"{conversation}-0", "speaker": "learner", "kind": "message", "text": text}]
                    result = await channel.call("learning_step", start)
                    session = result["references"]["lesson"]
                    channel.note(result)
                    event = (result.get("captured_event_ids") or [None])[0]
                    closed = False
                else:
                    found = re.search(r"current event (e-[0-9a-f]+)", context)
                    event = found.group(1) if found else None
                    closed = False
                    if step.get("attempt") and event:
                        record = {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-a{n}",
                                  "observations": [observation(f"w{n}", event, step)],
                                  "event_dispositions": [{"event_id": event, "outcome": "processed"}],
                                  "session": channel.use(session)}
                        result = await channel.call("learning_step", record, marked=True)
                        channel.note(result)
                        closed = True
                task = step.get("task")
                if not task and not closed and event:
                    result = await channel.call("learning_step", {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-d{n}",
                                                                  "event_dispositions": [{"event_id": event, "outcome": "no_change"}],
                                                                  "session": channel.use(session)})
                    channel.note(result)
                    closed = True
                reply_text = step["tutor"]
                reply = await channel.call("reply", {"text": reply_text, "save_after": bool(task)})
                self.hook(place, "post-tool-use", conversation, tool_name=TOOL_PREFIX + "reply",
                          tool_input={"text": reply_text}, tool_response=reply)
                if task:
                    checkpoint = {"current_problem": step.get("problem", script["goal"]),
                                  "working_diagnosis": step.get("diagnosis", "Working on the task."),
                                  "next_action": "Mark the answer to the task.",
                                  "pending_task": {"proposal_event_ref": reply.get("event_id"), "conditions": task}}
                    request = {"schema": SCHEMA, "action": "update", "request_id": f"{conversation}-c{n}",
                               "checkpoint": checkpoint, "session": channel.use(session)}
                    if not closed and event:
                        request["event_dispositions"] = [{"event_id": event, "outcome": "no_change"}]
                    result = await channel.call("learning_step", request)
                    channel.note(result)
                self.hook(place, "stop", conversation, prompt_id=prompt_id, stop_hook_active=False, last_assistant_message=reply_text)
                turns.append(turn)
        return turns

    def lesson_command(self, place: Place, model: str, effort: str) -> list[str]:
        folders = [place.folder]
        real = place.folder.resolve()
        if real != place.folder:
            folders.append(real)
        deny = [f"{tool}(/{f}/.magnesium/**)" for f in folders for tool in ("Write", "Edit")]
        allow = [f"{tool}(/{f}/Study/*/{name})" for f in folders for name in ("plan.md", "graph.yaml") for tool in ("Edit", "Write")]
        settings = {
            "sandbox": {"enabled": True, "autoAllowBashIfSandboxed": False, "allowUnsandboxedCommands": False},
            "permissions": {"deny": deny, "allow": allow},
            "enableAllProjectMcpServers": False,
        }
        args = ["--setting-sources", "project,local", "--tools", ",".join(LESSON_TOOLS), "--append-system-prompt", DRAWS,
                "--permission-mode", "auto", "--model", model, "--effort", effort]
        plugin = self.plugin or (place.home / "Library/Application Support/magnesium/claude-plugin")
        args += ["--plugin-dir", str(plugin), "--settings", json.dumps(settings)]
        return args

    def live(self, place: Place, model: str, effort: str, log: Path) -> "claude_code.Conversation":
        place.surface = "window"
        env = place.env(ENABLE_CLAUDEAI_MCP_SERVERS="false", PWD=str(place.folder))
        return claude_code.Conversation(cwd=place.folder, env=env, args=self.lesson_command(place, model, effort), log=log)

    def start_line(self, goal: str) -> str:
        return f"/magnesium:learn {goal.strip()}"

    def papers_line(self, request: str) -> str:
        return f"/magnesium:papers {request.strip()}"

    def transcript_of(self, place: Place, session_id: str) -> Path | None:
        projects = place.home / ".claude" / "projects"
        for path in projects.rglob(f"{session_id}.jsonl"):
            return path
        return None

    def tutor_text(self, tool: str, tool_input: dict) -> str | None:
        if tool == TOOL_PREFIX + "reply":
            return tool_input.get("text")
        return None

    def is_runtime_tool(self, tool: str) -> bool:
        return tool.startswith(TOOL_PREFIX)

    def is_reply_tool(self, tool: str) -> bool:
        return tool == TOOL_PREFIX + "reply"

    def is_record_tool(self, tool: str) -> bool:
        return tool == TOOL_PREFIX + "learning_step"

    def carries_mark(self, tool: str, tool_input: dict) -> bool:
        if not self.is_record_tool(tool):
            return False
        return any(o.get("attempt") or o.get("attempts") for o in tool_input.get("observations") or [])

    def posed_check(self, reply: str) -> bool:
        return re.search(r"quick check \d+(?!\d)", reply.lower()) is not None


class Channel:
    def __init__(self, adapter: Today, place: Place, conversation: str):
        self.adapter, self.place, self.conversation = adapter, place, conversation
        self.client = None
        self.calls: list[Call] = []
        self.revisions: dict[str, int] = {}

    async def __aenter__(self):
        env = self.place.env(CLAUDE_CODE_SESSION_ID=self.conversation)
        self.client = NativeMcpClient([str(self.adapter.binary), "_internal", "mcp-server", *HOST], cwd=str(self.place.folder), env=env,
                                      limits=TransportLimits(request_timeout=60.0, max_frame_bytes=8 * 1024 * 1024))
        await self.client.__aenter__()
        return self

    async def __aexit__(self, *exc):
        await self.client.__aexit__(*exc)

    def use(self, session: str) -> dict:
        return {"operation": "use", "session_id": session, "expected_revision": self.revisions.get(session, 1)}

    def note(self, result: dict):
        for key, value in (result.get("revisions") or {}).items():
            self.revisions[key] = value

    async def call(self, tool: str, arguments: dict, marked: bool = False) -> dict:
        raw = await self.client.request("tools/call", {"name": tool, "arguments": arguments})
        content = (raw.get("content") or [{}])[0].get("text", "")
        error = raw.get("isError") is True
        try:
            value = json.loads(content)
        except json.JSONDecodeError:
            value = {"text": content}
        pretty = len(json.dumps(value, indent=2, ensure_ascii=False)) if isinstance(value, dict) else len(content)
        self.calls.append(Call(tool=tool, chars=len(content), pretty_chars=pretty, marked=marked, record=tool == "learning_step", error=error,
                               note=content[:300] if error else ""))
        if error:
            raise RuntimeError(f"{tool} failed: {content[:1500]}")
        return value


def when(turn: dict, base: datetime | None = None) -> int:
    day = (base or now_local()).replace(hour=0, minute=0, second=0, microsecond=0) + timedelta(days=turn.get("day", 0))
    hour, minute = (int(x) for x in turn.get("time", "12:00").split(":"))
    moment = day.replace(hour=hour, minute=minute)
    latest = now_local() - timedelta(minutes=5)
    if moment > latest:
        moment = latest
    return int(moment.timestamp() * 1000)


def observation(ref: str, event: str, turn: dict) -> dict:
    attempt = dict(turn["attempt"])
    attempt.setdefault("question", turn.get("task_asked") or turn["learner"][:200])
    return {"client_ref": ref, "kind": "learner_work", "description": turn.get("shows", "The learner's answer to the task."),
            "events": [{"event_id": event, "role": "learner_response"}], "attempt": attempt}



ADAPTERS = {"today": Today}


def make(name: str, binary: Path, plugin: Path | None, work: Path):
    if name not in ADAPTERS:
        raise SystemExit(f"Unknown adapter {name}. Known: {', '.join(ADAPTERS)}")
    return ADAPTERS[name](binary, plugin, work)
