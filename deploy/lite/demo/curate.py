#!/usr/bin/env python3
"""Build the public fixture's curated inputs and Packs with the real compiler.

No model transport and no accept-all owner-review command. The named curator
is a fixture declaration, not a claim of human or business approval.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
CURATOR = "public-demo-curator"


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()

def adjudication_summary(graph, items):
    return dict(graph=graph, decisions={item["id"]: item["decision"] for item in items})


def stable(kind, table, column=None):
    return kind + ":" + digest([table, column])


# Counts and relationships are checked against seed.sql by the live fixture test.
TABLES = {
    "regions": (2, ["id"], "id:bigint name:text"),
    "areas": (4, ["id"], "id:bigint region_id:bigint name:text"),
    "sites": (8, ["id"], "id:bigint area_id:bigint name:text"),
    "codes": (3, ["code"], "code:bigint label:text"),
    "customers": (24, ["id"], "id:bigint site_id:bigint name:text phone_synthetic:text"),
    "orders": (120, ["id"], "id:bigint customer_id:bigint status_code:bigint region_id:bigint amount:numeric qty:bigint ordered_at:timestamp"),
    "shipments": (90, ["id"], "id:bigint order_id:bigint site_id:bigint shipped_at:timestamp"),
    "calendar": (31, ["day_id"], "day_id:bigint day_date:date month_no:bigint"),
    "order_lines": (240, ["order_id", "line_no"], "order_id:bigint line_no:bigint qty:bigint"),
    "events": (20000, ["id"], "id:bigint order_id:bigint event_no:bigint"),
}
RELATIONS = [
    ("areas", "region_id", "regions", "id"),
    ("sites", "area_id", "areas", "id"),
    ("customers", "site_id", "sites", "id"),
    ("orders", "customer_id", "customers", "id"),
    ("orders", "status_code", "codes", "code"),
    ("orders", "region_id", "regions", "id"),
    ("shipments", "order_id", "orders", "id"),
    ("shipments", "site_id", "sites", "id"),
    ("order_lines", "order_id", "orders", "id"),
    ("events", "order_id", "orders", "id"),
]


def inventory():
    return [
        dict(dataset="demo." + name, primary_key=key, comment=None, foreign_keys=[],
             columns=[dict(name=c.split(":")[0], data_type=c.split(":")[1],
                           nullable=False, comment=None) for c in columns.split()])
        for name, (_, key, columns) in TABLES.items()
    ]


def generate(compiler, out):
    def bridge(operation, **kwargs):
        result = subprocess.run([str(compiler)], input=canonical(dict(operation=operation, **kwargs)),
                                text=True, capture_output=True, timeout=120, check=True)
        return json.loads(result.stdout)

    def save(name, value):
        (out / name).write_text(canonical(value) + "\n")

    tables = inventory()
    candidates = [dict(provenance="declared_fk", confidence=1,
                       **{"from": dict(dataset="demo." + child, columns=[col]),
                          "to": dict(dataset="demo." + parent, columns=[key])})
                  for child, col, parent, key in RELATIONS]
    graph = bridge("graph", tables=tables, candidates=candidates)
    entities = [dict(id=stable("entity", t["dataset"]), canonical_name=t["dataset"],
                     aliases=[], main_table=t["dataset"], related_tables=[t["dataset"]],
                     primary_key=t["primary_key"]) for t in tables]
    registry = bridge("registry", registry={"entities": {}}, entities=entities,
                      inventory=[t["dataset"] for t in tables])["registry"]
    accepted = {}
    for table in tables:
        for kind, name in [("table", None)] + [("column", c["name"]) for c in table["columns"]]:
            id_ = stable(kind, table["dataset"], name)
            accepted[id_] = dict(id=id_, kind=kind, label=name or table["dataset"],
                                 description="Public synthetic logistics fixture; not customer data.")
    for edge in graph["edges"]:
        accepted[edge["id"]] = dict(id=edge["id"], kind="relation", decision="proposed",
                                    name="Declared fixture reference", cardinality="many_to_one")
    report = bridge("review", graph=graph, registry=registry,
                    mapped=[e["id"] for e in graph["edges"]], rejections={}, core=[],
                    orphan_threshold=0.2)
    draft = dict(protocol="oo.selfbuild.community-draft/v1", graph=graph, registry=registry,
                 accepted=accepted, unresolved=[], review=report, published=False,
                 model_calls=0, provenance="curated_local_fixture")
    versions = dict(project="demo", ontology_version="demo-v1", catalog_version="demo-v1",
                    policy=dict(id="demo", version="demo-v1"),
                    knowledge=dict(id="demo", version="demo-v1"))
    effective = dict(versions, protocol="oo.builder.effective-oos/v1",
        ontology=dict(spec_version="oos/v1", name="oo-logistics-demo", locale="en-US",
                      include_schemas=["demo"], default_tier_preference=["file"],
                      publish=dict(include_unreviewed=[]), classification=dict(auto_accept="none"),
                      lifecycle=dict(deprecation_min_releases=2),
                      compile=dict(per_role=True, budgets=dict(readme_tokens=2000, table_file_tokens=3000),
                                   sample_values=dict(default="masked", max_per_column=0))),
        datasets=[dict(schema_version="oos/v1", dataset=t["dataset"], tier="file",
                       label=t["dataset"], primary_key=t["primary_key"],
                       columns=[dict(name=c["name"], data_type=c["data_type"],
                                     pii="personal" if c["name"] == "phone_synthetic" else "none",
                                     classification=dict(state="confirmed", by=CURATOR),
                                     review=dict(state="approved", by=CURATOR))
                                for c in t["columns"]]) for t in tables],
        entities=[], relationships=[], metrics=[], examples=[], enums=[], glossary=[])
    decisions = {name: dict(decision="approved", reviewer=CURATOR,
                           summary_sha256=report["summary_sha256"])
                 for name in report["summary"]["naming_conflicts"]}
    if report["summary"]["orphan_review_required"]:
        decisions["orphan_entities"] = dict(decision="approved", reviewer=CURATOR,
                                            summary_sha256=report["summary_sha256"])
    for role in ("analyst", "ops"):
        visibility = dict(versions, protocol="oo.builder.role-visibility/v1", principal=role,
                          role="oo_r_" + role, issued_for_compiler="oo-builder/role-pack/v1",
                          datasets=[dict(dataset=t["dataset"],
                                         columns=[c["name"] for c in t["columns"] if c["name"] != "phone_synthetic"],
                                         row_security="rls" if t["dataset"] == "demo.orders" else "none",
                                         global_stats="denied") for t in tables],
                          limits=dict(max_file_bytes=262144, max_total_bytes=1048576, max_entries=128))
        command = dict(protocol="oo.builder.reviewed-community/v1", draft=draft, effective=effective,
                       visibility=visibility, orphan_threshold=0.2, ceilings={},
                       classification_review=dict(review_origin="operator_pinned_file",
                                                  project="demo", role="oo_r_" + role, principal=role),
                       review=dict(reviewer=CURATOR, reviewed_at="2026-11-09", mode="self",
                                   review_origin="operator_pinned_file",
                                   draft_sha256=digest(draft), effective_sha256=digest(effective),
                                   summary_sha256=report["summary_sha256"], decisions=decisions,
                                   entities={e["id"]: dict(key=e["primary_key"][0] if len(e["primary_key"]) == 1 else e["primary_key"],
                                                          display=e["primary_key"][0]) for e in entities}))
        compiled = bridge("reviewed", command=command)
        save("reviewed-" + role + ".json", command)
        save("pack-" + role + ".json", compiled["producer"]["artifact"])
    items, edges = [], []
    for child, col, parent, key in RELATIONS:
        id_ = "relation/" + child + "." + col
        items.append(dict(id=id_, **{"class": "relation"},
                          subject={"from": "demo." + child + "." + col, "to": "demo." + parent + "." + key},
                          decision="adopted", evidence=dict(containment=1, distinct_containment=1,
                          parent_unique=True, max_fanout=1, normalized_collisions=0, unmatched_rows=0,
                          empty_rows=0, kind="declared_fk", role=None, from_columns=[col], to_columns=[key],
                          cardinality="many_to_one", rows_checked=TABLES[child][0], basis="seed.sql declared foreign key"),
                          scores=dict(confidence=1), rule_id="relation.containment_strict",
                          reason="Every synthetic child key has a declared, seeded parent."))
        edges.append(dict(relation=id_, **{"from": "entity/" + child, "to": "entity/" + parent},
                          cardinality="many_to_one", adopted=True))
    for name, (rows, key, _) in TABLES.items():
        items.append(dict(id="entity_key/" + name, **{"class": "entity_key"},
                          subject=dict(table="demo." + name, columns=key), decision="adopted",
                          evidence=dict(unique_ratio=1, non_null_ratio=1, rows_checked=rows,
                                        basis="seed.sql primary key"), scores=dict(confidence=1),
                          rule_id="entity_key.unique_non_null", reason="Declared synthetic primary key."))
    items.append(dict(id="sensitive/customers.phone_synthetic", **{"class": "sensitive"},
                      subject=dict(column="demo.customers.phone_synthetic"), decision="hidden",
                      evidence=dict(basis="explicit fixture deny declaration", rows_checked=24),
                      scores=dict(confidence=1), rule_id="sensitive.fixture_declared",
                      reason="Synthetic stand-in for a private contact field; denied for both demo roles."))
    items.append(dict(id="code_table/codes", **{"class": "code_table"},
                      subject=dict(table="demo.codes"), decision="complete",
                      evidence=dict(coverage=1, rows_checked=120), scores=dict(confidence=1),
                      rule_id="code.coverage", reason="All three seeded order statuses have labels."))
    public_graph = dict(entities=[dict(id="entity/" + name, label=name, table="demo." + name,
                                      key=key, display=key[0], rows=rows)
                                 for name, (rows, key, _) in TABLES.items()], edges=edges)
    save("adjudication.json", dict(protocol="oo.adjudication/v1",
         input_sha256=hashlib.sha256((out / "reviewed-analyst.json").read_bytes()).hexdigest(),
         thresholds=dict(relation_containment_strict=0.98, relation_containment_with_signal=0.9, code_coverage=0.99),
         items=items, discovery=dict(status="complete", budget=len(edges), checked=len(edges), remaining=0,
                                     unchecked=[]),
         graph=public_graph, summary_sha256=digest(adjudication_summary(public_graph, items))))
    save("gold.json", [
        dict(id="Q01", question="How many orders can this role see?",
             sql="SELECT COUNT(*) AS value FROM demo.orders", expected={"analyst": [["60"]], "ops": [["60"]]}),
        dict(id="Q02", question="What is the exact total amount of those orders?",
             sql="SELECT SUM(amount) AS value FROM demo.orders",
             expected={"analyst": [["3517.5000"]], "ops": [["3757.5000"]]}),
        dict(id="Q16", question="How many January dates are in the demo calendar?",
             sql="SELECT COUNT(*) AS value FROM demo.calendar WHERE month_no=1",
             expected={"analyst": [["31"]], "ops": [["31"]]}),
        dict(id="DENY01", question="Show customer phone numbers (must be denied).",
             sql="SELECT phone_synthetic FROM demo.customers", denied=True),
    ])
    files = ["seed.sql", "roles.yaml", "reviewed-analyst.json", "reviewed-ops.json", "pack-analyst.json",
             "pack-ops.json", "adjudication.json", "gold.json"]
    save("manifest.json", dict(protocol="oo.demo-assets/v1", local_fixture=True, model_calls=0,
         review_basis="public synthetic fixture curation; not business approval",
         files={name: hashlib.sha256((out / name).read_bytes()).hexdigest() for name in files}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    generate(args.compiler.resolve(), HERE)
