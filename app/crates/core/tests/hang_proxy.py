#!/usr/bin/env python3

import json
import os
import subprocess
import sys
import threading
import time

FLAG = os.environ.get("HANG_FLAG", "")
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
