"""Exception queue, test-library isolation and sparse research-to-mapping conversion."""

import copy
from contextlib import contextmanager, redirect_stderr, redirect_stdout
import io
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("auto_curate", SCRIPTS / "calibre_auto_curate.py")
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class AutoCurationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.library = self.root / "test-library"
        self.library.mkdir()
        (self.library / "metadata.db").write_bytes(b"fixture")
        self.receipt = self.root / "receipt.json"
        self.receipt.write_text(json.dumps({
            "status": "verified", "source": str(self.root / "original"),
            "test_library": str(self.library),
        }))
        self.job = "1" * 32
        self.export = {
            "adapter_version": "nexus-facts-batch-v2",
            "library_write_performed": False, "library": str(self.library),
            "job_id": self.job,
            "mappings": [{
                "id": 1, "uuid": str(uuid.uuid4()), "title": "Fixture Book",
                "authors": ["Test Author"], "field_mask": [False, True, False, False],
                "fields": {"#series2": "Real Subseries", "#series2_index": 3.5},
                "research_id": "fixture-research",
                "evidence": [{"url": "https://author.example/chronology", "source_hash": "a" * 64}],
            }],
        }

    def test_sparse_fields_do_not_clear_main_or_deeper_levels(self):
        rows = m.convert_export(self.export, self.library, self.job)
        self.assertEqual(rows[0]["update_fields"], "#series2;#series2_index")
        self.assertEqual(rows[0]["#series2_index"], "3.5")
        self.assertNotIn("series;", rows[0]["update_fields"])
        self.assertNotIn("#series3", rows[0]["update_fields"])

    def test_default_engine_build_uses_vendored_core_and_selected_cache(self):
        target=self.root/"isolated-build-cache"
        def build(command,**kwargs):
            self.assertIn("--locked",command)
            self.assertIn("--offline",command)
            self.assertEqual(kwargs["env"]["CARGO_TARGET_DIR"],str(target))
            engine=target/"debug/nexus-calibre-research"
            engine.parent.mkdir(parents=True)
            engine.write_text("fixture")
            engine.chmod(0o700)
        self.assertTrue((m.CORE/"Cargo.toml").is_file())
        self.assertTrue(m.CORE.is_relative_to(m.REPO))
        with patch.dict(os.environ,{"CARGO_TARGET_DIR":str(target)}), \
                patch.object(m.subprocess,"run",side_effect=build):
            self.assertEqual(m.ensure_engine(),target/"debug/nexus-calibre-research")

    def test_engine_build_cache_cannot_write_inside_a_calibre_library(self):
        with patch.dict(os.environ,{"CARGO_TARGET_DIR":str(self.library/"build")}), \
                patch.object(m.subprocess,"run") as build:
            with self.assertRaisesRegex(m.Refusal,"Build cache"):
                m.ensure_engine()
            build.assert_not_called()

    def test_export_binding_and_evidence_required(self):
        for modify in (
            lambda e: e.update(job_id="other"),
            lambda e: e.update(library=str(self.root)),
            lambda e: e["mappings"][0].update(evidence=[]),
            lambda e: e["mappings"][0].update(field_mask=[False] * 4),
            lambda e: e["mappings"][0]["fields"].update(series="Injected"),
        ):
            value = copy.deepcopy(self.export)
            modify(value)
            with self.assertRaises(m.Refusal):
                m.convert_export(value, self.library, self.job)

    def test_live_library_refused_even_with_a_receipt(self):
        with patch.object(m, "LIVE_LIBRARY", self.library):
            with self.assertRaisesRegex(m.Refusal, "Live-library"):
                m.validate_clone(self.library, self.receipt)

    def test_clone_receipt_binding_and_hardlinks(self):
        self.assertEqual(m.validate_clone(self.library, self.receipt), self.library)
        self.receipt.write_text(json.dumps({
            "status": "verified", "source": str(self.library),
            "test_library": str(self.library),
        }))
        with self.assertRaises(m.Refusal):
            m.validate_clone(self.library, self.receipt)
        source = self.root / "original"
        source.mkdir()
        os.link(self.library / "metadata.db", source / "metadata.db")
        self.receipt.write_text(json.dumps({
            "status": "verified", "source": str(source), "test_library": str(self.library),
        }))
        with self.assertRaisesRegex(m.Refusal, "hardlinked"):
            m.validate_clone(self.library, self.receipt)

    def save_result(self, book_id, status, title):
        path = self.root / "engine/book-results"
        path.mkdir(parents=True, exist_ok=True)
        (path / f"book-{book_id}.json").write_text(json.dumps({
            "book": {"id": book_id, "uuid": str(uuid.uuid4()), "title": title,
                     "authors": ["Fixture Author"]},
            "status": status, "error": "Source conflict" if status == "conflict" else None,
            "run": {"notes": ["Actual research note"],
                    "evidence": [{"url": "https://author.example/source"}],
                    "hierarchy": [], "field_mask": [False] * 4},
        }))

    def test_searchable_exception_queue_omits_routine_and_protected_records(self):
        self.save_result(1, "unverified", "One Book")
        self.save_result(2, "blocked", "Other Book")
        self.save_result(3, "ready", "Mapped Book")
        self.save_result(4, "protected", "Curated Book")
        self.assertEqual(len(m.list_exceptions(self.root)), 1)
        self.assertEqual(m.list_exceptions(self.root, query="one book")[0]["id"], 1)
        self.assertEqual(m.list_exceptions(self.root, book_ids=[2]), [])
        self.assertEqual(m.list_exceptions(self.root, book_ids=[2], retry=True)[0]["status"], "blocked")
        self.assertEqual(len(m.list_exceptions(self.root, statuses=["unverified"])), 1)
        self.assertTrue(m.list_exceptions(self.root)[0]["sources"])

    def test_main_and_all_three_subseries_are_exported_independently(self):
        item=self.export["mappings"][0]
        item["field_mask"]=[True]*4
        item["fields"]={
            "series":"Fixture Universe","series_index":99,
            "#series2":"Broad Saga","#series2_index":10,
            "#series3":"Inner Arc","#series3_index":3,
            "#series4":"Smallest Arc","#series4_index":1,
        }
        row=m.convert_export(self.export,self.library,self.job)[0]
        self.assertEqual(set(row["update_fields"].split(";")),set(m.FIELDS))
        self.assertEqual([row[f] for f in ("series_index","#series2_index","#series3_index","#series4_index")],
                         ["99","10","3","1"])

    def test_invalid_evidence_and_incomplete_profiles_go_to_retry_not_manual_review(self):
        for book_id,status in enumerate(("research_error","invalid_evidence","research_pending","partial","schema_capacity"),1):
            self.save_result(book_id,status,f"Pending Book {book_id}")
        self.assertEqual(m.list_exceptions(self.root),[])
        self.assertEqual(len(m.list_exceptions(self.root,retry=True)),5)

    def test_readonly_research_checks_workspace_but_apply_still_checks_library_reserve(self):
        workspace=self.root/"research-workspace"
        with patch.object(m,"storage_guard") as guard:
            m.storage_preflight(self.library,workspace,apply=False)
            guard.assert_called_once_with(self.root)
        with patch.object(m,"storage_guard") as guard:
            m.storage_preflight(self.library,workspace,apply=True)
            self.assertEqual([c.args[0] for c in guard.call_args_list],[self.library,self.root])


    def test_invalid_types_controls_uuids_urls_and_hashes_are_refused(self):
        changes = [
            lambda x: x.update(uuid="not-a-uuid"),
            lambda x: x.update(title="Bad\nTitle"),
            lambda x: x.update(authors="Not a list"),
            lambda x: x.update(authors=["Bad\x00Author"]),
            lambda x: x.update(order_basis="publication"),
            lambda x: x["fields"].update({"#series2":"Bad\tSeries"}),
            lambda x: x["fields"].update({"#series2_index":True}),
            lambda x: x["fields"].update({"#series2_index":"3.5"}),
            lambda x: x["fields"].update({"#series2_index":float("nan")}),
            lambda x: x["fields"].update({"#series2_index":float("inf")}),
            lambda x: x["evidence"][0].update(source_hash="g"*64),
            lambda x: x["evidence"][0].update(url="https:///missing-host"),
            lambda x: x["evidence"][0].update(url="https://user:pass@author.example/source"),
            lambda x: x["evidence"][0].update(url="https://author.example:8443/source"),
        ]
        for change in changes:
            with self.subTest(change=change):
                value=copy.deepcopy(self.export)
                change(value["mappings"][0])
                with self.assertRaises(m.Refusal):
                    m.convert_export(value,self.library,self.job)

    def test_workspace_lock_precedes_clone_preflight(self):
        events=[]
        @contextmanager
        def locked(root):
            events.append("lock")
            yield
        def validate(*args):
            events.append("validate")
            raise m.Refusal("stop after ordered preflight")
        args=SimpleNamespace(library=self.library,clone_receipt=self.receipt,
                             workspace=self.root/"workspace",apply=False)
        with patch.object(m,"workspace_lock",locked), patch.object(m,"validate_clone",validate):
            with self.assertRaises(m.Refusal):
                m.run(args)
        self.assertEqual(events,["lock","validate"])

    def test_workspace_equal_to_library_is_refused_before_lock_file_creation(self):
        with self.assertRaises(m.Refusal):
            with m.workspace_lock(self.library):
                self.fail("Library directory acquired as a workspace")
        self.assertFalse((self.library/"workflow.lock").exists())

    def test_workspace_lock_does_not_follow_symlinks(self):
        workspace=self.root/"workspace"
        workspace.mkdir()
        target=self.root/"untouched"
        target.write_text("keep")
        before=target.stat()
        (workspace/"workflow.lock").symlink_to(target)
        with self.assertRaises(OSError):
            with m.workspace_lock(workspace):
                self.fail("Followed a symlink lock")
        self.assertEqual(target.read_text(),"keep")
        self.assertEqual(target.stat().st_mode,before.st_mode)

    def test_export_only_rejects_research_controls_before_any_engine_call(self):
        for flags in (["--refresh"],["--limit","1"],["--book","1"],["--retry-exceptions"]):
            with self.subTest(flags=flags), patch.object(m,"ensure_engine") as engine:
                with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stopped:
                    m.main(["--export-only",*flags])
                self.assertEqual(stopped.exception.code,2)
                engine.assert_not_called()

    def test_queue_order_uses_sequence_and_result_hash_not_mtime(self):
        self.save_result(1,"unverified","One Book")
        jobs=self.root/"jobs"
        jobs.mkdir()
        old,seq1=m.create_job(jobs,"1"*32,self.library)
        new,seq2=m.create_job(jobs,"2"*32,self.library)
        self.assertEqual((seq1,seq2),(1,2))
        old_queue=old/"exceptions.json"
        new_queue=new/"retries.json"
        old_queue.write_text(json.dumps(m.queue_observations(
            [{"id":1,"status":"unverified"}],self.root,old.name,seq1)))
        new_queue.write_text(json.dumps(m.queue_observations(
            [{"id":1,"status":"blocked","error":"new operational failure"}],self.root,new.name,seq2)))
        os.utime(old_queue,ns=(2_000_000_000_000_000_000,)*2)
        os.utime(new_queue,ns=(1,1))
        self.assertEqual(m.list_exceptions(self.root),[])
        retry=m.list_exceptions(self.root,retry=True)
        self.assertEqual(retry[0]["status"],"blocked")
        self.assertEqual(retry[0]["error"],"new operational failure")
        self.save_result(1,"ready","One Book")
        self.assertEqual(m.list_exceptions(self.root,retry=True),[])

    def test_malformed_saved_json_names_the_bad_file(self):
        self.save_result(1,"blocked","One Book")
        path=self.root/"engine/book-results/book-1.json"
        path.write_text("{partial")
        with self.assertRaisesRegex(m.Refusal,"book-1.json"):
            m.list_exceptions(self.root,retry=True)
        path.write_text("{}")
        with self.assertRaisesRegex(m.Refusal,"book-1.json"):
            m.list_exceptions(self.root,retry=True)

    def test_summary_inspection_uses_sequence_and_never_starts_engine_or_locks(self):
        workspace=self.root/"workspace"
        jobs=workspace/"jobs"
        jobs.mkdir(parents=True)
        for job_id in ("1"*32,"2"*32):
            job,seq=m.create_job(jobs,job_id,self.library)
            (job/"summary.json").write_text(json.dumps({
                "job_id":job_id,"job_sequence":seq,"library":str(self.library),
                "applied":False,"validation_errors":[],
            }))
        with patch.object(m,"ensure_engine") as engine, patch.object(m,"workspace_lock") as lock:
            output=io.StringIO()
            with redirect_stdout(output):
                code=m.main(["--library",str(self.library),"--workspace",str(workspace),
                             "--last-summary"])
            self.assertEqual(code,0)
            self.assertEqual(json.loads(output.getvalue())["job_id"],"2"*32)
            engine.assert_not_called()
            lock.assert_not_called()
        with self.assertRaises(m.Refusal):
            m.inspect_summary(workspace,self.library,"../outside")

    def test_export_only_disables_ai_and_noop_apply_records_paths_and_reason(self):
        workspace=self.root/"workspace"
        def engine_run(command,**kwargs):
            request=json.loads(Path(command[1]).read_text())
            self.assertEqual(request["mode"],"export")
            self.assertFalse(request["enable_ai"])
            self.assertFalse(request["refresh"])
            self.assertEqual(request["book_ids"],[])
            self.assertIsNone(request["limit"])
            export=copy.deepcopy(self.export)
            export.update(job_id=request["job_id"],statuses={},exceptions=[],retries=[])
            path=workspace/"engine/exports"/f"{request['job_id']}.json"
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps(export))
        with patch.object(m,"read_csv",return_value=({},[])), \
                patch.object(m,"snapshot",return_value={"books":{}}), \
                patch.object(m,"protected_ids",return_value=set()), \
                patch.object(m,"ensure_engine",return_value=Path("/fixture-engine")), \
                patch.object(m,"build_report",return_value={"errors":[],"changes":[]}), \
                patch.object(m.subprocess,"run",side_effect=engine_run) as process:
            output=io.StringIO()
            with redirect_stdout(output):
                code=m.main(["--library",str(self.library),"--clone-receipt",str(self.receipt),
                             "--workspace",str(workspace),"--export-only","--apply"])
            summary=json.loads(output.getvalue())
            self.assertEqual(code,0)
            self.assertFalse(summary["applied"])
            self.assertEqual(summary["apply_status"],"no_changes")
            self.assertIn("No metadata changes",summary["apply_note"])
            self.assertTrue(Path(summary["mapping_csv_path"]).is_file())
            self.assertTrue(Path(summary["dry_run_report_path"]).is_file())
            self.assertEqual(summary["job_sequence"],1)
            self.assertEqual(process.call_count,1)

    def test_apply_status_covers_each_audit_and_native_outcome(self):
        scenarios = [
            (False,True,False,True,None,"dry_run_only"),
            (False,True,True,True,None,"dry_run_only"),
            (True,True,False,True,None,"applied"),
            (True,True,True,True,None,"blocked_by_errors"),
            (True,True,False,False,None,"no_changes"),
            (False,False,False,False,None,"dry_run_only"),
            (True,False,False,False,None,"no_verified_mapping"),
            (True,True,False,True,"native","failed"),
            (True,True,False,True,"guard","failed"),
        ]
        for n,(apply,has_rows,errors,changes,failure,expected) in enumerate(scenarios):
            with self.subTest(status=expected,failure=failure,apply=apply):
                workspace=self.root/f"scenario-{n}"
                def invoke(command,**kwargs):
                    if command[0]=="calibre-debug":
                        if failure=="native":
                            raise m.subprocess.CalledProcessError(2,command)
                        return
                    request=json.loads(Path(command[1]).read_text())
                    export=copy.deepcopy(self.export)
                    export.update(job_id=request["job_id"],statuses={},exceptions=[],retries=[])
                    if not has_rows:
                        export["mappings"]=[]
                    path=workspace/"engine/exports"/f"{request['job_id']}.json"
                    path.parent.mkdir(parents=True)
                    path.write_text(json.dumps(export))
                def guard(*args):
                    if failure=="guard":
                        raise m.Refusal("Fixture reserve refusal")
                audit={"errors":["Fixture validation conflict"] if errors else [],
                       "changes":[{"id":1}] if changes else [],"approval":"fixture-approval"}
                with patch.object(m,"read_csv",return_value=({},[])), \
                        patch.object(m,"snapshot",return_value={"books":{}}), \
                        patch.object(m,"protected_ids",return_value=set()), \
                        patch.object(m,"ensure_engine",return_value=Path("/fixture-engine")), \
                        patch.object(m,"storage_preflight"), \
                        patch.object(m,"storage_guard",side_effect=guard), \
                        patch.object(m,"build_report",return_value=audit), \
                        patch.object(m.subprocess,"run",side_effect=invoke) as process:
                    flags=["--library",str(self.library),"--clone-receipt",str(self.receipt),
                           "--workspace",str(workspace),"--export-only"]
                    if apply:
                        flags.append("--apply")
                    with redirect_stdout(io.StringIO()),redirect_stderr(io.StringIO()):
                        code=m.main(flags)
                summary=m.inspect_summary(workspace,self.library)
                self.assertEqual(summary["apply_status"],expected)
                self.assertIn(summary["apply_status"],{s.value for s in m.ApplyStatus})
                self.assertEqual(summary["applied"],expected=="applied")
                self.assertEqual(code,2 if errors or failure else 0)
                self.assertEqual(process.call_count,2 if expected=="applied" or failure=="native" else 1)
                if failure:
                    self.assertIn("apply_error",summary)
                    self.assertEqual(summary["apply_attempted"],failure=="native")
        self.assertEqual((self.library/"metadata.db").read_bytes(),b"fixture")

    def test_empty_retry_selection_has_explicit_status_and_does_not_start_engine(self):
        workspace=self.root/"empty-retry"
        with patch.object(m,"read_csv",return_value=({},[])), \
                patch.object(m,"snapshot",return_value={"books":{}}), \
                patch.object(m,"protected_ids",return_value=set()), \
                patch.object(m,"ensure_engine") as engine:
            output=io.StringIO()
            with redirect_stdout(output):
                code=m.main(["--library",str(self.library),"--clone-receipt",str(self.receipt),
                             "--workspace",str(workspace),"--retry-exceptions"])
        self.assertEqual(code,0)
        summary=json.loads(output.getvalue())
        self.assertEqual(summary["apply_status"],"skipped_no_selection")
        self.assertFalse(summary["applied"])
        engine.assert_not_called()

    def test_summary_rejects_unknown_apply_status_but_keeps_legacy_summaries_readable(self):
        workspace=self.root/"status-inspection"
        jobs=workspace/"jobs"
        jobs.mkdir(parents=True)
        job,sequence=m.create_job(jobs,"3"*32,self.library)
        summary={"job_id":job.name,"job_sequence":sequence,"library":str(self.library),"applied":False}
        path=job/"summary.json"
        path.write_text(json.dumps(summary))
        self.assertEqual(m.inspect_summary(workspace,self.library)["job_id"],job.name)
        summary["apply_status"]="invented-status"
        path.write_text(json.dumps(summary))
        with self.assertRaisesRegex(m.Refusal,"apply_status"):
            m.inspect_summary(workspace,self.library)


if __name__ == "__main__":
    unittest.main()
