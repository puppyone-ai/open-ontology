import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import bounded_source as b
import producer as p


TABLE={"owner":"S","table":"T","keys":["ID"],"columns":[
    {"name":"ID","type":"NUMBER","precision":9,"scale":0,"nullable":"N"},
    {"name":"N","type":"NUMBER","precision":38,"scale":9,"nullable":"Y"},
    {"name":"R","type":"RAW","nullable":"Y"},
    {"name":"TXT","type":"VARCHAR2","nullable":"Y"}]}


class ProducerTests(unittest.TestCase):
    def test_decimal_and_binary_are_exact_without_float(self):
        self.assertEqual(p.decode_row({"ID":"1","N":"99999999999999999999.123456789","R":"AAD/","TXT":" "},TABLE),
                         ["1","99999999999999999999.123456789","0000ff"," "])
        self.assertEqual(p.canonical_number("-0E-9"),"0")
        self.assertEqual(p.canonical_number("1E-130"),"0."+"0"*129+"1")
        for value in [1.25,"NaN","Infinity","1E9999","abc"]:
            with self.assertRaises(ValueError):
                p.canonical_number(value)

    def test_missing_fields_null_key_and_unknown_temporal_contract_refuse(self):
        for row in [{"ID":"1"},{"ID":None,"N":None,"R":None,"TXT":None}]:
            with self.assertRaises(ValueError):
                p.decode_row(row,TABLE)
        t=copy.deepcopy(TABLE)
        t["columns"][3]["type"]="TIMESTAMP(9)"
        with self.assertRaisesRegex(ValueError,"type_contract"):
            p.decode_row({"ID":"1","N":None,"R":None,"TXT":"not a verified converter"},t)

    def batch(self):
        return {"protocol":"oo.c1.typed-batch/v3","sequence":1,"kind":"cdc","previous_position":"0","position":"1",
                "parts":[{"table":"S.\u4e2d\u6587","path":"/a/one.arrow","sha256":"a"*64,"bytes":100,"rows":1,
                          "spec":{"columns":[{"name":"ID","oid":20,"typmod":-1,"nullable":False}],"keys":[0]}}]}

    def test_shared_digest_excludes_paths_and_uses_utf8(self):
        batch=self.batch()
        changed=copy.deepcopy(batch)
        changed["parts"][0]["path"]="/different/one.arrow"
        self.assertEqual(p.batch_digest(batch),p.batch_digest(changed))
        del changed["parts"][0]["path"]
        expected=hashlib.sha256(json.dumps(changed,ensure_ascii=False,sort_keys=True,separators=(",",":")).encode()).hexdigest()
        self.assertEqual(p.batch_digest(batch),expected)
        changed["position"]="2"
        self.assertNotEqual(p.batch_digest(batch),p.digest(changed))

    def test_only_exact_durable_control_ack_advances(self):
        batch=self.batch()
        context={"source_id":"oracle_fixture","lease_id":"real-task","lease_generation":3}
        ack={"durable":True,"receipt":{"protocol":"oo.c1.typed-ack/v3","source":"oracle_fixture",
             "task":"real-task","generation":3,"epoch":7,"sequence":1,"position":"1","digest":p.batch_digest(batch)}}
        p.validate_typed_ack(ack,batch,context,7)
        for key,value in [("protocol","oo.c1.typed-receipt/v3"),("epoch",6),("task","other"),("digest","b"*64)]:
            bad=copy.deepcopy(ack);bad["receipt"][key]=value
            with self.assertRaisesRegex(ValueError,"typed_ack"):
                p.validate_typed_ack(bad,batch,context,7)
        ack["durable"]=False
        with self.assertRaises(ValueError):
            p.validate_typed_ack(ack,batch,context,7)

    def test_ordered_protocol_is_an_explicit_cdc_dialect_and_changes_replay_digest(self):
        legacy = self.batch()
        ordered = copy.deepcopy(legacy)
        ordered["protocol"] = p.ORDERED_PROTOCOL
        self.assertTrue(p.batch_protocol_supported(ordered))
        self.assertNotEqual(p.batch_digest(legacy), p.batch_digest(ordered))
        ordered["kind"] = "fill"
        self.assertFalse(p.batch_protocol_supported(ordered))
        ordered["protocol"] = "unknown-protocol"
        self.assertFalse(p.batch_protocol_supported(ordered))

    def test_hash_path_and_symlink_guards(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"input";path.write_bytes(b"fixed")
            ref={"path":str(path),"sha256":b.sha(path)}
            self.assertEqual(p.pinned(ref),path)
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError,"digest"):
                p.pinned(ref)
            link=Path(tmp)/"link";link.symlink_to(path)
            with self.assertRaisesRegex(ValueError,"path"):
                p.pinned({"path":str(link),"sha256":b.sha(path)})

    def test_mysql_is_not_an_oracle_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError,"producer_config"):
                p.Exporter({"protocol":"oo.oracle.arrow-producer-config/v1","engine":"mysql"},Path(tmp)/"out")
            self.assertFalse((Path(tmp)/"out").exists())

    def test_materializer_cannot_relabel_oracle_as_mysql(self):
        request={"source_id":"oracle_fixture","entries":[{"request":{"source_engine":"oracle"}}]}
        p.oracle_materialized_request(request,"oracle_fixture")
        request["entries"][0]["request"]["source_engine"]="mysql"
        with self.assertRaisesRegex(ValueError,"cross_engine"):
            p.oracle_materialized_request(request,"oracle_fixture")

    def test_real_opaque_bytes_required_and_bound_separately_from_scn(self):
        with tempfile.TemporaryDirectory() as tmp:
            def ref(name,raw):
                path=Path(tmp)/name;path.write_bytes(raw)
                return {"path":str(path),"sha256":b.sha(path),"bytes":len(raw)}
            offsets={"offset_before":ref("before",b"unit opaque A"),"offset_after":ref("after",b"unit opaque B")}
            value=p.oracle_position("tx", "999999999999", 3, "a"*64, offsets)
            self.assertEqual(value["event_count"],3)
            self.assertEqual(value["commit_scn"],"999999999999")
            bad=copy.deepcopy(offsets);bad["offset_after"]["bytes"]+=1
            with self.assertRaisesRegex(ValueError,"size"):
                p.oracle_position("tx","7",3,"a"*64,bad)
            bad["offset_after"]=offsets["offset_before"]
            with self.assertRaisesRegex(ValueError,"not_advanced"):
                p.oracle_position("tx","7",3,"a"*64,bad)


if __name__=="__main__":
    unittest.main()
