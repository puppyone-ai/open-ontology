"""T6 product path: reuse the retained Builder, owner-review and publisher."""
import json
import sys

from runner import ROOT, write_json
from t1 import private
from t6 import BUILDER_CALL_LIMIT, MODEL, load_module
from t6_fixture import GoldFixture


class ReviewedGoldFixture(GoldFixture):
    def __init__(self, out, engine, scale):
        sys.path.insert(0, str(ROOT / "tools/release-coordinator/tests"))
        product = load_module(
            "t6_retained_product", ROOT / "tools/release-coordinator/tests/retained_cycle_live.py")
        super().__init__(out, engine, scale)
        self.product = product
        self.builder_call_limit = BUILDER_CALL_LIMIT
        self.model_stack = self.root / "model-stack.json"
        # References only: no endpoint/key material is copied into this file.
        private(self.model_stack, json.dumps({"model": MODEL}))

    def publish(self, run):
        self.prepare(run)
        self.db.sql("CREATE ROLE oo_r_owner NOLOGIN NOINHERIT; "
                    "GRANT oo_r_owner TO oo_f22_login WITH INHERIT FALSE, SET TRUE;")
        report, output = self.product.Retained.builder(self, run)
        write_json(self.out / "builder-result.json", report)
        if report["stage"] != "review_required" or not 1 <= report["model_calls"] <= BUILDER_CALL_LIMIT:
            raise ValueError("BUILDER_NOT_REVIEWABLE")
        reviewed = self.product.Retained.review(self, output)
        code, receipt = self.product.Retained.publisher(self, run, reviewed, "gold")
        if code or receipt["outcome"] != "activated" or receipt["state"] != "ready":
            raise ValueError("REVIEWED_PUBLICATION_FAILED")
        document = receipt["gateway_document"]
        visibility = document["visibility"]
        self.identity = {
            "project_id": visibility["project"], "principal_id": visibility["principal"],
            "role": visibility["role"], "policy_version": visibility["policy"]["version"],
            "knowledge_version": visibility["knowledge"]["version"]}
        self.artifact = document["envelope"]["producer"]["artifact"]
        path = self.runtime / "reviewed/library/library.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        self.fixture.replace(path, self.fixture.encode(document))
        write_json(self.out / "publication-result.json", {
            "state": receipt["state"], "outcome": receipt["outcome"],
            "receipt_sha256": receipt["receipt_sha256"], "pack_digest": receipt["pack_digest"],
            "fences": [f["outcome"] for f in receipt["fences"]]})
        return receipt
