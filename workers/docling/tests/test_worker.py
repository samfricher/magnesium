#!/usr/bin/env python3
"""Worker self-tests (LEARN-208): execute `worker.py` for real with a
stubbed `docling` module, covering framing, validation, mapping, and
error paths. Only Docling's own conversion quality stays unverified
here (needs the installed stack + models); everything the worker owns
is executed. No model inference; loopback-free local subprocesses.
"""

import base64
import hashlib
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
WORKER = os.path.join(HERE, "..", "worker.py")
STUB = os.path.join(HERE, "stub_docling")


def run_worker(payload_or_raw, files=None, extra_env=None):
    """Run worker.py with a framed request; return (exit, framed_json_or_None, stderr).

    Accepts a request dict (framed here) or raw bytes (sent verbatim for
    malformed-frame cases).
    """
    if isinstance(payload_or_raw, (bytes, bytearray)):
        frame = bytes(payload_or_raw)
    else:
        body = json.dumps(payload_or_raw).encode()
        frame = b"MAGNESIUM_DOCS_V1 %d\n" % len(body) + body
    env = dict(os.environ)
    env["PYTHONPATH"] = STUB + os.pathsep + env.get("PYTHONPATH", "")
    env.update(extra_env or {})
    completed = subprocess.run(
        [sys.executable, WORKER],
        input=frame,
        env=env,
        capture_output=True,
        timeout=120,
    )
    stderr = completed.stderr.decode(errors="replace")
    if completed.returncode != 0:
        return completed.returncode, None, stderr
    out = completed.stdout
    try:
        marker, rest = out.split(b"\n", 1)
        name, length = marker.decode("ascii").split(" ", 1)
        assert name == "MAGNESIUM_DOCS_V1"
        assert int(length) == len(rest), (int(length), len(rest))
        return 0, json.loads(rest.decode()), stderr
    except (ValueError, AssertionError, IndexError) as error:
        return completed.returncode, None, f"bad frame: {error} {stderr[-200:]}"


def snapshot_file(workdir, name=b"snapshot-bytes"):
    path = os.path.join(workdir, "snapshot.pdf")
    with open(path, "wb") as handle:
        handle.write(name)
    digest = hashlib.sha256(name).hexdigest()
    return path, digest


def base_request(path, digest, start=1, end=2):
    return {
        "version": 1,
        "source_kind": "pdf",
        "source_sha256": digest,
        "snapshot_path": path,
        "anchor_start": start,
        "anchor_end": end,
    }


FAILURES = []


def check(name, condition, detail=""):
    print(("PASS " if condition else "FAIL ") + name)
    if not condition:
        FAILURES.append(f"[{name}] {detail}"[:500])


def main():
    workdir = tempfile.mkdtemp(prefix="magnesium-docs-worker-test-")
    try:
        path, digest = snapshot_file(workdir)

        code, response, stderr = run_worker(base_request(path, digest))
        check("round-trip exit", code == 0, stderr)
        check("method", response and response.get("method") == "docling", response)
        check(
            "method_version names stub stack",
            response and "unknown" in response.get("method_version", ""),
            response,
        )
        blocks = (response or {}).get("blocks", [])
        check("four blocks", len(blocks) == 4, response)
        check(
            "page anchors preserved",
            [(b.get("anchor_kind"), b.get("anchor_start"), b.get("anchor_end")) for b in blocks]
            == [("page", 1, 1), ("page", 1, 1), ("page", 2, 2), ("page", 2, 2)],
            response,
        )
        warnings = [b.get("warning") for b in blocks]
        check("table warned", bool(warnings[2]), response)
        check("formula warned", bool(warnings[3]), response)
        check("no warning on prose", not warnings[0] and not warnings[1], response)
        check(
            "coverage complete",
            (response or {}).get("coverage") == "complete",
            response,
        )

        code, response, _ = run_worker(
            base_request(path, digest), extra_env={"MAGNESIUM_STUB_MODE": "partial"}
        )
        check("partial coverage", code == 0 and response.get("coverage") == "partial", response)

        code, response, _ = run_worker(
            base_request(path, digest), extra_env={"MAGNESIUM_STUB_MODE": "raise"}
        )
        check("conversion exception fails bounded", code != 0 and response is None, (code, response))

        code, response, _ = run_worker(
            base_request(path, digest), extra_env={"MAGNESIUM_STUB_MODE": "empty"}
        )
        check(
            "empty document is text_unavailable",
            code == 0 and response.get("coverage") == "text_unavailable",
            response,
        )

        many = [
            {"label": "text", "text": f"block {i}", "pages": [1]}
            for i in range(513)
        ]
        code, response, _ = run_worker(
            base_request(path, digest),
            extra_env={"MAGNESIUM_STUB_ITEMS": json.dumps(many)},
        )
        check(
            "513 blocks truncate to partial, never complete",
            code == 0
            and len(response.get("blocks", [])) == 512
            and response.get("coverage") == "partial"
            and bool(response.get("missing")),
            {"blocks": len((response or {}).get("blocks", [])), "coverage": (response or {}).get("coverage")},
        )

        bad = base_request(path, "0" * 64)
        code, response, stderr = run_worker(bad)
        check("sha mismatch exits nonzero", code != 0 and response is None, (code, stderr[-100:]))

        bad_kind = base_request(path, digest)
        bad_kind["source_kind"] = "txt"
        code, response, _ = run_worker(bad_kind)
        check("unsupported kind exits nonzero", code != 0 and response is None, (code, response))

        code, response, stderr = run_worker(b"MAGNESIUM_DOCS_V1 nope\n{}")
        check("malformed frame exits nonzero", code != 0 and response is None, (code, stderr[-100:]))

        code, response, stderr = run_worker(base_request(path, digest, start=0, end=1))
        check("bad range exits nonzero", code != 0 and response is None, (code, stderr[-100:]))

        link = os.path.join(workdir, "link.pdf")
        os.symlink(path, link)
        code, response, _ = run_worker(base_request(link, digest))
        check("symlink snapshot refused", code != 0 and response is None, (code, response))

        big = os.path.join(workdir, "big.pdf")
        with open(big, "wb") as handle:
            handle.seek(64 * 1024 * 1024)
            handle.write(b"x")
        big_digest = hashlib.sha256(open(big, "rb").read()).hexdigest()
        code, response, _ = run_worker(base_request(big, big_digest))
        check("oversized snapshot refused", code != 0 and response is None, (code, response))
    finally:
        import shutil

        shutil.rmtree(workdir, ignore_errors=True)
    if FAILURES:
        print("\n".join(FAILURES))
        return 1
    print("worker self-tests passed (stubbed docling; conversion quality NOT_RUN)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
