"""Bounded test-only stdio MCP client for an explicitly selected native child.

No installed assistant account or provider invocation. One outstanding request
owns its entire write/read deadline. stderr is drained concurrently with bounded
retained evidence and a flood limit. The context reaps only its owned child and
private POSIX process group.
"""
from __future__ import annotations
import asyncio
from dataclasses import dataclass
import json
import os
import signal
from typing import Any, Mapping, Sequence


class NativeMcpError(RuntimeError):
    """Bounded native transport/protocol/tool failure."""


@dataclass(frozen=True)
class TransportLimits:
    request_timeout: float = 15.0
    shutdown_timeout: float = 1.0
    max_frame_bytes: int = 2 * 1024 * 1024
    max_notifications: int = 32
    max_stderr_bytes: int = 16 * 1024
    max_stderr_total_bytes: int = 1024 * 1024

    def __post_init__(self):
        if any(value <= 0 for value in self.__dict__.values()):
            raise ValueError("native transport limits must be positive")


class NativeMcpClient:
    def __init__(self, command: Sequence[str], *, cwd: str, env: Mapping[str, str],
                 limits: TransportLimits | None = None):
        self.command, self.cwd, self.env = list(command), cwd, dict(env)
        self.limits = limits or TransportLimits()
        self.proc = None
        self.next_id = 0
        self._outstanding = False
        self._failed = False
        self._stderr_tail = bytearray()
        self._stderr_total = 0
        self._stderr_task = None
        self._stderr_failure = None
        self._notifications = 0
        self._last_frame = None
        self._closed = False
        self._cleanup_error = None
        self._group_cleanup = "NOT_RUN"

    async def start(self):
        if self.proc is not None:
            raise NativeMcpError("native client already started")
        self._stderr_failure = asyncio.get_running_loop().create_future()
        async with asyncio.timeout(self.limits.request_timeout):
            self.proc = await asyncio.create_subprocess_exec(
                *self.command, cwd=self.cwd, env=self.env,
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE, limit=self.limits.max_frame_bytes + 1,
                start_new_session=os.name == "posix",
            )
        self._stderr_task = asyncio.create_task(self._drain_stderr())
        return self

    async def __aenter__(self):
        try:
            await self.start()
            await self.initialize()
            return self
        except BaseException:
            await self.close()
            raise

    async def __aexit__(self, *_):
        await self.close()

    async def _drain_stderr(self):
        while chunk := await self.proc.stderr.read(4096):
            self._stderr_total += len(chunk)
            self._stderr_tail.extend(chunk)
            del self._stderr_tail[:-self.limits.max_stderr_bytes]
            if self._stderr_total > self.limits.max_stderr_total_bytes and not self._stderr_failure.done():
                self._stderr_failure.set_result(NativeMcpError("native stderr flood exceeded byte limit"))

    async def _guard_stderr(self, operation):
        task = asyncio.create_task(operation)
        try:
            done, _ = await asyncio.wait((task, self._stderr_failure), return_when=asyncio.FIRST_COMPLETED)
            if self._stderr_failure in done:
                raise self._stderr_failure.result()
            return await task
        finally:
            if not task.done():
                task.cancel()
            await asyncio.gather(task, return_exceptions=True)

    async def _write(self, frame):
        body = (json.dumps(frame, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
        if len(body) > self.limits.max_frame_bytes:
            raise NativeMcpError("native request exceeds frame byte limit")
        self.proc.stdin.write(body)
        await self.proc.stdin.drain()

    async def _exchange(self, method, params, request_id):
        await self._write({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            try:
                line = await self.proc.stdout.readline()
            except (ValueError, asyncio.LimitOverrunError) as error:
                raise NativeMcpError("native response exceeds frame byte limit") from error
            if not line:
                raise NativeMcpError("native EOF before response")
            self._last_frame = line[:self.limits.max_frame_bytes].decode("utf-8", errors="replace")
            if len(line) > self.limits.max_frame_bytes:
                raise NativeMcpError("native response exceeds frame byte limit")
            if not line.endswith(b"\n"):
                raise NativeMcpError("native EOF with partial JSON frame")
            try:
                response = json.loads(line.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise NativeMcpError("malformed native JSON frame") from error
            if not isinstance(response, dict) or response.get("jsonrpc") != "2.0":
                raise NativeMcpError("malformed native JSON-RPC envelope")
            if "id" not in response:
                if not isinstance(response.get("method"), str) or "result" in response or "error" in response:
                    raise NativeMcpError("malformed native notification")
                self._notifications += 1
                if self._notifications > self.limits.max_notifications:
                    raise NativeMcpError("native notifications exceeded limit")
                continue
            if type(response["id"]) is not int or response["id"] != request_id:
                raise NativeMcpError(f"wrong native response ID: expected {request_id}, got {response['id']!r}")
            if "method" in response or ("result" in response) == ("error" in response):
                raise NativeMcpError("malformed native response envelope")
            if "error" in response:
                raise NativeMcpError(f"native protocol error: {json.dumps(response['error'])[:2000]}")
            return response["result"]

    async def request(self, method: str, params: dict[str, Any]):
        if self.proc is None or self._failed:
            raise NativeMcpError("native transport is unavailable after failure")
        if self._outstanding:
            raise NativeMcpError("native test client allows one outstanding request")
        self._outstanding = True
        self.next_id += 1
        try:
            async with asyncio.timeout(self.limits.request_timeout):
                return await self._guard_stderr(self._exchange(method, params, self.next_id))
        except TimeoutError as error:
            self._failed = True
            raise NativeMcpError("native request timeout (stdin drain and response share one deadline)") from error
        except (BrokenPipeError, ConnectionError, OSError) as error:
            self._failed = True
            raise NativeMcpError(f"native pipe failure: {error}") from error
        except NativeMcpError:
            self._failed = True
            raise
        except asyncio.CancelledError:
            # A canceled exchange cannot safely share a later response ID.
            self._failed = True
            raise
        finally:
            self._outstanding = False

    async def initialize(self):
        result = await self.request("initialize", {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "magnesium-native-validation", "version": "0"},
        })
        if (not isinstance(result, dict) or result.get("protocolVersion") != "2025-11-25"
                or not isinstance(result.get("capabilities"), dict)
                or not isinstance(result.get("serverInfo"), dict)
                or not isinstance(result["serverInfo"].get("name"), str)
                or not isinstance(result["serverInfo"].get("version"), str)):
            raise NativeMcpError("malformed native initialization result")
        try:
            async with asyncio.timeout(self.limits.request_timeout):
                await self._guard_stderr(self._write({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        except TimeoutError as error:
            raise NativeMcpError("native initialized notification timeout") from error

    async def call(self, name: str, arguments: dict[str, Any]):
        result = await self.request("tools/call", {"name": name, "arguments": arguments})
        if not isinstance(result, dict):
            raise NativeMcpError("malformed native tool result")
        if result.get("isError") is True:
            raise NativeMcpError(f"native tool error: {json.dumps(result)[:2000]}")
        content = result.get("content")
        if not isinstance(content, list) or len(content) != 1 or not isinstance(content[0], dict) or content[0].get("type") != "text" or not isinstance(content[0].get("text"), str):
            raise NativeMcpError("malformed native tool text content")
        try:
            value = json.loads(content[0]["text"])
        except json.JSONDecodeError as error:
            raise NativeMcpError("malformed native tool JSON content") from error
        if not isinstance(value, dict):
            raise NativeMcpError("native tool payload must be an object")
        return value

    def diagnostics(self):
        return {"pid": self.proc.pid if self.proc else None,
                "returncode": self.proc.returncode if self.proc else None,
                "owner_pid": os.getpid(), "stderr_bytes": self._stderr_total,
                "stderr": bytes(self._stderr_tail).decode("utf-8", errors="replace"),
                "notifications": self._notifications, "last_frame": self._last_frame,
                "group_cleanup": self._group_cleanup}

    def _signal_owned(self, sig):
        try:
            if os.name == "posix":
                os.killpg(self.proc.pid, sig)
            elif sig == signal.SIGTERM:
                self.proc.terminate()
            else:
                self.proc.kill()
        except ProcessLookupError:
            pass
        except OSError as error:
            raise NativeMcpError(f"cannot signal owned native process group: {error}") from error

    def _owned_group_exists(self):
        # start_new_session assigned precisely this invocation's private PGID.
        # Never search by executable name or signal the caller's process group.
        try:
            os.killpg(self.proc.pid, 0)
            return True
        except ProcessLookupError:
            return False
        except OSError as error:
            raise NativeMcpError(f"cannot inspect owned native process group: {error}") from error

    async def _wait_owned_group(self):
        deadline = asyncio.get_running_loop().time() + self.limits.shutdown_timeout
        while self._owned_group_exists():
            remaining = deadline - asyncio.get_running_loop().time()
            if remaining <= 0:
                return False
            await asyncio.sleep(min(0.01, remaining))
        return True

    async def _cleanup_owned_group(self):
        if os.name != "posix":
            self._group_cleanup = "NOT_APPLICABLE (owned child only on this platform)"
            return
        # A clean leader exit does not establish that its descendants exited.
        # Redirected stdio makes proc.wait() complete while they remain alive.
        if self._owned_group_exists():
            self._signal_owned(signal.SIGTERM)
            if not await self._wait_owned_group():
                self._signal_owned(signal.SIGKILL)
                if not await self._wait_owned_group():
                    raise NativeMcpError("owned native process group remains after bounded shutdown; cleanup is incomplete")
        self._group_cleanup = "PASS"

    async def close(self):
        if self.proc is None:
            return
        if self._closed:
            if self._cleanup_error:
                raise NativeMcpError(self._cleanup_error)
            return
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
            try:
                await asyncio.wait_for(self.proc.wait(), self.limits.shutdown_timeout)
            except TimeoutError:
                self._signal_owned(signal.SIGTERM)
                try:
                    await asyncio.wait_for(self.proc.wait(), self.limits.shutdown_timeout)
                except TimeoutError:
                    self._signal_owned(getattr(signal, "SIGKILL", signal.SIGTERM))
                    await asyncio.wait_for(self.proc.wait(), self.limits.shutdown_timeout)
            await self._cleanup_owned_group()
        except BaseException as error:
            self._group_cleanup = "FAIL"
            self._cleanup_error = str(error)
            raise
        finally:
            # Once this ownership lifetime ended, never target a potentially
            # reused PGID during an accidental repeated close().
            self._closed = True
            if self._stderr_task:
                if not self._stderr_task.done():
                    self._stderr_task.cancel()
                await asyncio.gather(self._stderr_task, return_exceptions=True)
            if self._stderr_failure and not self._stderr_failure.done():
                self._stderr_failure.cancel()
