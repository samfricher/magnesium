#!/usr/bin/env python3
"""Fixed 12-document native/optional Docling fidelity gate.

Native status and exact physical-anchor inspection are independent of lexical
retrieval. Every document and failed response is retained in the result file.
Unavailable optional conversion is NOT_RUN, never a measured fidelity pass.
No installed assistant account, provider discovery, or model inference.
"""
from __future__ import annotations
import argparse
import asyncio
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
if str(REPO) not in sys.path:
    sys.path.insert(0, str(REPO))
from ci.isolated_environment import isolated_environment
from ci.native_mcp import NativeMcpClient, TransportLimits
from ci.source_preparation import call_once, prepare_source_fixture

BIN = os.path.abspath(os.environ.get("MAGNESIUM_TEST_BINARY", REPO / "target/debug/magnesium"))
WORKER = HERE / "worker.py"
SAMPLE = HERE / "sample"


@dataclass(frozen=True)
class GateConfig:
    preparation_seconds: float = 30.0
    preparation_attempts: int = 20
    request_seconds: float = 15.0

    def __post_init__(self):
        if min(self.preparation_seconds, self.preparation_attempts, self.request_seconds) <= 0:
            raise ValueError("gate limits must be positive")


def run_binary(env, *args, cwd=None):
    return subprocess.run([BIN, *args], cwd=cwd, env=env, capture_output=True, text=True, timeout=30)


def runtime_version(env, workspace):
    completed = run_binary(env, "--version", cwd=workspace)
    if completed.returncode != 0:
        raise RuntimeError(f"magnesium --version failed: {completed.stderr[-300:]}")
    numeric = r"(?:0|[1-9][0-9]*)"
    identifiers = r"[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*"
    match = re.fullmatch(rf"magnesium ({numeric}\.{numeric}\.{numeric}(?:-{identifiers})?(?:\+{identifiers})?)", completed.stdout.strip())
    if match is None or len(match[1]) > 128:
        raise RuntimeError("magnesium --version did not return a bounded semantic version")
    return match[1]


def native_client(env, workspace, config):
    version = runtime_version(env, workspace)
    return NativeMcpClient(
        [BIN, "_internal", "mcp-server", "--plugin-version", version,
         "--adapter-protocol-version", "2"], cwd=workspace, env=env,
        limits=TransportLimits(request_timeout=config.request_seconds),
    )


def sha_of(path):
    with open(path, "rb") as handle:
        digest = hashlib.sha256()
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def blocks_covering(blocks, kind, anchor):
    if kind == "pdf" and type(anchor.get("page")) is int:
        return [block for block in blocks if block.get("anchor_kind") == "page"
                and type(block.get("anchor_start")) is int and type(block.get("anchor_end")) is int
                and block["anchor_start"] <= anchor["page"] <= block["anchor_end"]]
    return [block for block in blocks if block.get("anchor_kind") == "section"]


def check_anchors(entry, blocks):
    """Exact quotations in their own anchored blocks; no page joining/normalization."""
    missing = []
    for anchor in entry.get("anchors", []):
        wanted = anchor.get("text")
        if not isinstance(wanted, str) or not wanted:
            missing.append(anchor)
        elif not any(wanted in block.get("text", "") for block in blocks_covering(blocks, entry.get("kind"), anchor)):
            missing.append(f"{wanted} (not on page {anchor['page']})" if entry.get("kind") == "pdf" and "page" in anchor else wanted)
    return missing


def source_reference(sample_path, entry, anchor=None):
    """Bind authored locations to fixture bytes; search candidates are not proof."""
    reference = {"path": entry["file"], "content_sha256": sha_of(sample_path)}
    if entry["native"]["support"] == "unsupported":
        return reference
    kind = "page" if entry["kind"] == "pdf" else "line"
    start = (anchor or {}).get("page" if kind == "page" else "line")
    end = (anchor or {}).get("end", start)
    if type(start) is not int or type(end) is not int or not 1 <= start <= end:
        raise ValueError("native anchors need an explicit 1-based physical range")
    if kind == "page" and start != end:
        raise ValueError("PDF fidelity inspects one physical page at a time")
    return {**reference, "anchor_kind": kind, "anchor_start": start, "anchor_end": end}


def validate_source(result, reference, action, expected_coverage):
    if not isinstance(result, dict) or result.get("schema_version") != 1 or result.get("action") != action or result.get("active") is not True:
        raise RuntimeError("malformed/inactive source response")
    file, anchor = result.get("file", {}), result.get("anchor", {})
    if file.get("path") != reference["path"] or file.get("content_sha256") != reference["content_sha256"]:
        raise RuntimeError("source response path/hash does not match the fixture")
    if (anchor.get("kind") != reference["anchor_kind"]
            or anchor.get("physical_start") != reference["anchor_start"]
            or anchor.get("physical_end") != reference["anchor_end"]):
        raise RuntimeError("source response has the wrong physical anchor")
    coverage = result.get("coverage", {})
    if not isinstance(coverage, dict) or coverage.get("status") != expected_coverage:
        raise RuntimeError(f"expected native {expected_coverage}, got {coverage}")
    extraction = result.get("extraction", {})
    if extraction.get("text_layer_only") is not True or not extraction.get("note") or not extraction.get("limits") or result.get("basis", {}).get("visual_analysis") is not False:
        raise RuntimeError("source response omits truthful native extraction limitations")
    if action == "status" and ("excerpt" in result or "original" in result):
        raise RuntimeError("byte-free source status fabricated excerpt/original")
    if expected_coverage == "text_unavailable" and (not coverage.get("missing_ranges") or result.get("excerpt", "").strip()):
        raise RuntimeError("unavailable source must report missing text without a readable excerpt")


def retrieval_eligible(hit, reference):
    return (isinstance(hit, dict) and hit.get("path") == reference["path"]
            and hit.get("content_sha256") == reference["content_sha256"]
            and hit.get("anchor_kind") == reference["anchor_kind"]
            and type(hit.get("anchor_start")) is int and type(hit.get("anchor_end")) is int
            and hit["anchor_start"] <= reference["anchor_start"] <= reference["anchor_end"] <= hit["anchor_end"])


async def native_baseline(env, workspace, manifest, *, config=None, client_factory=None, clock=time.monotonic):
    config, records, client = config or GateConfig(), [], None
    try:
        factory = client_factory or (lambda env, workspace: native_client(env, workspace, config))
        client = factory(env, workspace)
        async with client:
            for entry in manifest["documents"]:
                record = {"file": entry["file"], "kind": entry["kind"], "status": "FAIL",
                          "responses": [], "anchors": [], "retrieval": [], "errors": []}
                try:
                    expectation = entry["native"]
                    support = expectation["support"]
                    record["support"] = support
                    path = os.path.join(workspace, entry["file"])
                    deadline = clock() + config.preparation_seconds
                    async def call(name, args):
                        return await call_once(client,name,args,record,deadline=deadline,clock=clock)
                    if support == "unsupported":
                        reference = source_reference(path, entry)
                        status = await call("sources", {"action": "status", **reference})
                        if (status.get("schema_version") != 1 or status.get("action") != "status" or status.get("active") is not True
                                or status.get("file", {}).get("path") != entry["file"]
                                or status.get("coverage", {}).get("status") != "unsupported"
                                or not status.get("alternative") or "excerpt" in status or "original" in status):
                            raise RuntimeError("native unsupported format was not truthfully qualified")
                        record.update(status="NOT_RUN", qualification_status="PASS", coverage="unsupported",
                                      reason="Native text fidelity is unsupported for this format; no physical anchor was invented")
                        continue
                    if support not in ("readable", "text_unavailable"):
                        raise ValueError("unknown native support expectation")
                    anchors = entry.get("anchors", [])
                    if support == "readable" and not anchors:
                        raise ValueError("readable native sample has no independently authored anchors")
                    first = anchors[0] if anchors else expectation["anchor"]
                    reference = source_reference(path, entry, first)
                    # Genuine scoped preparation precedes status; its lexical
                    # matches do not contribute to either fidelity oracle.
                    await prepare_source_fixture(client, workspace, entry["file"], "pdf" if entry["kind"] == "pdf" else "text",
                                                  reference["content_sha256"], record, require_hit=support == "readable",
                                                  deadline=deadline, attempts=config.preparation_attempts, clock=clock)
                    status = await call("sources", {"action": "status", **reference})
                    validate_source(status, reference, "status", expectation["coverage"])
                    record["coverage"] = status["coverage"]["status"]
                    if support == "text_unavailable":
                        query = expectation.get("query", "unreadable scanned glyphs")
                        searched = await call("search_sources", {"query": query, "path_prefix": entry["file"], "limit": 50})
                        if not isinstance(searched.get("hits"), list):
                            raise RuntimeError("malformed search response")
                        hits = [hit for hit in searched["hits"] if hit.get("path") == entry["file"]]
                        record["retrieval"].append({"query": query, "hits": len(hits), "status": "PASS" if not hits else "FAIL"})
                        if hits:
                            record["errors"].append("unavailable text produced fabricated lexical hits")
                    else:
                        for anchor in anchors:
                            reference = source_reference(path, entry, anchor)
                            inspected = await call("sources", {"action": "inspect", **reference})
                            anchor_record = {"reference": reference, "text": anchor.get("text"), "status": "FAIL"}
                            try:
                                validate_source(inspected, reference, "inspect", expectation["coverage"])
                                wanted = anchor.get("text")
                                if not isinstance(wanted, str) or not wanted or wanted not in inspected.get("excerpt", ""):
                                    raise RuntimeError(f"exact quotation missing at physical anchor: {wanted!r}")
                                anchor_record["status"] = "PASS"
                            except (RuntimeError, TypeError) as error:
                                anchor_record["error"] = str(error)
                                record["errors"].append(str(error))
                            finally:
                                record["anchors"].append(anchor_record)
                            query = " ".join(str(anchor.get("text", "")).split()[:12])
                            searched = await call("search_sources", {"query": query, "path_prefix": entry["file"], "limit": 50})
                            if not isinstance(searched.get("hits"), list):
                                raise RuntimeError("malformed search response")
                            eligible = [hit for hit in searched["hits"] if retrieval_eligible(hit, reference)]
                            record["retrieval"].append({"query": query, "reference": reference, "hits": len(eligible), "status": "PASS" if eligible else "FAIL"})
                            if not eligible:
                                record["errors"].append("quotation retrieval has no eligible path/hash/physical-anchor candidate")
                    record["status"] = "FAIL" if record["errors"] else "PASS"
                except Exception as error:
                    record["errors"].append(str(error))
                    record["transport"] = client.diagnostics()
                finally:
                    records.append(record)
    except Exception as error:
        for entry in manifest["documents"][len(records):]:
            records.append({"file": entry["file"], "kind": entry["kind"], "status": "FAIL",
                            "errors": [f"native initialization/cleanup failed: {error}"],
                            "responses": [], "transport": client.diagnostics() if client else {}})
        # A failed child lifetime cannot qualify an otherwise passing run.
        if records:
            records[-1]["status"] = "FAIL"
            records[-1].setdefault("errors", []).append(f"native lifetime failed: {error}")
    return records


def try_docling():
    started = time.monotonic()
    try:
        import docling  # noqa: F401
    except ImportError as error:
        return None, f"docling is not installed ({error})", 0.0
    from importlib import metadata
    versions = {}
    for name in ("docling", "docling-slim", "docling-core"):
        try:
            versions[name] = metadata.version(name)
        except metadata.PackageNotFoundError:
            versions[name] = "unknown"
    return versions, "", time.monotonic() - started


def package_size_mb():
    import importlib.util
    total = 0
    for name in ("docling", "docling_slim", "docling_core"):
        spec = importlib.util.find_spec(name)
        if spec is None or not spec.origin:
            return None
        for dirpath, _, files in os.walk(os.path.dirname(spec.origin)):
            total += sum(os.path.getsize(os.path.join(dirpath, name)) for name in files)
    return round(total / (1024 * 1024), 1)


def peak_rss_mb():
    divider = 1024 * 1024 if sys.platform == "darwin" else 1024
    return resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / divider


def worker_convert(workspace_file, sha, kind):
    body = json.dumps({"version": 1, "source_kind": kind, "source_sha256": sha,
                       "snapshot_path": str(workspace_file), "anchor_start": 1, "anchor_end": 10}).encode()
    started = time.monotonic()
    completed = subprocess.run([sys.executable, str(WORKER)], input=b"MAGNESIUM_DOCS_V1 %d\n" % len(body) + body,
                               capture_output=True, timeout=300)
    elapsed = time.monotonic() - started
    if completed.returncode != 0:
        return None, elapsed, completed.stderr.decode(errors="replace")[-2000:]
    marker, rest = completed.stdout.split(b"\n", 1)
    name, length = marker.decode("ascii").split(" ", 1)
    if name != "MAGNESIUM_DOCS_V1" or int(length) != len(rest) or len(rest) > 4 * 1024 * 1024:
        raise RuntimeError("worker returned an invalid/big frame")
    result = json.loads(rest.decode())
    if not isinstance(result, dict):
        raise RuntimeError("worker returned a non-object payload")
    return result, elapsed, ""


async def evaluate_sample(sample, manifest, config):
    results = {"status": "FAIL", "documents": [], "worker": {"status": "NOT_RUN", "reason": "native workspace was not initialized"}, "budgets": {}, "failures": []}
    baselines, versions, reason = {}, None, "native workspace was not initialized"
    with tempfile.TemporaryDirectory(prefix="magnesium-docling-gate-") as workdir:
        workspace = os.path.join(workdir, "workspace")
        os.makedirs(workspace)
        try:
            env = isolated_environment(workdir)
            init = run_binary(env, "init", workspace)
            if init.returncode != 0:
                raise RuntimeError(f"magnesium init failed: {init.stderr[-500:]}")
            for entry in manifest["documents"]:
                shutil.copy(os.path.join(sample, entry["file"]), os.path.join(workspace, entry["file"]))
            native_started = time.monotonic()
            baselines = {record["file"]: record for record in await native_baseline(env, workspace, manifest, config=config)}
            results["budgets"]["native_sample_seconds"] = round(time.monotonic() - native_started, 2)
            versions, reason, seconds = try_docling()
            results["budgets"]["worker_import_seconds"] = round(seconds, 2)
            results["budgets"]["worker_package_mb"] = package_size_mb()
            results["worker"] = ({"status": "NOT_RUN", "reason": reason} if versions is None else {"status": "PASS", "versions": versions, "per_document": {}})
        except Exception as error:
            results["failures"].append(str(error))
            reason = str(error)

        for entry in manifest["documents"]:
            name, kind = entry["file"], entry["kind"]
            record = {"file": name, "status": "FAIL", "native": baselines.get(name, {"status": "FAIL", "errors": [reason]}),
                      "worker": {"status": "NOT_RUN", "reason": reason}}
            try:
                native = record["native"]
                native_ok = native.get("status") == "PASS" or (native.get("status") == "NOT_RUN" and native.get("qualification_status") == "PASS")
                if not native_ok:
                    results["failures"].append(f"[{name}] native failed: {native.get('errors')}")
                worker_ok = True
                if versions is not None:
                    path = os.path.join(workspace, name)
                    converted, elapsed, error = worker_convert(path, sha_of(path), kind)
                    results["worker"]["per_document"][name] = round(elapsed, 2)
                    if converted is None:
                        raise RuntimeError(f"worker failed: {error}")
                    missing = check_anchors(entry, converted.get("blocks", []))
                    coverage = converted.get("coverage")
                    if entry["native"]["support"] == "text_unavailable" and coverage != "text_unavailable":
                        wanted, blocks = entry.get("ocr_text"), converted.get("blocks", [])
                        if not wanted or not any(wanted in block.get("text", "") for block in blocks_covering(blocks, kind, entry["native"]["anchor"])):
                            missing.append("independently authored scan quotation absent at physical page")
                    worker_ok = not missing and coverage in ("complete", "partial", "text_unavailable")
                    record["worker"] = {"status": "PASS" if worker_ok else "FAIL", "coverage": coverage,
                                        "blocks": len(converted.get("blocks", [])), "warnings": converted.get("warnings", []),
                                        "missing_anchors": missing, "method_version": converted.get("method_version"),
                                        "response": converted}
                    if not worker_ok:
                        results["failures"].append(f"[{name}] worker fidelity failed: {record['worker']}")
                        results["worker"]["status"] = "FAIL"
                record["status"] = "PASS" if native_ok and worker_ok else "FAIL"
            except Exception as error:
                record["worker"] = {"status": "FAIL", "error": str(error)}
                results["worker"]["status"] = "FAIL"
                results["failures"].append(f"[{name}] {error}")
            finally:
                results["documents"].append(record)
        results["budgets"]["child_process_peak_rss_mb"] = round(peak_rss_mb(), 1)
        results["budgets"]["measurement_scope"] = "cumulative high-water mark of native and optional worker children; not worker-only RSS or live-model evidence"
        results["budgets"]["worker_resources"] = ({"status": "NOT_RUN", "reason": reason} if versions is None else {"status": "PASS", "note": "per-document conversion time and package bytes; child RSS is aggregate"})
    results["status"] = "FAIL" if results["failures"] else "PASS"
    return results


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sample", default=str(SAMPLE))
    parser.add_argument("--out", default="docling-gate-results.json", help="machine-readable output, written on failure too")
    parser.add_argument("--preparation-seconds", type=float, default=30)
    parser.add_argument("--preparation-attempts", type=int, default=20)
    parser.add_argument("--request-seconds", type=float, default=15)
    args = parser.parse_args(argv)
    results = {"status": "FAIL", "documents": [], "worker": {"status": "NOT_RUN"}, "failures": []}
    try:
        config = GateConfig(args.preparation_seconds, args.preparation_attempts, args.request_seconds)
        with open(os.path.join(args.sample, "manifest.json")) as handle:
            manifest = json.load(handle)
        documents = manifest["documents"]
        results["documents"] = [{"file": entry.get("file", f"unknown-{index}"), "status": "FAIL",
                                  "native": {"status": "FAIL", "errors": ["sample validation was not completed"]},
                                  "worker": {"status": "NOT_RUN", "reason": "sample validation was not completed"}}
                                 for index, entry in enumerate(documents)]
        if len(documents) != 12 or len({entry["file"] for entry in documents}) != 12:
            raise ValueError("the gate requires the fixed 12 distinct documents")
        for entry in documents:
            path = entry["file"]
            if os.path.isabs(path) or any(part in ("", ".", "..") for part in path.replace("\\", "/").split("/")):
                raise ValueError("sample filenames must be contained relative paths")
            if entry["native"]["support"] not in ("readable", "text_unavailable", "unsupported"):
                raise ValueError("unknown native support expectation")
        results = asyncio.run(evaluate_sample(args.sample, manifest, config))
    except Exception as error:
        results["failures"].append(str(error))
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w") as handle:
        json.dump(results, handle, indent=2)
        handle.write("\n")
    if results["status"] != "PASS":
        print("\n".join(results["failures"]))
        print(f"gate FAIL; results written to {args.out}")
        return 1
    worker = results["worker"]
    print(f"native gate PASS (12 documents); worker {worker['status']}" + (f": {worker['reason']}" if worker.get("reason") else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
