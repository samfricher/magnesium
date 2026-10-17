#!/usr/bin/env python3
"""Inexpensive actual-core readable/blank PDF smoke with private source policy."""
from __future__ import annotations
import argparse
import asyncio
import json
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "workers/docling"))
import gate
from ci.isolated_environment import isolated_environment
from ci.source_fixtures import smoke_manifest, write_pdf


async def smoke():
    result = {"status": "FAIL", "documents": [], "policy": {}, "failures": []}
    with tempfile.TemporaryDirectory(prefix="magnesium-native-source-smoke-") as temporary:
        workspace = Path(temporary, "workspace")
        workspace.mkdir()
        env = isolated_environment(Path(temporary, "private"))
        try:
            init = gate.run_binary(env, "init", str(workspace))
            if init.returncode:
                raise RuntimeError(f"native init failed: {init.stderr[-1000:]}")
            write_pdf(workspace / "readable.pdf")
            write_pdf(workspace / "blank.pdf", image_only=True)
            write_pdf(workspace / "excluded.pdf")
            Path(workspace, ".magnesium/magnesiumignore").write_text("excluded.pdf\n")
            manifest = smoke_manifest()
            result["documents"] = await gate.native_baseline(env, str(workspace), manifest)
            for document in result["documents"]:
                if document["status"] != "PASS":
                    result["failures"].append(f"[{document['file']}] {document.get('errors')}")
            reference = {"path": "excluded.pdf", "content_sha256": gate.sha_of(workspace / "excluded.pdf"),
                         "anchor_kind": "page", "anchor_start": 1, "anchor_end": 1}
            async with gate.native_client(env, str(workspace), gate.GateConfig()) as client:
                excluded = await client.call("sources", {"action": "inspect", **reference})
                result["policy"]["response"] = excluded
                if excluded.get("coverage", {}).get("status") != "excluded" or "excerpt" in excluded or "original" in excluded:
                    raise RuntimeError("source exclusion leaked bytes or failed to report excluded")
            # Allowlist-derived environment never inherits provider state or
            # source-worker opt-ins, even when invoking shell has them set.
            forbidden = ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "MAGNESIUM_DOCS_WORKER", "MAGNESIUM_DOCS_WORKER_OPT_IN", "MAGNESIUM_ALLOW_HOST_DISCOVERY")
            if any(name in env for name in forbidden) or Path(env["HOME"]).resolve() == Path.home().resolve():
                raise RuntimeError("native smoke did not use its private credential-free environment")
            result["policy"]["status"] = "PASS"
            result["policy"]["private_environment"] = True
        except Exception as error:
            result["failures"].append(str(error))
    result["status"] = "FAIL" if result["failures"] else "PASS"
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=str(ROOT / "target/validation/native-source-smoke.json"))
    args = parser.parse_args(argv)
    result = asyncio.run(smoke())
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Native readable/blank PDF and exclusion smoke {result['status']}; results: {out}")
    if result["failures"]:
        print("\n".join(result["failures"]))
    return int(result["status"] != "PASS")


if __name__ == "__main__":
    raise SystemExit(main())
