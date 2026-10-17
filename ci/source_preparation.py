"""Strict test-only source setup. Semantic and permission oracles never retry."""
from __future__ import annotations
from contextlib import closing
import asyncio
import hashlib
import json
from pathlib import Path
import sqlite3
import time


def read_state(workspace, path):
    index = Path(workspace, ".magnesium/index/source-search.sqlite").resolve()
    with closing(sqlite3.connect(index.as_uri() + "?mode=ro", uri=True)) as connection:
        connection.row_factory = sqlite3.Row
        row = connection.execute("SELECT display_path AS path, content_sha256, source_kind, index_status, present, exclusion_reason, last_error FROM source_manifest WHERE display_path=?", (path,)).fetchone()
        return dict(row) if row is not None else None


def validate_packet(packet):
    if (not isinstance(packet, dict) or not isinstance(packet.get("hits"), list)
            or packet.get("coverage") not in ("complete", "partial", "stale", "indexing_on_demand")
            or packet.get("reconciliation") not in (None, "deferred_budget", "deferred_concurrent_update")
            or (packet.get("reconciliation") is not None and packet["coverage"] not in ("partial", "stale"))):
        raise RuntimeError("malformed source preparation coordinator response")


def validate_manifest(state, path, kind, expected_hash):
    if state is not None and (state.get("path") != path or state.get("content_sha256") != expected_hash
            or state.get("source_kind") != kind or state.get("present") != 1
            or state.get("exclusion_reason") is not None or state.get("last_error") is not None
            or state.get("index_status") not in ("indexed", "pending")):
        raise RuntimeError(f"source fixture changed identity, policy, presence or extraction error: {state}")


def validate_hits(packet,path,kind,expected_hash):
    anchor_kind="page" if kind=="pdf" else "line"
    for hit in packet["hits"]:
        if (not isinstance(hit, dict) or hit.get("path") != path or hit.get("content_sha256") != expected_hash
                or hit.get("anchor_kind") != anchor_kind or hit.get("authority") != "original_source"
                or type(hit.get("anchor_start")) is not int or type(hit.get("anchor_end")) is not int
                or not 1 <= hit["anchor_start"] <= hit["anchor_end"]):
            raise RuntimeError("source preparation hit changed identity or authority")

def fixture_ready(packet, state, status, path, kind, expected_hash, *, advanced=False, require_hit=True):
    validate_packet(packet)
    anchor_kind = "page" if kind == "pdf" else "line"
    validate_hits(packet,path,kind,expected_hash)
    if (not isinstance(status, dict) or status.get("schema_version") != 1 or status.get("active") is not True
            or status.get("action") != "status" or status.get("file", {}).get("path") != path
            or status.get("anchor", {}).get("kind") != anchor_kind
            or status["anchor"].get("physical_start") != 1 or status["anchor"].get("physical_end") != 1
            or "excerpt" in status or "original" in status):
        raise RuntimeError("malformed source preparation policy status")
    progress = (packet["coverage"] == "indexing_on_demand" or packet.get("reconciliation") is not None)
    if state is None:
        if status.get("coverage", {}).get("status") == "unknown" and not packet["hits"] and packet.get("reconciliation") is not None:
            return False
        raise RuntimeError("undiscovered fixture lacks explicit deferred discovery and current policy status")
    if (state.get("path") != path or state.get("content_sha256") != expected_hash or state.get("source_kind") != kind
            or state.get("present") != 1 or state.get("exclusion_reason") is not None or state.get("last_error") is not None
            or status.get("file", {}).get("content_sha256") != expected_hash or status["file"].get("source_kind") != kind
            or status.get("coverage", {}).get("status") not in (("complete", "partial") if require_hit else ("text_unavailable", "partial"))
            or status["coverage"].get("last_error") is not None):
        raise RuntimeError("source preparation changed identity, policy, presence or extraction error")
    if state["index_status"] == "indexed" and (packet["hits"] if require_hit else status["coverage"]["status"] == "text_unavailable"):
        return True
    if not packet["hits"] and progress and (state["index_status"] == "pending" or (state["index_status"] == "indexed" and advanced)):
        return False
    raise RuntimeError("source fixture is not indexed or explicitly preparing")


async def call_once(client,name,arguments,record,*,deadline,clock=time.monotonic):
    """A final semantic/content/policy oracle gets one bounded recorded call."""
    observation={"tool":name,"arguments":dict(arguments)}
    try:
        remaining=deadline-clock()
        if remaining<=0:
            raise RuntimeError("incomplete preparation: shared deadline exhausted")
        async with asyncio.timeout(remaining):
            result=await client.call(name,arguments)
        observation["result"]=result
        if not isinstance(result,dict) or result.get("error") is not None:
            raise RuntimeError("malformed/terminal native tool response")
        if name=="search_sources": validate_packet(result)
        if clock()>=deadline:
            raise RuntimeError("incomplete preparation: shared deadline exhausted during request")
        return result
    except Exception as error:
        observation["error"]=str(error)
        raise
    finally:
        record.setdefault("responses",[]).append(observation)


async def prepare_source_fixture(client, workspace, path, kind, expected_hash, record, *, seconds=5, attempts=32, clock=time.monotonic, deadline=None, require_hit=True):
    """One document deadline/cap; preserve every request and read-only state."""
    deadline = clock() + seconds if deadline is None else deadline
    anchor_kind = "page" if kind == "pdf" else "line"
    async def call(name,arguments):
        return await call_once(client,name,arguments,record,deadline=deadline,clock=clock)
    try:
        for _ in range(attempts):
            searched = await call("search_sources", {"query": path, "path_prefix": path, "limit": 50})
            # Reject wrong hits and malformed state before another API request.
            validate_packet(searched)
            validate_hits(searched,path,kind,expected_hash)
            before = read_state(workspace, path)
            observation = {"before_status":before}
            record.setdefault("preparation_states", []).append(observation)
            validate_manifest(before,path,kind,expected_hash)
            status = await call("sources", {"action": "status", "path": path, "anchor_kind": anchor_kind, "anchor_start": 1, "anchor_end": 1})
            state = read_state(workspace, path)
            target, root = Path(workspace, path), Path(workspace).resolve()
            if target.is_symlink() or not target.is_file() or not target.resolve().is_relative_to(root):
                raise RuntimeError("source fixture is not a regular contained file")
            live_hash = hashlib.sha256(target.read_bytes()).hexdigest()
            observation.update(manifest=state,live_hash=live_hash)
            if live_hash != expected_hash:
                raise RuntimeError("source fixture changed independently authored hash")
            advanced = state is not None and state.get("index_status") == "indexed" and (before is None or before.get("index_status") == "pending")
            ready = fixture_ready(searched, state, status, path, kind, expected_hash, advanced=advanced, require_hit=require_hit)
            if clock() >= deadline:
                raise RuntimeError("incomplete preparation: shared deadline exhausted during request")
            if ready:
                return searched
        raise RuntimeError("incomplete preparation: attempt cap exhausted")
    except Exception as error:
        raise RuntimeError(f"{error}; observations: {json.dumps(record, ensure_ascii=False)}") from error


def prepare_fixture_sources(env, workspace, binary, version, fixtures):
    """Reuse the maintained bounded native transport for positive setup only.

    fixtures contain (relative path, source kind, independently authored bytes).
    Each target gets one fixed preparation deadline and attempt cap.
    """
    from ci.native_mcp import NativeMcpClient, TransportLimits
    async def run():
        records = []
        async with NativeMcpClient(
                [str(binary), "_internal", "mcp-server", "--plugin-version", version, "--adapter-protocol-version", "2"],
                cwd=workspace, env=env, limits=TransportLimits(request_timeout=5)) as client:
            for path, kind, authored_bytes in fixtures:
                record = {"file": path, "kind": kind, "responses": []}
                records.append(record)
                await prepare_source_fixture(client, workspace, path, kind, hashlib.sha256(authored_bytes).hexdigest(), record)
        return records
    return asyncio.run(run())
