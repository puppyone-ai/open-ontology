"""Protocol/negative tests. A simulated materializer must NOT pass admission."""
import copy
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest

import bounded_source as b
import producer as p


@unittest.skipUnless(os.environ.get("OO_ORACLE_NATIVE_BIN"), "explicit native test executable required")
class NativeArgvTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name).resolve()
        self.binary=Path(os.environ["OO_ORACLE_NATIVE_BIN"]).resolve()
        self.bundle=self.root/"bundle";self.bundle.mkdir()
        self.table={"owner":"UNIT","table":"ITEMS","keys":["ID"],"columns":[
            {"name":"ID","type":"NUMBER","precision":9,"scale":0,"nullable":"N"},
            {"name":"N","type":"NUMBER","precision":38,"scale":9,"nullable":"Y"}]}
        frames=[]; artifacts={}; spec=None; mappings=None
        for index,kind in enumerate(("fill","cdc"),1):
            data={"table":self.table,"rows":[["1","12.5" if kind=="fill" else "13.5"]]}
            if kind=="cdc":data["cdc"]={"positions":["1"],"operations":["upsert"]}
            source=self.bundle/f"{index}.json";b.atomic(source,b.canonical(data))
            output=self.bundle/f"{index}.arrow"
            run=subprocess.run([str(self.binary)],input=b.canonical({"action":"encode-part","part":str(source),
                "sha256":b.sha(source),"output":str(output)}),capture_output=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stderr)
            receipt=json.loads(run.stdout);spec=receipt["typed_spec"];mappings=receipt["source_mappings"]
            receipt_path=self.bundle/f"{index}.receipt.json"
            b.atomic(receipt_path,b.canonical(receipt))
            artifacts[receipt_path.name]=b.sha(receipt_path)
            frames.append({"kind":"typed_batch","batch":{"protocol":"oo.c1.typed-batch/v3","sequence":index,
                "kind":kind,"previous_position":"0","position":"0" if kind=="fill" else "1",
                "parts":[{"table":"UNIT.ITEMS","spec":spec,"path":str(output),"sha256":b.sha(output),
                          "bytes":output.stat().st_size,"rows":1}]}})
            artifacts[output.name]=b.sha(output)
        b.atomic(self.bundle/"batches.jsonl",b"".join(b.canonical(f)+b"\n" for f in frames))
        artifacts["batches.jsonl"]=b.sha(self.bundle/"batches.jsonl")
        manifest={"protocol":"oo.oracle.producer-bundle/v1","engine":"oracle","source_id":"fixture_oracle",
            "boundary":{"mode":"debezium_snapshot_and_log","identity":"UNIT-FIXTURE","snapshot_scn":"7",
                        "final_position":"1","connector_offsets_sha256":"a"*64},
            "typed_tables":{"UNIT.ITEMS":spec},"source_mappings":{"UNIT.ITEMS":mappings},
            "checkpoints":[{"fixture_only":True}],"batch_count":2,"artifacts":artifacts}
        b.atomic(self.bundle/"manifest.json",b.canonical(manifest))
        materializer=self.root/"materializer"
        materializer.write_text(f"#!{sys.executable}\nimport json,sys\nv=json.load(sys.stdin)\nassert v['engine']=='oracle'\n"
            "s=v['context']['source_id']\n"
            "print(json.dumps({'protocol':'oo.oracle.materialize-result/v1','engine':'oracle','source_id':s,"
            "'request':{'protocol':'oo.pipeline.native-c1-baseline/v1','source_id':s,'fixture_only':True}}))\n")
        materializer.chmod(0o700)
        def ref(path):
            path=Path(path).resolve()
            return {"path":str(path),"sha256":b.sha(path)}
        self.cfg={"protocol":"oo.oracle.producer-argv/v1","engine":"oracle","python":ref(sys.executable),
            "helper":ref(p.__file__),"bounded_source":ref(b.__file__),"bundle":ref(self.bundle/"manifest.json"),
            "materializer":ref(materializer),"admission_config":{"protocol":"oo.source-admission.config/v1","source_id":"fixture_oracle"}}
        self.context={"protocol":"oo.jobs.oracle-c1-bootstrap/v1","engine":"oracle","source_id":"fixture_oracle",
            "source_generation":1,"lease_id":"00000000-0000-4000-8000-000000000001","lease_generation":3}

    def tearDown(self):
        self.temp.cleanup()

    def run_peer(self,cfg,bad_ack=False):
        path=self.root/"argv.json";b.atomic(path,b.canonical(cfg));out=self.root/"transport"
        process=subprocess.Popen([str(self.binary),"c1-baseline-stream-v3","--config",str(path),"--out",str(out)],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
        frames=[]
        try:
            process.stdin.write(b.canonical(self.context)+b"\n");process.stdin.flush()
            for _ in range(200):
                self.assertTrue(select.select([process.stdout],[],[],10)[0],"producer blocked")
                line=process.stdout.readline()
                if not line:break
                frame=json.loads(line);frames.append(frame)
                kind=frame["kind"]
                if kind=="materialized":break
                stage={"open":"stage_open","stream_started":"stream_started","typed_batch":"typed_batch",
                       "snapshot_end":"snapshot_end","end":"end"}[kind]
                ack={"protocol":"oo.jobs.oracle-c1-baseline-ack/v1","stage":stage,"durable":True}
                if kind=="open":ack["source_epoch"]=7
                if kind=="typed_batch":
                    batch=frame["batch"]
                    self.assertTrue(all(Path(x["path"]).is_relative_to(out) for x in batch["parts"]))
                    ack["receipt"]={"protocol":"oo.c1.typed-receipt/v3" if bad_ack else "oo.c1.typed-ack/v3",
                        "source":self.context["source_id"],"task":self.context["lease_id"],"generation":3,"epoch":7,
                        "sequence":batch["sequence"],"position":batch["position"],"digest":p.batch_digest(batch)}
                    if frame.get("source_position",{}).get("checkpoint_kind")=="buffer_consumption":
                        position=frame["source_position"]
                        sanitized=copy.deepcopy(position)
                        for name in ("captured_event_ids","end_evidence"):sanitized[name].pop("path")
                        ack["receipt"].update({"checkpoint_kind":"buffer_consumption",
                            "context_sha256":position["context_sha256"],"bundle_sha256":position["bundle_sha256"],
                            "buffer_cursor":position["cursor_after"],"source_evidence_sha256":p.digest(position),
                            "source_offset_write":False,"source_position":{"engine":"oracle",
                            "source_offset_advanced":False,"apply_ordinal":position["target_apply_ordinal"],
                            "source_evidence_sha256":p.digest(position),"buffer_position":sanitized}})
                if kind=="end":ack["rows"]=[]
                process.stdin.write(b.canonical(ack)+b"\n");process.stdin.flush()
            stdout,stderr=process.communicate(timeout=10)
            return process.returncode,frames,stderr
        finally:
            if process.poll() is None:
                os.killpg(process.pid,signal.SIGKILL)
                process.communicate()

    def test_real_native_entry_does_not_accept_fake_materializer(self):
        code,frames,stderr=self.run_peer(self.cfg)
        self.assertEqual(code,2,stderr)
        self.assertIn(b"materialized_admission_required",stderr)
        self.assertEqual([f["kind"] for f in frames],["open","stream_started","typed_batch","snapshot_end","typed_batch","end"])
        self.assertEqual(frames[0]["source_engine"],"oracle")
        self.assertNotIn("source_uuid",frames[0])

    def test_target_receipt_without_control_ack_is_rejected(self):
        code,frames,stderr=self.run_peer(self.cfg,bad_ack=True)
        self.assertEqual(code,2)
        self.assertEqual(frames[-1]["kind"],"typed_batch")
        self.assertIn(b"typed_ack_mismatch",stderr)

    def test_missing_official_materializer_refuses_before_stage_open(self):
        cfg=copy.deepcopy(self.cfg);del cfg["materializer"]
        code,frames,stderr=self.run_peer(cfg)
        self.assertEqual(code,2)
        self.assertFalse(frames)
        self.assertIn(b"official_oracle_materializer_required",stderr)

    @unittest.skipUnless(os.environ.get("OO_ORACLE_BUFFER_INDEX"),"explicit retained captured-buffer input required")
    def test_mainline_argv_real_buffer_inputs_with_simulated_control_peer(self):
        import buffer_cursor
        index_path=Path(os.environ["OO_ORACLE_BUFFER_INDEX"]).resolve()
        index=json.loads(index_path.read_text())
        context=json.loads(Path(index["context"]["path"]).read_text())
        cfg=copy.deepcopy(self.cfg)
        cfg["bundle"]=context["bundle"]
        cfg["buffer_index"]={"path":str(index_path),"sha256":b.sha(index_path)}
        module=Path(buffer_cursor.__file__).resolve()
        cfg["buffer_cursor"]={"path":str(module),"sha256":b.sha(module)}
        cfg["admission_config"]["source_id"]=context["source_id"]
        self.context["source_id"]=context["source_id"]
        code,frames,stderr=self.run_peer(cfg)
        self.assertEqual(code,2,stderr)
        self.assertIn(b"materialized_admission_required",stderr)
        self.assertEqual(frames[0]["checkpoint_kind"],"buffer_consumption")
        self.assertEqual(frames[0]["buffer_index"]["sha256"],b.sha(index_path))
        self.assertIsInstance(frames[0]["source_identity"],dict)
        self.assertTrue(frames[0]["measured_id"].startswith("oracle."+context["source_identity"]["dbid"]+"."))
        transactions=[f for f in frames if f.get("source_position")]
        self.assertEqual(len(transactions),index["transaction_count"])
        self.assertTrue(all(f.get("buffer_transaction") for f in transactions))
        self.assertTrue(all("offset_before" not in f["source_position"] for f in transactions))
        self.assertEqual(frames[-1]["checkpoint"]["cursor"],index["final_cursor"])
        self.assertFalse(frames[-1]["checkpoint"]["source_offset_write"])


if __name__=="__main__":
    unittest.main()
