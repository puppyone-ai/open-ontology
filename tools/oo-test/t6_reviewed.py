"""T6 product path: reuse the retained Builder, owner-review and publisher."""
import json
import sys

from runner import ROOT, write_json
from t1 import private
from t6 import MAX_CALLS, MODEL, load_module
from t6_fixture import GoldFixture


def completeness(facts, draft, reviewed):
    """Check every retained table/column, including inherited family members."""
    import selfbuild_budget

    items = []
    for table in facts["tables"]:
        for kind, column in [("table", None)] + [("column", c["name"]) for c in table["columns"]]:
            key = selfbuild_budget.stable_id(kind, table["dataset"], column)
            value = draft.get("accepted", {}).get(key, {})
            resolved = (value.get("id") == key and value.get("kind") == kind and
                        all(isinstance(value.get(k), str) and value[k].strip()
                            for k in ("label", "description")))
            items.append({"id": key, "dataset": table["dataset"], "column": column,
                          "resolved": bool(resolved), "inherited_from": value.get("inherited_from")})
    expected = {t["dataset"]: {c["name"] for c in t["columns"]} for t in facts["tables"]}
    actual = {t["dataset"]: {c["name"] for c in t["columns"]}
              for t in reviewed.get("compiled", {}).get("effective", {}).get("datasets", [])}
    review_ok = (reviewed.get("status") == "reviewed" and
                 reviewed.get("review_decision", {}).get("mode") == "self" and actual == expected)
    return {"status": "passed" if items and all(i["resolved"] for i in items)
            and not draft.get("unresolved", [True]) and review_ok else "failed",
            "scope": "retained gold-question tables, not full source fixture",
            "tables": len(expected), "columns": sum(map(len, expected.values())),
            "items": items, "unresolved": draft.get("unresolved"),
            "reviewed_shape_matches": review_ok, "connectivity": draft.get("review")}


class ReviewedGoldFixture(GoldFixture):
    def __init__(self, out, engine, scale, version=None, worker_image=None):
        sys.path.insert(0, str(ROOT / "tools/release-coordinator/tests"))
        product = load_module(
            "t6_retained_product", ROOT / "tools/release-coordinator/tests/retained_cycle_live.py")
        super().__init__(out, engine, scale, version, worker_image)
        self.product = product
        self.model_stack = self.root / "model-stack.json"
        # References only: no endpoint/key material is copied into this file.
        private(self.model_stack, json.dumps({"model": MODEL}))

    def plan_builder(self, run):
        import retained_build
        import selfbuild_budget

        db = retained_build.lib.ReadOnlyPostgres(
            ["docker", "exec", "-i", self.db.name, "psql", "-U", "postgres"], 30000)
        facts, _ = retained_build.facts_for(db, {
            "library_id": "owned", "generation": run, "source_engine": self.engine})
        tables = retained_build.inventory(facts)
        runner = selfbuild_budget.Runner(
            {"model": MODEL["model"], "request_timeout_seconds": 300},
            self.out / "builder-preflight")
        graph = selfbuild_budget.bridge("graph", budget=runner.budget, tables=tables, candidates=[])
        delta = selfbuild_budget.bridge("incremental", tables=tables, graph=graph, previous={})
        estimate = runner.preflight(graph, delta["work"], {t["dataset"]: t for t in tables})
        self.set_builder_budget(estimate)

    def set_builder_budget(self, estimate):
        batches = estimate["batches"]
        # Runner.batch allows the initial attempt plus two missing-item repairs.
        if type(batches) is not int:
            raise ValueError("BUILDER_PREFLIGHT_BUDGET")
        limit = batches * 3
        if not 1 <= limit <= 4096:
            raise ValueError("BUILDER_PREFLIGHT_BUDGET")
        self.builder_call_limit = limit
        path = self.out / "preflight.json"
        preflight = json.loads(path.read_text())
        preflight.update(builder_batches=batches, builder_repair_calls=batches * 2,
                         builder_call_limit=limit, max_total_model_calls=MAX_CALLS + limit,
                         builder_budget_basis="retained Builder preflight batches + two repairs per batch")
        write_json(path, preflight)
        print(json.dumps(preflight), flush=True)

    def publish(self, run):
        self.prepare(run)
        self.plan_builder(run)
        self.db.sql("CREATE ROLE oo_r_owner NOLOGIN NOINHERIT; "
                    "GRANT oo_r_owner TO oo_f22_login WITH INHERIT FALSE, SET TRUE;")
        report, output = self.product.Retained.builder(self, run)
        write_json(self.out / "builder-result.json", report)
        if report["stage"] != "review_required" or not 1 <= report["model_calls"] <= self.builder_call_limit:
            raise ValueError("BUILDER_NOT_REVIEWABLE")
        reviewed = self.product.Retained.review(self, output)
        t5 = completeness(
            json.loads((output / "facts.json").read_text()),
            json.loads((output / "draft.json").read_text()), json.loads(reviewed))
        write_json(self.out / "t5-result.json", t5)
        if t5["status"] != "passed":
            raise ValueError("BUILDER_INCOMPLETE")
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
