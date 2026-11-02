import copy
import hashlib
import json
from pathlib import Path
import tempfile
import tracemalloc
import unittest

import bounded_source as b
import buffer_cursor as c
import producer as p


class BufferCursorTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.context = {"protocol": c.CONTEXT, "engine": "oracle", "source_id": "oracle_test",
            "bundle": {"sha256": "a"*64}, "raw_spool": {"sha256": "b"*64},
            "source_resume": {"target_ack_updates_vendor_offset": False}}
        self.context_sha = hashlib.sha256(b.canonical(self.context)+b"\n").hexdigest()
        self.batch = {"protocol": "oo.c1.typed-batch/v3", "sequence": 7, "kind": "cdc",
            "previous_position": "0", "position": "1",
            "parts": [{"table": "S.T", "path": "/test.arrow", "sha256": "c"*64, "rows": 2, "bytes": 100}]}
        end = {"status": "END", "id": "TX1", "event_count": 2,
               "data_collections": [{"data_collection": "DB.S.T", "event_count": 2}]}
        end_path = self.root/"end.json"; b.atomic(end_path, b.canonical(end))
        ids_path = self.root/"ids.jsonl"
        events = [{"protocol": c.EVENT, "captured_id": {"transaction_id": "TX1", "total_order": i,
                   "commit_scn": "123456", "scn": "123400", "rs_id": "actual-"+str(i), "ssn": 0,
                   "db": "DB", "schema": "S", "table": "T"}, "event_body_sha256": "d"*64} for i in (1,2)]
        b.atomic(ids_path, b"".join(b.canonical(e)+b"\n" for e in events))
        self.position = {"protocol": c.POSITION, "engine": "oracle", "checkpoint_kind": "buffer_consumption",
            "context_sha256": self.context_sha, "bundle_sha256": "a"*64, "spool_sha256": "b"*64,
            "cursor_before": c.cursor(0,None), "cursor_after": c.cursor(1,"TX1"), "transaction_id": "TX1",
            "commit_scn": "123456", "event_count": 2, "end_event_count": 2,
            "event_order_first": 1, "event_order_last": 2, "events_sha256": "e"*64,
            "captured_event_ids": c.reference(ids_path), "end_evidence": c.reference(end_path),
            "typed_batch_digest": p.batch_digest(self.batch), "target_apply_ordinal": "1",
            "source_offset_write": False}

    def tearDown(self):
        self.tmp.cleanup()

    def verify(self, **kw):
        return c.verify_transition(kw.get("context",self.context),self.context_sha,
            kw.get("position",self.position),kw.get("batch",self.batch),
            kw.get("cursor",c.cursor(0,None)),kw.get("replay_digest"))

    def test_consumption_progress_is_not_vendor_progress(self):
        result = self.verify()
        self.assertEqual(result["cursor"],c.cursor(1,"TX1"))
        self.assertFalse(result["source_offset_write"])
        self.assertNotIn("offset_before",self.position)
        self.assertNotIn("offset_after",self.position)
        self.assertEqual(self.position["commit_scn"],"123456")
        replay = self.verify(cursor=c.cursor(1,"TX1"),replay_digest=result["payload_sha256"])
        self.assertTrue(replay["replay"])

    def test_replay_conflict_or_skipped_cursor_is_refused(self):
        with self.assertRaisesRegex(ValueError,"replay_conflict"):
            self.verify(cursor=c.cursor(1,"TX1"),replay_digest="f"*64)
        with self.assertRaisesRegex(ValueError,"stale_cursor"):
            self.verify(cursor=c.cursor(2,"TX2"))
        bad=copy.deepcopy(self.position);bad["cursor_after"]=c.cursor(2,"TX1")
        with self.assertRaisesRegex(ValueError,"transition_order"):
            self.verify(position=bad)

    def test_source_identity_or_bundle_cannot_cross_context(self):
        context=copy.deepcopy(self.context);context["source_id"]="other_oracle"
        with self.assertRaisesRegex(ValueError,"context_mismatch"):
            self.verify(context=context)
        for key in ("bundle_sha256","spool_sha256"):
            position=copy.deepcopy(self.position);position[key]="f"*64
            with self.assertRaisesRegex(ValueError,"context_mismatch"):
                self.verify(position=position)

    def test_end_count_id_and_payload_tamper_refuse(self):
        position=copy.deepcopy(self.position);position["end_event_count"]=1
        with self.assertRaisesRegex(ValueError,"end_count"):
            self.verify(position=position)
        path=Path(self.position["captured_event_ids"]["path"]);path.write_text("{}")
        with self.assertRaisesRegex(ValueError,"digest"):
            self.verify()

    def test_buffer_payload_cannot_be_relabelled_vendor_offset(self):
        position=copy.deepcopy(self.position);position["protocol"]="oo.c1.oracle-logminer-position/v1"
        with self.assertRaisesRegex(ValueError,"context_mismatch"):
            self.verify(position=position)
        position=copy.deepcopy(self.position);position["source_offset_write"]=True
        with self.assertRaisesRegex(ValueError,"context_mismatch"):
            self.verify(position=position)

    def test_same_commit_scn_does_not_merge_transactions(self):
        position=copy.deepcopy(self.position)
        position["cursor_before"]=c.cursor(1,"TX0")
        position["cursor_after"]=c.cursor(2,"TX1")
        position["target_apply_ordinal"]="2"
        batch=copy.deepcopy(self.batch);batch.update(previous_position="1",position="2",sequence=8)
        position["typed_batch_digest"]=p.batch_digest(batch)
        self.assertEqual(self.verify(position=position,batch=batch,cursor=c.cursor(1,"TX0"))["cursor"],c.cursor(2,"TX1"))
        self.assertEqual(position["commit_scn"],"123456")

    def test_target_ack_must_echo_buffer_identity_not_vendor_offset(self):
        sanitized=copy.deepcopy(self.position)
        for key in ("captured_event_ids","end_evidence"):sanitized[key].pop("path")
        ack={"durable":True,"receipt":{"protocol":"oo.c1.typed-ack/v3",
            "checkpoint_kind":"buffer_consumption","context_sha256":self.context_sha,
            "bundle_sha256":"a"*64,"buffer_cursor":self.position["cursor_after"],
            "source_evidence_sha256":p.digest(self.position),"source_offset_write":False,
            "source_position":{"engine":"oracle","source_offset_advanced":False,
                "apply_ordinal":self.position["target_apply_ordinal"],
                "source_evidence_sha256":p.digest(self.position),"buffer_position":sanitized}}}
        p.validate_buffer_ack(ack,self.position)
        bad=copy.deepcopy(ack);bad["receipt"]["source_offset_write"]=True
        with self.assertRaisesRegex(ValueError,"buffer_ack"):
            p.validate_buffer_ack(bad,self.position)
        bad=copy.deepcopy(ack);bad["receipt"]["buffer_cursor"]=c.cursor(0,None)
        with self.assertRaisesRegex(ValueError,"buffer_ack"):
            p.validate_buffer_ack(bad,self.position)
        bad=copy.deepcopy(ack);bad["receipt"]["source_position"]["buffer_position"]["commit_scn"]="999999"
        with self.assertRaisesRegex(ValueError,"buffer_ack"):
            p.validate_buffer_ack(bad,self.position)

    def test_large_complete_transaction_is_streamed_and_late_corruption_refuses(self):
        count = 101000
        path = self.root/"large-ids.jsonl"
        with path.open("wb") as stream:
            for order in range(1, count+1):
                table = "A" if order <= count//2 else "B"
                event = {"protocol": c.EVENT, "captured_id": {
                    "transaction_id": "TX1", "total_order": order, "commit_scn": "123456",
                    "scn": "123400", "rs_id": "r"*96+str(order), "ssn": 0,
                    "db": "DB", "schema": "S", "table": table}, "event_body_sha256": "d"*64}
                last_offset = stream.tell()
                last_line = b.canonical(event)+b"\n"
                stream.write(last_line)
        self.assertGreater(path.stat().st_size, c.MAX_BYTES)
        end = {"status": "END", "id": "TX1", "event_count": count,
               "data_collections": [{"data_collection": "DB.S."+name, "event_count": count//2}
                                    for name in ("A", "B")]}
        end_path = self.root/"large-end.json"
        b.atomic(end_path, b.canonical(end))
        batch = copy.deepcopy(self.batch)
        batch["parts"] = [dict(batch["parts"][0], table="S."+name, rows=count//2) for name in ("A", "B")]
        position = dict(self.position, event_count=count, end_event_count=count, event_order_last=count,
                        captured_event_ids=c.reference(path), end_evidence=c.reference(end_path),
                        typed_batch_digest=p.batch_digest(batch))
        original = copy.deepcopy(position)
        tracemalloc.start()
        result = self.verify(position=position, batch=batch)
        _, peak = tracemalloc.get_traced_memory()
        tracemalloc.stop()
        self.assertEqual(result["cursor"], c.cursor(1, "TX1"))
        self.assertLess(peak, 8*1024*1024, "do not retain a hash set or all event records")
        self.assertEqual(position, original)
        for mutation, error in (("digest", "digest"), ("order", "captured_id"), ("body_hash", "captured_id")):
            bad = json.loads(last_line)
            if mutation == "order":
                bad["captured_id"]["total_order"] -= 1
            else:
                bad["event_body_sha256"] = ("e" if mutation == "digest" else "g")*64
            line = b.canonical(bad)+b"\n"
            self.assertEqual(len(line), len(last_line))
            with path.open("r+b") as stream:
                stream.seek(last_offset)
                stream.write(line)
            changed = copy.deepcopy(position)
            if mutation != "digest":
                changed["captured_event_ids"] = c.reference(path)
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, error):
                self.verify(position=changed, batch=batch)
            self.assertEqual(position, original)
            with path.open("r+b") as stream:
                stream.seek(last_offset)
                stream.write(last_line)

    def test_cursor_uses_canonical_wire_integer_not_a_million_transaction_ceiling(self):
        for value in (1000001, 2**64-1):
            self.assertEqual(c.ordinal(c.cursor(value, "TX1")), value)
        for value in ("01", "-1", "1.0", str(2**64), "9"*1000):
            with self.subTest(value=value[:30]), self.assertRaises(ValueError):
                c.ordinal({"transaction_ordinal": value, "transaction_id": "TX1"})

    def test_duplicate_end_members_and_oversized_record_fail_closed(self):
        end = {"event_count": 2, "data_collections": [
            {"data_collection": "DB.S.T", "event_count": 1},
            {"data_collection": "DB.S.T", "event_count": 1}]}
        with self.assertRaisesRegex(ValueError, "end_collection"):
            c.end_collections(end)
        path = self.root/"wide-line.jsonl"
        path.write_bytes(b" "*(c.EVENT_LINE_BYTES+1)+b"\n")
        with self.assertRaisesRegex(ValueError, "record_size"):
            list(c.json_lines(c.reference(path)))


if __name__=="__main__":
    unittest.main()
