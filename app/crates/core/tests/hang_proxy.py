#!/usr/bin/env python3
"""Stands between the direct engine and Magnesium's tool server, for tests.

While the file named by HANG_FLAG exists, the answer to the first
learning_step that records an answer is held back for HANG_SECONDS. The
request itself goes through at once, so the record commits and only its
answer is late: an outcome the engine cannot know. The flag may hold a
number: that many such answers are held back, one in each server, the
replays of the same request included. When none is left the flag is
removed, so the server that replaces this one answers at once.

With DELAY_EACH set, every tool call's answer is held back that many
seconds instead, as on a machine under load.
"""

import json
import os
import subprocess
import sys
import threading
import time

FLAG = os.environ.get("HANG_FLAG", "")
# The same, for a learning_step that changes practice items and records no
# answer, such as a repair.
PRACTICE_FLAG = os.environ.get("HANG_PRACTICE_FLAG", "")
DELAY_EACH = float(os.environ.get("DELAY_EACH", "0"))
SECONDS = float(os.environ.get("HANG_SECONDS", "4"))
child = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
held = set()
calls = set()


def up():
    for line in child.stdout:
        try:
            value = json.loads(line)
        except Exception:
            value = {}
        if value.get("id") in held:
            time.sleep(SECONDS)
        elif DELAY_EACH and value.get("id") in calls:
            time.sleep(DELAY_EACH)
        sys.stdout.write(line)
        sys.stdout.flush()


threading.Thread(target=up, daemon=True).start()
for line in sys.stdin:
    try:
        value = json.loads(line)
        params = value.get("params") or {}
        if value.get("method") == "tools/call":
            calls.add(value.get("id"))
        arguments = params.get("arguments") or {}
        step = value.get("method") == "tools/call" and params.get("name") == "learning_step"
        flag = None
        if step and arguments.get("observations") and FLAG and os.path.exists(FLAG):
            flag = FLAG
        elif (step and arguments.get("practice") and not arguments.get("observations")
              and PRACTICE_FLAG and os.path.exists(PRACTICE_FLAG)):
            flag = PRACTICE_FLAG
        if flag:
            with open(flag, encoding="utf-8") as handle:
                left = int(handle.read().strip() or "1") - 1
            if left > 0:
                with open(flag, "w", encoding="utf-8") as handle:
                    handle.write(str(left))
            else:
                os.remove(flag)
            held.add(value.get("id"))
    except Exception:
        pass
    child.stdin.write(line)
    child.stdin.flush()
child.stdin.close()
child.wait()
