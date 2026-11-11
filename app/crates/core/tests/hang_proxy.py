#!/usr/bin/env python3
"""Stands between the direct engine and Magnesium's tool server, for tests.

While the file named by HANG_FLAG exists, the answer to the first
learning_step that records an answer is held back for HANG_SECONDS. The
request itself goes through at once, so the record commits and only its
answer is late: an outcome the engine cannot know. The flag is removed, so
the server that replaces this one answers at once.
"""

import json
import os
import subprocess
import sys
import threading
import time

FLAG = os.environ["HANG_FLAG"]
SECONDS = float(os.environ.get("HANG_SECONDS", "4"))
child = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
held = set()


def up():
    for line in child.stdout:
        try:
            value = json.loads(line)
        except Exception:
            value = {}
        if value.get("id") in held:
            time.sleep(SECONDS)
        sys.stdout.write(line)
        sys.stdout.flush()


threading.Thread(target=up, daemon=True).start()
for line in sys.stdin:
    try:
        value = json.loads(line)
        params = value.get("params") or {}
        arguments = params.get("arguments") or {}
        if (value.get("method") == "tools/call" and params.get("name") == "learning_step"
                and arguments.get("observations") and os.path.exists(FLAG)):
            os.remove(FLAG)
            held.add(value.get("id"))
    except Exception:
        pass
    child.stdin.write(line)
    child.stdin.flush()
child.stdin.close()
child.wait()
