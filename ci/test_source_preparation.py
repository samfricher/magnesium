"""Deterministic readable-source continuation policy, without any sidecar."""
from contextlib import closing
import hashlib
import sqlite3
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from ci import source_preparation as preparation

BYTES=b"Readers pin the end mark.\n"
HASH=hashlib.sha256(BYTES).hexdigest()


def packet(kind="text", status="indexed", coverage="complete", diagnostic=None):
    anchor="page" if kind=="pdf" else "line"
    return {"coverage":coverage,"reconciliation":diagnostic,"hits":[] if status!="indexed" else [
        {"path":"lesson.md","content_sha256":HASH,"anchor_kind":anchor,"anchor_start":1,"anchor_end":1,"authority":"original_source"}]}


def manifest(kind="text", status="indexed"):
    return {"path":"lesson.md","content_sha256":HASH,"source_kind":kind,"index_status":status,"present":1,"exclusion_reason":None,"last_error":None}


def status(kind="text", coverage="complete"):
    return {"schema_version":1,"action":"status","active":True,
            "file":{"path":"lesson.md", "content_sha256":HASH,"source_kind":kind},
            "anchor":{"kind":"page" if kind=="pdf" else "line","physical_start":1,"physical_end":1},
            "coverage":{"status":coverage,"last_error":None}}


class Client:
    def __init__(self, responses): self.responses=iter(responses); self.calls=[]
    async def call(self,name,arguments): self.calls.append((name,dict(arguments))); return next(self.responses)


class ManifestConnectionLifecycleTests(unittest.TestCase):
    def check_closed_read(self, outcome):
        with tempfile.TemporaryDirectory() as workspace:
            index=Path(workspace,".magnesium/index/source-search.sqlite")
            index.parent.mkdir(parents=True)
            with closing(sqlite3.connect(index)) as setup:
                if outcome!="error":
                    setup.execute("CREATE TABLE source_manifest(display_path TEXT, content_sha256 TEXT, source_kind TEXT, index_status TEXT, present INTEGER, exclusion_reason TEXT, last_error TEXT)")
                    if outcome=="row":
                        setup.execute("INSERT INTO source_manifest VALUES(?,?,?,?,?,?,?)",("lesson.md",HASH,"text","indexed",1,None,None))
                    setup.commit()
            uri=index.resolve().as_uri()+"?mode=ro"
            connection=sqlite3.connect(uri,uri=True)
            try:
                with patch.object(preparation.sqlite3,"connect",return_value=connection) as connect:
                    if outcome=="error":
                        with self.assertRaisesRegex(sqlite3.OperationalError,"no such table"):
                            preparation.read_state(workspace,"lesson.md")
                    else:
                        result=preparation.read_state(workspace,"lesson.md")
                        self.assertEqual(result,manifest() if outcome=="row" else None)
                    connect.assert_called_once_with(uri,uri=True)
                # This checks the actual SQLite resource, not just context exit
                # or a mock close counter. A transaction context leaves it open.
                with self.assertRaisesRegex(sqlite3.ProgrammingError,"closed database"):
                    connection.execute("SELECT 1")
            finally:
                connection.close()

    def test_row_read_closes_read_only_connection(self):
        self.check_closed_read("row")

    def test_no_row_read_closes_read_only_connection(self):
        self.check_closed_read("no_row")

    def test_query_error_closes_read_only_connection(self):
        self.check_closed_read("error")


class SourcePreparationTests(unittest.IsolatedAsyncioTestCase):
    async def run_fixture(self, responses, states, **kwargs):
        client=Client(responses); record={"responses":[]}
        with tempfile.TemporaryDirectory() as workspace:
            Path(workspace,"lesson.md").write_bytes(BYTES)
            with patch.object(preparation,"read_state",side_effect=states):
                result=await preparation.prepare_source_fixture(client,workspace,"lesson.md","text",HASH,record,**kwargs)
        return result,record,client

    async def test_pending_and_no_row_resume_then_require_original_indexed_hit(self):
        for first_state,coverage,diagnostic in [(None,"partial","deferred_budget"),
                (None,"partial","deferred_concurrent_update"),(manifest(status="pending"),"indexing_on_demand",None)]:
            with self.subTest(state=first_state):
                result,record,client=await self.run_fixture(
                    [packet(status="pending",coverage=coverage,diagnostic=diagnostic),status(coverage="unknown" if first_state is None else "partial"),packet(),status()],
                    [first_state,first_state,manifest(),manifest()],clock=lambda:0)
                self.assertTrue(result["hits"]); self.assertEqual(len(client.calls),4)
                self.assertEqual(len(record["responses"]),4); self.assertEqual(len(record["preparation_states"]),2)
                self.assertEqual(client.calls[0][1],{"query":"lesson.md","path_prefix":"lesson.md","limit":50})

    def test_blank_readiness_uses_truthful_status_not_preparation_path_matches(self):
        searched=packet("pdf",status="pending")
        self.assertTrue(preparation.fixture_ready(searched,manifest("pdf"),status("pdf",coverage="text_unavailable"),"lesson.md","pdf",HASH,require_hit=False))
        self.assertTrue(preparation.fixture_ready(packet("pdf"),manifest("pdf"),status("pdf",coverage="text_unavailable"),"lesson.md","pdf",HASH,require_hit=False))

    async def test_status_advanced_index_requires_later_matching_hit(self):
        _,_,client=await self.run_fixture([packet(status="pending",coverage="partial",diagnostic="deferred_budget"),status(),packet(),status()],
            [None,manifest(),manifest(),manifest()],clock=lambda:0)
        self.assertEqual(len(client.calls),4)

    async def test_wrong_hit_fails_before_status_or_retry(self):
        for field,value in [("content_sha256","b"*64),("path","other.md"),("authority","derived"),("anchor_kind","page")]:
            client=Client([packet(coverage="indexing_on_demand")]); client.responses=iter([packet(coverage="indexing_on_demand")])
            response=packet(coverage="indexing_on_demand"); response["hits"][0][field]=value; client.responses=iter([response])
            with tempfile.TemporaryDirectory() as workspace, self.assertRaisesRegex(RuntimeError,"identity or authority"):
                await preparation.prepare_source_fixture(client,workspace,"lesson.md","text",HASH,{"responses":[]},clock=lambda:0)
            self.assertEqual(len(client.calls),1)

    async def test_observed_terminal_before_row_cannot_be_healed_by_status(self):
        for field,value in [("index_status","failed"),("index_status","stale"),("index_status","excluded"),("index_status","deleted"),
                ("content_sha256","b"*64),("source_kind","pdf"),("present",0),("exclusion_reason","magnesiumignore"),("last_error","failed extraction")]:
            before=manifest(status="pending");before[field]=value
            client=Client([packet(status="pending",coverage="partial",diagnostic="deferred_budget"),status()]);record={"responses":[]}
            with tempfile.TemporaryDirectory() as workspace,patch.object(preparation,"read_state",side_effect=[before,manifest()]),self.assertRaises(RuntimeError):
                await preparation.prepare_source_fixture(client,workspace,"lesson.md","text",HASH,record,clock=lambda:0)
            self.assertEqual(len(client.calls),1)
            self.assertEqual(record["preparation_states"][0]["before_status"],before)

    async def test_one_cap_and_shared_deadline_keep_all_observations(self):
        with self.assertRaisesRegex(RuntimeError,"attempt cap"):
            await self.run_fixture([packet(status="pending",coverage="partial",diagnostic="deferred_budget"),status(coverage="partial")]*2,
                [manifest(status="pending")]*4,attempts=2,clock=lambda:0)
        clock=iter([0,0,0,0,0,5])
        with self.assertRaisesRegex(RuntimeError,"shared deadline"):
            await self.run_fixture([packet(status="pending",coverage="partial",diagnostic="deferred_budget"),status(coverage="partial")],
                [manifest(status="pending")]*2,clock=lambda:next(clock))

    async def test_changed_missing_and_symlink_inputs_fail_once(self):
        for mutation in ("changed","missing","symlink"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as workspace:
                target=Path(workspace,"lesson.md"); target.write_bytes(BYTES)
                if mutation=="changed": target.write_bytes(b"different")
                elif mutation=="missing": target.unlink()
                else:
                    target.unlink(); outside=Path(workspace,"outside.md"); outside.write_bytes(BYTES); target.symlink_to(outside)
                client=Client([packet(),status()])
                with patch.object(preparation,"read_state",return_value=manifest()),self.assertRaisesRegex(RuntimeError,"regular contained|authored hash"):
                    await preparation.prepare_source_fixture(client,workspace,"lesson.md","text",HASH,{"responses":[]},clock=lambda:0)
                self.assertEqual(len(client.calls),2)

    def test_text_pdf_identity_policy_and_discovery_classification(self):
        for kind in ("text","pdf"):
            self.assertTrue(preparation.fixture_ready(packet(kind),manifest(kind),status(kind),"lesson.md",kind,HASH))
            for field,value in [("index_status","failed"),("index_status","stale"),("index_status","excluded"),("index_status","deleted"),
                    ("content_sha256","b"*64),("present",0),("exclusion_reason","magnesiumignore"),("last_error","extraction failed")]:
                state=manifest(kind); state[field]=value
                with self.assertRaises(RuntimeError): preparation.fixture_ready(packet(kind,status="pending",coverage="partial",diagnostic="deferred_budget"),state,status(kind,coverage="partial"),"lesson.md",kind,HASH)
            for coverage,diagnostic in [("partial",None),("indexing_on_demand",None)]:
                with self.assertRaises(RuntimeError): preparation.fixture_ready(packet(kind,status="pending",coverage=coverage,diagnostic=diagnostic),None,status(kind,coverage="unknown"),"lesson.md",kind,HASH)
            with self.assertRaises(RuntimeError): preparation.fixture_ready(packet(kind,status="pending",coverage="partial",diagnostic="deferred_budget"),manifest(kind),status(kind),"lesson.md",kind,HASH)


if __name__=="__main__": unittest.main()
