#!/usr/bin/env python3
"""Gate regression tests without Docling, models, or user source material."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gate
from ci import source_preparation

HASH = hashlib.sha256(b"fixture bytes").hexdigest()


def entry(name="readable.pdf", anchors=None, support="readable"):
    return {"file": name, "kind": "pdf", "anchors": anchors if anchors is not None else [{"page": 1, "text": "base is not conditional"}],
            "native": {"support": support, "coverage": "text_unavailable" if support == "text_unavailable" else "complete", "anchor": {"page": 1}}}


def source_reply(arguments, excerpt="base is not conditional", coverage="complete", page=None):
    return {"schema_version": 1, "action": arguments["action"], "active": True,
            "file": {"path": arguments["path"], "content_sha256": arguments.get("content_sha256", HASH), "source_kind":"pdf"},
            "anchor": {"kind": "page", "physical_start": page or arguments["anchor_start"], "physical_end": page or arguments["anchor_end"]},
            "coverage": {"status": coverage, "missing_ranges": ["page 1-1"] if coverage == "text_unavailable" else []},
            "extraction": {"text_layer_only": True, "note": "text-layer extraction only; scanned pages are not understood", "limits": {"max_pdf_pages": 2048}},
            "basis": {"visual_analysis": False},
            **({"excerpt": excerpt} if arguments["action"] == "inspect" else {})}


def search_reply(path="readable.pdf", page=1):
    return {"coverage": "complete", "reconciliation": None, "hits": [{"candidate_id": "candidate", "path": path,
            "content_sha256": HASH, "anchor_kind": "page", "anchor_start": page, "anchor_end": page,"authority":"original_source"}]}


class FakeClient:
    def __init__(self, reply):
        self.reply, self.calls, self.closed = reply, [], False
    async def __aenter__(self):
        return self
    async def __aexit__(self, *_):
        self.closed = True
    async def call(self, name, arguments):
        self.calls.append((name, arguments))
        return self.reply(name, arguments)
    def diagnostics(self):
        return {"stderr": "fixture diagnostic"}


class GateTests(unittest.IsolatedAsyncioTestCase):
    async def baseline(self, documents, reply, **kwargs):
        with tempfile.TemporaryDirectory() as workspace:
            for document in documents:
                Path(workspace, document["file"]).write_bytes(b"fixture bytes")
            client = FakeClient(reply)
            def state(_workspace, path):
                return {"path":path,"content_sha256":HASH,"source_kind":"pdf","index_status":"indexed","present":1,"exclusion_reason":None,"last_error":None}
            with patch.object(source_preparation, "read_state", side_effect=state):
                records = await gate.native_baseline({}, workspace, {"documents": documents}, client_factory=lambda *_: client, **kwargs)
        self.assertTrue(client.closed)
        return records, client

    async def test_current_reference_never_uses_removed_handle(self):
        records, client = await self.baseline([entry()], lambda name, args: source_reply(args) if name == "sources" else search_reply())
        self.assertEqual(records[0]["status"], "PASS", records)
        references = [args for name, args in client.calls if name == "sources"]
        self.assertEqual([args["action"] for args in references], ["status", "status", "inspect"])
        self.assertNotIn("content_sha256", references[0])
        for args in references[1:]:
            self.assertEqual(set(args), {"action", "path", "content_sha256", "anchor_kind", "anchor_start", "anchor_end"})
        self.assertEqual(records[0]["anchors"][0]["status"], "PASS")
        self.assertEqual(records[0]["retrieval"][0]["status"], "PASS")

    async def test_zero_hit_scan_is_inspected_and_truthful(self):
        def reply(name, args):
            return source_reply(args, excerpt="", coverage="text_unavailable") if name == "sources" else {"coverage": "complete", "reconciliation": None, "hits": []}
        records, client = await self.baseline([entry("blank.pdf", [], "text_unavailable")], reply)
        self.assertEqual(records[0]["status"], "PASS", records)
        self.assertTrue(any(name == "sources" and args["action"] == "status" for name, args in client.calls))
        self.assertEqual(records[0]["retrieval"][0]["hits"], 0)

    async def test_wrong_text_wrong_page_and_missing_second_fail(self):
        for excerpt, page, anchors in [("base is conditional", 1, None), ("base is not conditional", 2, None),
                                      ("base is not conditional", 1, [{"page": 1, "text": "base is not conditional"}, {"page": 2, "text": "writers append frames"}])]:
            with self.subTest(excerpt=excerpt, page=page):
                def reply(name, args):
                    return source_reply(args, excerpt=excerpt, page=page) if name == "sources" else search_reply()
                records, _ = await self.baseline([entry(anchors=anchors)], reply)
                self.assertEqual(records[0]["status"], "FAIL", records)
                self.assertTrue(records[0]["errors"])

    async def test_right_text_but_ineligible_retrieval_fails_separately(self):
        records, _ = await self.baseline([entry()], lambda name, args: source_reply(args) if name == "sources" else search_reply(page=2))
        self.assertEqual(records[0]["anchors"][0]["status"], "PASS")
        self.assertEqual(records[0]["retrieval"][0]["status"], "FAIL")
        self.assertEqual(records[0]["status"], "FAIL")

    async def test_unsupported_has_no_invented_anchor(self):
        document = {"file": "notes.docx", "kind": "docx", "anchors": [{"section": "body", "text": "note"}], "native": {"support": "unsupported", "coverage": "unsupported"}}
        def reply(name, args):
            self.assertEqual(name, "sources")
            self.assertEqual(set(args), {"action", "path", "content_sha256"})
            return {"schema_version": 1, "action": "status", "active": True, "coverage": {"status": "unsupported"}, "file": {"path": "notes.docx"}, "alternative": "use its native application"}
        records, _ = await self.baseline([document], reply)
        self.assertEqual(records[0]["status"], "NOT_RUN")
        self.assertEqual(records[0]["qualification_status"], "PASS")

    async def test_final_calls_are_one_shot_bounded_and_keep_every_response(self):
        for coverage in ("stale","excluded","partial","unknown"):
            client=FakeClient(lambda *_:{"coverage":{"status":coverage}})
            record={"responses":[]}
            result=await gate.call_once(client,"sources",{},record,deadline=5,clock=lambda:0)
            self.assertEqual(result["coverage"]["status"],coverage)
            self.assertEqual(len(client.calls),1)
            self.assertEqual(len(record["responses"]),1)
        client=FakeClient(lambda *_:{})
        record={"responses":[]}
        with self.assertRaisesRegex(RuntimeError,"shared deadline"):
            await gate.call_once(client,"sources",{},record,deadline=5,clock=lambda:5)
        self.assertEqual(client.calls,[])
        self.assertEqual(len(record["responses"]),1)
        self.assertIn("error",record["responses"][0])

    async def test_malformed_preparation_and_wrong_hash_do_not_retry(self):
        client = FakeClient(lambda *_: {"coverage": "indexing_on_demand", "hits": [], "reconciliation": "unknown-state"})
        with self.assertRaisesRegex(RuntimeError, "malformed"):
            await gate.call_once(client, "search_sources", {}, {"responses": []}, deadline=5, clock=lambda: 0)
        self.assertEqual(len(client.calls), 1)
        def reply(name, args):
            if name == "search_sources":
                return search_reply()
            result = source_reply(args)
            result["file"]["content_sha256"] = "0" * 64
            return result
        records, client = await self.baseline([entry()], reply)
        self.assertEqual(records[0]["status"], "FAIL")
        self.assertEqual(len(client.calls), 2)
        self.assertIn("changed identity", str(records[0]["errors"]))

    async def test_final_semantic_query_does_not_retry_wrong_identity_or_missing_text(self):
        for wrong in ("hash", "text"):
            def reply(name, args):
                if name == "sources":
                    return source_reply(args, excerpt="base is conditional" if wrong == "text" and args["action"] == "inspect" else "base is not conditional")
                result = search_reply()
                if args["query"] != "readable.pdf":
                    result["coverage"] = "indexing_on_demand"
                    if wrong == "hash": result["hits"][0]["content_sha256"] = "0"*64
                return result
            records, client = await self.baseline([entry()], reply)
            self.assertEqual(records[0]["status"], "FAIL", records)
            queries = [args["query"] for name, args in client.calls if name == "search_sources"]
            self.assertEqual(queries, ["readable.pdf", "base is not conditional"])

    async def test_call_error_kept_and_no_retry(self):
        def reply(*_):
            raise RuntimeError("unknown field handle")
        records, client = await self.baseline([entry()], reply)
        self.assertEqual(records[0]["status"], "FAIL")
        self.assertEqual(len(client.calls), 1)
        self.assertIn("unknown field", str(records[0]))
        self.assertIn("error", records[0]["responses"][0])


class HelperTests(unittest.TestCase):
    def test_anchor_match_preserves_location_negation_and_signs(self):
        document = {"kind": "pdf", "anchors": [{"page": 1, "text": "e(t+1) ≥ e(t)"}, {"page": 1, "text": "is not conditional"}]}
        page = lambda text, n: {"anchor_kind": "page", "anchor_start": n, "anchor_end": n, "text": text}
        self.assertEqual(gate.check_anchors(document, [page("e(t+1) ≥ e(t); is not conditional", 1)]), [])
        self.assertEqual(len(gate.check_anchors(document, [page("e(t+1) < e(t); is conditional", 1)])), 2)
        self.assertEqual(len(gate.check_anchors(document, [page("filler", 1), page("e(t+1) ≥ e(t); is not conditional", 2)])), 2)
        office = {"kind": "docx", "anchors": [{"section": "body", "text": "pinned base"}]}
        self.assertTrue(gate.check_anchors(office, [page("pinned base", 1)]))
        self.assertEqual(gate.check_anchors(office, [{"anchor_kind": "section", "text": "pinned base"}]), [])

    def test_versions_are_probed_from_the_actual_binary(self):
        for version in ("0.2.0", "0.3.7", "1.0.0-rc.1"):
            with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 0, f"magnesium {version}\n", "")) as probe:
                client = gate.native_client({"HOME": "private-home"}, "private-workspace", gate.GateConfig())
                self.assertEqual(client.command, [gate.BIN, "_internal", "mcp-server", "--plugin-version", version, "--adapter-protocol-version", "2"])
                probe.assert_called_once_with({"HOME": "private-home"}, "--version", cwd="private-workspace")
        for output in ("", "other 0.2.0", "magnesium v0.2.0", "magnesium 0.2", "magnesium 00.2.0", "magnesium 0.2.0-", "magnesium 0.2.0+bad..metadata", "magnesium 0.2.0\nextra", "magnesium 0.2.0-" + "a" * 128):
            with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 0, output, "")), self.assertRaisesRegex(RuntimeError, "bounded semantic version"):
                gate.native_client({}, ".", gate.GateConfig())
        with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 2, "", "probe failed")), self.assertRaisesRegex(RuntimeError, "probe failed"):
            gate.runtime_version({}, ".")


class SampleManifestTests(unittest.TestCase):
    def test_fixed_sample_explicit_native_expectations_and_chrome_option(self):
        spec = importlib.util.spec_from_file_location("sample_generator", Path(gate.HERE, "sample/generate.py"))
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        with tempfile.TemporaryDirectory() as root:
            def pdf(_chrome, _html, path):
                Path(path).write_bytes(b"mock PDF rendering")
            def scan(_chrome, lines, path):
                Path(path).write_bytes(b"mock rasterization")
                return " ".join(lines)
            with patch.object(generator, "find_chrome", return_value="/explicit/chrome") as chrome, patch.object(generator, "print_pdf", side_effect=pdf), patch.object(generator, "scanned_pdf_from_text", side_effect=scan), contextlib.redirect_stdout(io.StringIO()):
                generator.main(["--chrome", "/explicit/chrome", root])
            chrome.assert_called_once_with("/explicit/chrome")
            documents = json.loads(Path(root, "manifest.json").read_text())["documents"]
        self.assertEqual(len(documents), 12)
        self.assertEqual([document["native"]["support"] for document in documents].count("readable"), 5)
        self.assertEqual([document["native"]["support"] for document in documents].count("text_unavailable"), 1)
        self.assertEqual([document["native"]["support"] for document in documents].count("unsupported"), 6)
        scan_entry = next(document for document in documents if document["native"]["support"] == "text_unavailable")
        self.assertEqual(scan_entry["native"]["anchor"], {"page": 1})
        self.assertTrue(any(anchor["text"] == "e(t+1) ≥ e(t)" for document in documents for anchor in document["anchors"]))


class OutputTests(unittest.TestCase):
    def sample(self, root):
        documents = [entry(f"sample-{index}.pdf") for index in range(12)]
        for document in documents:
            Path(root, document["file"]).write_bytes(b"fixture")
        Path(root, "manifest.json").write_text(json.dumps({"documents": documents}))

    def test_worker_failure_keeps_all_documents_and_writes_failure(self):
        with tempfile.TemporaryDirectory() as root:
            self.sample(root)
            out = Path(root, "result.json")
            async def baseline(*_, **__):
                return [{"file": f"sample-{i}.pdf", "status": "PASS"} for i in range(12)]
            with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 0, "", "")), patch.object(gate, "native_baseline", side_effect=baseline), patch.object(gate, "try_docling", return_value=({}, "", 0)), patch.object(gate, "worker_convert", side_effect=RuntimeError("worker EOF")), patch.object(gate, "package_size_mb", return_value=None):
                self.assertEqual(gate.main(["--sample", root, "--out", str(out)]), 1)
            result = json.loads(out.read_text())
            self.assertEqual(result["status"], "FAIL")
            self.assertEqual(len(result["documents"]), 12)
            self.assertTrue(all(document["worker"]["status"] == "FAIL" for document in result["documents"]))

    def test_initialization_failure_writes_each_document_and_optional_not_run(self):
        with tempfile.TemporaryDirectory() as root:
            self.sample(root)
            out = Path(root, "result.json")
            with patch.object(gate, "run_binary", side_effect=RuntimeError("init failed")):
                self.assertEqual(gate.main(["--sample", root, "--out", str(out)]), 1)
            result = json.loads(out.read_text())
            self.assertEqual(len(result["documents"]), 12)
            self.assertTrue(all(document["native"]["status"] == "FAIL" for document in result["documents"]))
            self.assertTrue(all(document["worker"]["status"] == "NOT_RUN" for document in result["documents"]))

    def test_native_protocol_failure_reaps_child_and_writes_every_document(self):
        from ci.native_mcp import NativeMcpClient, TransportLimits
        from ci.test_native_mcp import CHILD
        with tempfile.TemporaryDirectory() as root:
            self.sample(root)
            out = Path(root, "result.json")
            clients = []
            def factory(env, workspace, _config):
                client = NativeMcpClient([sys.executable, "-u", "-c", CHILD, "timeout"], cwd=workspace, env=env,
                                         limits=TransportLimits(request_timeout=0.1, shutdown_timeout=0.1, max_frame_bytes=4096,
                                                                max_notifications=4, max_stderr_bytes=1024, max_stderr_total_bytes=8192))
                clients.append(client)
                return client
            with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 0, "", "")), patch.object(gate, "native_client", side_effect=factory), patch.object(gate, "try_docling", return_value=(None, "not installed", 0)), patch.object(gate, "package_size_mb", return_value=None), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main(["--sample", root, "--out", str(out)]), 1)
            result = json.loads(out.read_text())
            self.assertEqual(len(result["documents"]), 12)
            self.assertEqual(result["status"], "FAIL")
            self.assertTrue(all(document["native"]["status"] == "FAIL" for document in result["documents"]))
            self.assertTrue(all(document["native"]["transport"]["returncode"] is not None for document in result["documents"]))
            # The timeout child would sleep for a minute, so a return code proves the gate reaped it.
            self.assertTrue(clients)
            self.assertTrue(all(client.proc.returncode is not None for client in clients))

    def test_missing_optional_conversion_is_not_run_without_weakening_native(self):
        with tempfile.TemporaryDirectory() as root:
            self.sample(root)
            out = Path(root, "result.json")
            async def baseline(*_, **__):
                return [{"file": f"sample-{i}.pdf", "status": "FAIL" if i == 3 else "PASS", "errors": ["missing text"] if i == 3 else []} for i in range(12)]
            with patch.object(gate, "run_binary", return_value=subprocess.CompletedProcess([], 0, "", "")), patch.object(gate, "native_baseline", side_effect=baseline), patch.object(gate, "try_docling", return_value=(None, "not installed", 0)), patch.object(gate, "package_size_mb", return_value=None):
                self.assertEqual(gate.main(["--sample", root, "--out", str(out)]), 1)
            result = json.loads(out.read_text())
            self.assertEqual(result["worker"]["status"], "NOT_RUN")
            self.assertEqual(result["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
