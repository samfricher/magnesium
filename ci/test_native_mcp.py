"""Adversarial real stdio children exercise bounded native test transport."""
import asyncio
import ctypes
import errno
import os
import signal
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from ci.isolated_environment import isolated_environment
from ci.native_mcp import NativeMcpClient, NativeMcpError, TransportLimits

CHILD = r'''
import json, os, sys, time
mode = sys.argv[1]
for line in sys.stdin.buffer:
    request = json.loads(line)
    if "id" not in request:
        continue
    if mode == "partial":
        sys.stdout.write('{"jsonrpc":'); sys.stdout.flush(); sys.exit(0)
    elif mode == "timeout":
        time.sleep(60)
    elif mode == "eof":
        sys.exit(0)
    elif mode == "malformed":
        print("not json", flush=True)
    elif mode == "oversized":
        print("x" * 8192, flush=True)
    elif mode == "stderr":
        os.write(2, b"diagnostic " * 20000); time.sleep(60)
    elif mode == "notifications":
        for i in range(20):
            print(json.dumps({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": i}}), flush=True)
    else:
        answer = {"jsonrpc": "2.0", "id": request["id"] + (1 if mode == "wrong-id" else 0), "result": {"protocolVersion": "2025-11-25", "capabilities": {}, "serverInfo": {"name": "fixture", "version": "0"}}}
        if mode == "initialize-error":
            del answer["result"]; answer["error"] = {"code": -32602, "message": "init rejected"}
        elif request["method"] == "tools/call":
            answer["result"] = {"content": [{"type": "text", "text": '{"answer": 42}'}]}
        print(json.dumps(answer), flush=True)
'''

PUBLISH_DESCENDANT_PID = r'''
with open(pid_path + ".tmp", "w") as pid_file:
    pid_file.write(str(descendant_pid))
os.replace(pid_path + ".tmp", pid_path)
'''


class NativeMcpTests(unittest.IsolatedAsyncioTestCase):
    def client(self, root, mode):
        return NativeMcpClient([sys.executable, "-u", "-c", CHILD, mode], cwd=root,
                               env=isolated_environment(Path(root, "private")),
                               limits=TransportLimits(request_timeout=0.15, shutdown_timeout=0.1,
                                                      max_frame_bytes=4096, max_notifications=4,
                                                      max_stderr_bytes=1024, max_stderr_total_bytes=8192))

    async def test_success_context_initializes_and_reaps(self):
        with tempfile.TemporaryDirectory() as root:
            client = self.client(root, "good")
            async with client:
                self.assertEqual(await client.call("fixture", {}), {"answer": 42})
            self.assertEqual(client.proc.returncode, 0)
            self.assertEqual(client.diagnostics()["owner_pid"], os.getpid())

    async def test_initialization_and_framing_failures_are_bounded_and_reaped(self):
        for mode, expected in [("partial", "partial JSON frame"), ("timeout", "timeout"), ("eof", "EOF"),
                               ("malformed", "JSON"), ("wrong-id", "response ID"),
                               ("oversized", "frame"), ("stderr", "stderr"),
                               ("notifications", "notifications"), ("initialize-error", "init rejected")]:
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as root:
                client = self.client(root, mode)
                started = time.monotonic()
                with self.assertRaisesRegex(NativeMcpError, expected):
                    async with client:
                        self.fail("bad child initialized")
                self.assertLess(time.monotonic() - started, 2)
                self.assertIsNotNone(client.proc.returncode)
                self.assertLessEqual(len(client.diagnostics()["stderr"].encode()), 1024)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux subreaper regression")
    async def test_clean_leader_exit_terminates_and_reaps_owned_descendant(self):
        # The outer test owns orphan reaping. Production cleanup must terminate
        # the private group, even after its leader has exited with closed pipes.
        # The child publishes the descendant pid by rename, so the poll below
        # never reads a partially written pid file.
        libc = ctypes.CDLL(None, use_errno=True)
        old = ctypes.c_int()
        self.assertEqual(libc.prctl(37, ctypes.byref(old), 0, 0, 0), 0)  # GET_CHILD_SUBREAPER
        self.assertEqual(libc.prctl(36, 1, 0, 0, 0), 0)  # SET_CHILD_SUBREAPER
        reaped = []
        client = None
        try:
            with tempfile.TemporaryDirectory() as root:
                orphan = CHILD + "\nimport subprocess, signal\np = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, preexec_fn=lambda: signal.signal(signal.SIGTERM, signal.SIG_IGN))\ndescendant_pid = p.pid\npid_path = 'descendant.pid'\n" + PUBLISH_DESCENDANT_PID
                client = NativeMcpClient([sys.executable, "-u", "-c", orphan, "good"], cwd=root,
                                         env=isolated_environment(Path(root, "private")),
                                         limits=TransportLimits(request_timeout=1, shutdown_timeout=0.15))
                async def reap_orphan():
                    deadline = asyncio.get_running_loop().time() + 2
                    while asyncio.get_running_loop().time() < deadline:
                        pid_file = Path(root, "descendant.pid")
                        if pid_file.exists():
                            pid = int(pid_file.read_text())
                            try:
                                waited, status = os.waitpid(pid, os.WNOHANG)
                            except ChildProcessError:
                                waited = 0  # Leader has not exited/reparented yet.
                            if waited:
                                reaped.append((waited, status))
                                return
                        await asyncio.sleep(0.005)
                reaper = asyncio.create_task(reap_orphan())
                try:
                    real_killpg = os.killpg
                    with patch("ci.native_mcp.os.killpg", wraps=real_killpg) as signals:
                        async with client:
                            pass
                    self.assertTrue(all(call.args[0] == client.proc.pid for call in signals.call_args_list))
                    self.assertTrue(any(call.args[1] == signal.SIGKILL for call in signals.call_args_list))
                    with patch("ci.native_mcp.os.killpg", side_effect=AssertionError("repeated close must not signal a reused group")):
                        await client.close()
                    await reaper
                    self.assertEqual(client.proc.returncode, 0)
                    self.assertEqual(len(reaped), 1, "clean leader left its owned descendant alive")
                    self.assertTrue(os.WIFSIGNALED(reaped[0][1]))
                    self.assertEqual(os.WTERMSIG(reaped[0][1]), signal.SIGKILL)
                finally:
                    # Reproduction cleanup is confined to this invocation's
                    # private process group and runs even against the old bug.
                    try:
                        os.killpg(client.proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    await reaper
                    pid_file = Path(root, "descendant.pid")
                    if pid_file.exists() and not reaped:
                        os.waitpid(int(pid_file.read_text()), 0)
        finally:
            self.assertEqual(libc.prctl(36, old.value, 0, 0, 0), 0)

    @unittest.skipUnless(os.name == "posix", "POSIX process-group permission regression")
    async def test_group_permission_denial_is_not_successful_cleanup(self):
        with tempfile.TemporaryDirectory() as root:
            client = self.client(root, "good")
            await client.start()
            await client.initialize()
            with patch("ci.native_mcp.os.killpg", side_effect=PermissionError(errno.EPERM, "Operation not permitted")):
                with self.assertRaisesRegex(NativeMcpError, "process group"):
                    await client.close()
            self.assertIsNotNone(client.proc.returncode)

    async def test_request_timeout_includes_stdin_drain(self):
        with tempfile.TemporaryDirectory() as root:
            client = NativeMcpClient([sys.executable, "-c", "import time; time.sleep(60)"], cwd=root,
                                    env=isolated_environment(Path(root, "private")),
                                    limits=TransportLimits(request_timeout=0.1, shutdown_timeout=0.1, max_frame_bytes=2 * 1024 * 1024))
            await client.start()
            started = time.monotonic()
            try:
                with self.assertRaisesRegex(NativeMcpError, "timeout"):
                    await client.request("input", {"text": "x" * (1024 * 1024)})
            finally:
                await client.close()
            self.assertLess(time.monotonic() - started, 2)
            self.assertIsNotNone(client.proc.returncode)


if __name__ == "__main__":
    unittest.main()
