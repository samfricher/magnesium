from __future__ import annotations

import json
import os
import queue
import shutil
import signal
import subprocess
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

SIGNED_OUT = ("not logged in", "please run /login", "invalid api key", "authentication_error", "oauth token has expired")


class SignedOut(RuntimeError):
    pass


def program() -> str:
    found = shutil.which("claude") or str(Path.home() / ".local" / "bin" / "claude")
    return str(Path(found).resolve())


@dataclass
class Reply:
    text: str = ""
    tools: list[dict] = field(default_factory=list)
    results: list[dict] = field(default_factory=list)
    usage: dict = field(default_factory=dict)
    cost_usd: float = 0.0
    seconds: float = 0.0
    error: str | None = None
    session_id: str | None = None


def add_usage(total: dict, usage: dict):
    for key in ("input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"):
        total[key] = total.get(key, 0) + int(usage.get(key) or 0)


def signed_out(text: str) -> bool:
    lowered = (text or "").lower()
    return any(marker in lowered for marker in SIGNED_OUT)


class Conversation:
    def __init__(self, cwd: Path, env: dict, args: list[str], log: Path):
        self.log = open(log, "a", encoding="utf-8")
        command = [program(), "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
                   "--include-partial-messages", "--permission-prompt-tool", "stdio", *args]
        self.proc = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, bufsize=1, start_new_session=True)
        self.lines: queue.Queue = queue.Queue()
        self.stderr: list[str] = []
        self.session_id: str | None = None
        self.init: dict | None = None
        self.counter = 0
        self.background: set[str] = set()
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._read_err, daemon=True).start()
        self._write({"type": "control_request", "request_id": self._id(), "request": {"subtype": "initialize", "hooks": None}})

    def _id(self) -> str:
        self.counter += 1
        return f"ys_{self.counter}"

    def _read(self):
        for line in self.proc.stdout:
            self.log.write(line)
            self.log.flush()
            try:
                self.lines.put(json.loads(line))
            except json.JSONDecodeError:
                continue
        self.lines.put(None)

    def _read_err(self):
        for line in self.proc.stderr:
            self.stderr.append(line)

    def _write(self, value: dict):
        self.proc.stdin.write(json.dumps(value) + "\n")
        self.proc.stdin.flush()

    def _answer(self, value: dict):
        request = value.get("request") or {}
        rid = value.get("request_id")
        if request.get("subtype") != "can_use_tool":
            self._write({"type": "control_response", "response": {"subtype": "error", "request_id": rid, "error": "not handled"}})
            return
        tool_input = request.get("input") or {}
        if request.get("tool_name") == "AskUserQuestion":
            response = {"behavior": "deny", "message": "The learner cannot answer a question form here. Ask it in your reply."}
        else:
            response = {"behavior": "allow", "updatedInput": tool_input}
        self._write({"type": "control_response", "response": {"subtype": "success", "request_id": rid, "response": response}})

    def say(self, text: str, timeout: float = 1200) -> Reply:
        self._write({"type": "user", "message": {"role": "user", "content": text}, "parent_tool_use_id": None, "session_id": "default"})
        return self._collect(timeout)

    def wait_background(self, timeout: float = 3600, quiet: float = 180) -> Reply | None:
        if not self.background:
            return None
        return self._collect(timeout, quiet_after_background=quiet)

    def _collect(self, timeout: float, quiet_after_background: float | None = None) -> Reply:
        reply = Reply()
        started = time.monotonic()
        seen: set[str] = set()
        idle_since = None
        while True:
            left = timeout - (time.monotonic() - started)
            if left <= 0:
                reply.error = "timed out"
                break
            try:
                value = self.lines.get(timeout=min(left, 5))
            except queue.Empty:
                if self.proc.poll() is not None:
                    reply.error = "Claude Code exited: " + "".join(self.stderr[-20:])[-1500:]
                    break
                if quiet_after_background is not None and not self.background:
                    idle_since = idle_since or time.monotonic()
                    if time.monotonic() - idle_since > quiet_after_background:
                        break
                continue
            idle_since = None
            if value is None:
                reply.error = "Claude Code exited: " + "".join(self.stderr[-20:])[-1500:]
                break
            kind = value.get("type")
            if kind == "system" and value.get("subtype") == "background_tasks_changed":
                self.background = {str(t.get("task_id")) for t in value.get("tasks") or []}
            if kind == "system" and value.get("subtype") == "init":
                self.init = value
                self.session_id = value.get("session_id") or self.session_id
            elif kind == "control_request":
                self._answer(value)
            elif kind == "assistant" and not value.get("parent_tool_use_id"):
                for block in (value.get("message") or {}).get("content") or []:
                    if block.get("type") == "tool_use" and block.get("id") not in seen:
                        seen.add(block.get("id"))
                        reply.tools.append({"id": block.get("id"), "name": block.get("name"), "input": block.get("input") or {}})
                    elif block.get("type") == "text":
                        reply.text += block.get("text", "")
            elif kind == "user":
                for block in (value.get("message") or {}).get("content") or []:
                    if isinstance(block, dict) and block.get("type") == "tool_result":
                        content = block.get("content")
                        body = content if isinstance(content, str) else "".join(
                            part.get("text", "") for part in content or [] if isinstance(part, dict))
                        reply.results.append({"id": block.get("tool_use_id"), "chars": len(body), "error": bool(block.get("is_error"))})
            elif kind == "result":
                if quiet_after_background is not None and self.background:
                    add_usage(reply.usage, value.get("usage") or {})
                    reply.cost_usd += float(value.get("total_cost_usd") or 0)
                    continue
                reply.session_id = value.get("session_id")
                self.session_id = reply.session_id or self.session_id
                add_usage(reply.usage, value.get("usage") or {})
                reply.cost_usd += float(value.get("total_cost_usd") or 0)
                if value.get("is_error"):
                    reply.error = str(value.get("result") or value.get("subtype"))
                if signed_out(str(value.get("result") or "")) or signed_out(reply.text):
                    raise SignedOut(str(value.get("result") or reply.text)[:500])
                break
        reply.seconds = time.monotonic() - started
        return reply

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(self.proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.proc.wait()
        self.log.close()


def ask(prompt: str, env: dict, cwd: Path, model: str, effort: str, schema: dict, timeout: float = 600) -> dict:
    command = [program(), "-p", "--output-format", "json", "--model", model, "--effort", effort, "--tools", "",
               "--setting-sources", "", "--no-session-persistence", "--json-schema", json.dumps(schema)]
    started = time.monotonic()
    out = subprocess.run(command, cwd=cwd, env=env, input=prompt, capture_output=True, text=True, timeout=timeout)
    seconds = time.monotonic() - started
    try:
        value = json.loads(out.stdout)
    except json.JSONDecodeError:
        if signed_out(out.stdout + out.stderr):
            raise SignedOut((out.stdout + out.stderr)[:500])
        raise RuntimeError(f"judge gave no JSON: {(out.stdout + out.stderr)[:800]}")
    if signed_out(str(value.get("result"))) and value.get("is_error"):
        raise SignedOut(str(value.get("result"))[:500])
    answer = value.get("structured_output")
    if answer is None:
        try:
            answer = json.loads(value.get("result") or "")
        except (json.JSONDecodeError, TypeError):
            answer = {}
    usage: dict = {}
    add_usage(usage, value.get("usage") or {})
    return {"answer": answer, "usage": usage, "seconds": seconds, "cost_usd": float(value.get("total_cost_usd") or 0)}
