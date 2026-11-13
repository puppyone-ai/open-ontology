"""Owned PG replica D28 fixed/open pools against human-authored fixture gold."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/pg-builder/scripts"))
import adjudication
import library_selfbuild as lib
import measure_reconciliation
import retained_build
import d28
from library_observation import quote


def save(path, value):
    path.write_text(lib.canonical(value))
    return dict(path=path.name, sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--logistics", type=Path, required=True)
    parser.add_argument("--image", default="postgres:17.11-bookworm")
    parser.add_argument("--open-pool", action="store_true")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    private = out / ".private"
    private.mkdir(mode=0o700)
    password = private / "postgres-password"
    password.write_text(secrets.token_hex(32))
    password.chmod(0o600)
    name = "oo-g-d28-" + uuid.uuid4().hex[:12]
    created = False

    def run(argv, data=None):
        process = subprocess.run(argv, input=data, text=True, capture_output=True, timeout=180)
        if process.returncode:
            # All inputs here are public synthetic fixture SQL, never credentials.
            (out / "failure.log").write_text(process.stderr)
            raise RuntimeError("d28_command_failed")
        return process.stdout

    def admin(sql):
        return run(["docker", "exec", "-i", name, "psql", "-X", "-qAt", "-U", "postgres",
                    "-v", "ON_ERROR_STOP=1"], sql)

    try:
        run(["docker", "run", "--detach", "--name", name, "--label", "oo.lane=g",
             "--label", "oo.task=V3-G-02", "--mount",
             f"type=bind,src={private},dst=/run/d28,readonly",
             "-e", "POSTGRES_PASSWORD_FILE=/run/d28/postgres-password", args.image])
        created = True
        for _ in range(90):
            if subprocess.run(["docker", "exec", name, "pg_isready", "-h", "127.0.0.1", "-U", "postgres"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("d28_database_not_ready")
        save(out / "engine.json", {"version":admin("show server_version;").strip(),
                                   "image":args.image,"scope":"synthetic Agent DB replica"})
        spec = importlib.util.spec_from_file_location(
            "d28_seed", ROOT / "testing/engines/seed-common/generate.py")
        seed = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(seed)
        admin("CREATE SCHEMA t6; SET search_path TO t6;\n" + "".join(seed.generate("postgres", "17", 200, 0)))
        admin("CREATE SCHEMA logistics;")
        selected = {"kh", "dd", "yd", "yd_mx", "sj", "zd", "ht"}
        for side in ("mysql-erp", "postgres-tms"):
            directory = args.logistics / "source-inputs" / side
            catalog = json.loads((directory / "catalog.json").read_text())
            loads = {t["table"]:t for t in json.loads((directory / "load.json").read_text())["tables"]}
            for table in catalog["tables"]:
                if table["name"] not in selected:
                    continue
                fields = []
                for column in table["columns"]:
                    typ = column["sql"].lower()
                    if re.match(r"^(bigint|int|tinyint|smallint)", typ):
                        typ = "numeric(20,0)" if "unsigned" in typ else "bigint"
                    elif not re.match(r"^(numeric|decimal|varchar|char\b|text|date$)", typ):
                        typ = "text"
                    fields.append(quote(column["name"]) + " " + typ)
                physical = '"logistics".' + quote(table["name"])
                admin("CREATE TABLE " + physical + "(" + ",".join(fields) + ");")
                raw = (args.logistics / loads[table["name"]]["file"]).read_text()
                admin("COPY " + physical + " FROM STDIN WITH (FORMAT csv, HEADER true, NULL '\\N');\n"
                      + raw + "\\.\n")
        admin("CREATE ROLE d28_reader LOGIN; GRANT USAGE ON SCHEMA t6,logistics TO d28_reader; "
              "GRANT SELECT ON ALL TABLES IN SCHEMA t6,logistics TO d28_reader;")
        admin("CREATE TABLE t6.profile_probe(email text,phone text,id_card text,bank_card text,label text);"
              "INSERT INTO t6.profile_probe VALUES ('fixture@example.invalid','+1 202 555 0123',"
              "'110101199001010010','4111111111111111','Contact label');"
              "GRANT SELECT ON t6.profile_probe TO d28_reader;")
        db = lib.ReadOnlyPostgres(["docker", "exec", "-i", name, "psql", "-U", "d28_reader", "-d", "postgres"], 120000)
        probes = {}
        for field in ("email", "phone", "id_card", "bank_card", "label"):
            profile = adjudication.column_profile(db, {"physical":'"t6"."profile_probe"'},
                                                   {"physical_name":field})
            decision = adjudication.sensitive_decision(profile, True, adjudication.thresholds())[0]
            if decision != ("visible" if field == "label" else "hidden"):
                raise AssertionError("d28_profile_probe")
            probes[field] = dict(profile=profile, decision=decision)
        save(out / "profile-probes.json", probes)
        from d28_semantic import semantic_probes
        save(out / "semantic-probes.json", semantic_probes(admin, db))
        manifest = {"datasets":[]}
        for dataset, schema, names, pool in [
            ("T6", "t6", ["regions","areas","sites","codes","customers","orders","shipments",
                          "calendar","order_lines","events","log_202601","log_202602"], [
                ("areas","region_id","regions","id",True),
                ("sites","area_id","areas","id",True),
                ("customers","site_id","sites","id",True),
                ("orders","customer_id","customers","id",True),
                ("orders","status_code","codes","code",True),
                ("shipments","order_id","orders","id",True),
                ("order_lines","order_id","orders","id",True),
                ("orders","id","customers","id",False)]),
            ("OO-Logistics", "logistics", sorted(selected), [
                ("yd","kh_dm","kh","khid",True),
                ("yd","erp_ddh","dd","ddid",True),
                ("yd_mx","ydh","yd","ydh",True),
                ("yd","sfje","dd","je_fen",False)])]:
            tables = []
            for table in names:
                columns = db.query(
                    "select json_agg(json_build_object('name',column_name,'physical_name',column_name,"
                    "'data_type',data_type,'nullable',is_nullable='YES') order by ordinal_position)"
                    " from information_schema.columns where table_schema='" + schema + "' and table_name='" + table + "'")
                if schema == "t6":
                    keys = db.query("select coalesce(json_agg(a.attname order by u.ordinality),'[]'::json)"
                        " from pg_index i cross join lateral unnest(i.indkey) with ordinality u(n,ordinality)"
                        " join pg_attribute a on a.attrelid=i.indrelid and a.attnum=u.n"
                        " where i.indrelid='" + schema + "." + table + "'::regclass and i.indisprimary")
                else:
                    keys = next(t["pk"] for side in ("mysql-erp","postgres-tms")
                        for t in json.loads((args.logistics / "source-inputs" / side / "catalog.json").read_text())["tables"]
                        if t["name"] == table)
                tables.append(dict(dataset=schema+"."+table, physical=quote(schema)+"."+quote(table),
                                   columns=columns, primary_key=keys))
            graph = {"edges":[dict(id=f"candidate-{i}", **{"from":schema+"."+child,"to":schema+"."+parent},
                columns={"from":[col],"to":[key]}, provenance="containment",confidence=.5)
                for i,(child,col,parent,key,_) in enumerate(pool)]}
            command = retained_build.review_input(tables, {"graph":graph}, "d28")
            # A model-proposed SQL without a supported independent definition is
            # an explicit negative metric, not an automatically certified query.
            command["effective"]["metrics"] = [{"metric":"unchecked_total","label":"Unchecked total",
                                                "sql":"SELECT 1"}]
            if schema in {"t6", "logistics"}:
                measure = dict(metric="order_count", dataset=schema + (".orders" if schema == "t6" else ".dd"),
                               key="id" if schema == "t6" else "ddid",
                               scope={"status_code":"1"} if schema == "t6" else {"hb":"CNY"},
                               time_column="ordered_at" if schema == "t6" else "ddrq",
                               kind="count_distinct", components=[])
                sql, _ = measure_reconciliation.statements({"tables":tables}, measure)
                command["effective"]["metrics"].append(dict(
                    metric="order_count",label="Order count",sql=sql,unit="count",status="draft",
                    owner="fixture",default_anchor="month_start",
                    control_total=dict(as_of="month_start",source="fixture",sql=sql,tolerance={"abs":"0"}),
                    tests=[dict(params={},expect={"rows":1})]))
                command["adjudication_specs"] = {"metrics":{"order_count":dict(
                    measure=measure, windows=[{"start":"2026-01-01","end":"2026-02-01"},
                                             {"start":"2026-02-01","end":"2026-03-01"}])}}
            if schema == "logistics":
                command["adjudication_specs"]["code_tables"] = [
                    dict(id="customer_status", **{"from":dict(dataset="logistics.kh",column="zt"),
                        "to":dict(dataset="logistics.zd",column="zddm")}, scope={"zdlx":"khzt"}),
                    dict(id="customer_class", **{"from":dict(dataset="logistics.kh",column="khfl"),
                        "to":dict(dataset="logistics.zd",column="zddm")}, scope={"zdlx":"khfl"})]
                command["effective"]["metrics"].append(dict(command["effective"]["metrics"][-1],
                    metric="class_dependent_count", label="Count with incomplete dictionary"))
                command["adjudication_specs"]["metrics"]["class_dependent_count"] = dict(
                    command["adjudication_specs"]["metrics"]["order_count"],
                    measure=dict(command["adjudication_specs"]["metrics"]["order_count"]["measure"],
                                 metric="class_dependent_count"),
                    code_dependencies=["code_table/customer_class"])
            # This expanded gold comes from fixture schema and gold SQL, not
            # from containment results or the discovered decision set.
            gold_pairs = {(schema+"."+c, (col,), schema+"."+p, (key,))
                          for c, col, p, key, positive in pool if positive}
            if schema == "t6":
                gold_pairs.update((schema+"."+c, (col,), schema+"."+p, (key,))
                                  for c, col, p, key in [
                    ("shipments","site_id","sites","id"),
                    ("events","order_id","orders","id"),
                    ("log_202601","order_id","orders","id"),
                    ("log_202602","order_id","orders","id")])
            else:
                gold_pairs.update([
                    ("logistics.dd", ("khid",), "logistics.kh", ("khid",)),
                    ("logistics.ht", ("khid",), "logistics.kh", ("khid",)),
                    ("logistics.dd", ("htid","bb"), "logistics.ht", ("htid","bb")),
                    ("logistics.yd", ("sj_id",), "logistics.sj", ("sjid",))])
            if args.open_pool:
                adjudication.discover_candidates(command, tables)
            prefix = dataset.lower().replace("-", "_")
            input_ref = save(out / (prefix+"-input.json"), command)
            document = adjudication.collect(db, command, tables, input_ref["sha256"])
            labels = [{"id":f"relation/candidate-{i}", "class":"relation","positive":positive}
                      for i, (*_, positive) in enumerate(pool)]
            if args.open_pool:
                labels = []
                found = set()
                for edge in command["draft"]["graph"]["edges"]:
                    pair = (edge["from"], tuple(edge["columns"]["from"]),
                            edge["to"], tuple(edge["columns"]["to"]))
                    found.add(pair)
                    labels.append(dict(id="relation/"+edge["id"], **{
                        "class":"relation", "positive":pair in gold_pairs}))
                for pair in sorted(gold_pairs - found):
                    labels.append(dict(id="relation/missing-"+lib.digest(pair)[:24],
                                       **{"class":"relation", "positive":True}))
            # Ground truth is fixed from declared fixture keys and the named
            # semantic PII canaries, never copied from the observed decisions.
            for table in tables:
                labels.append({"id":"entity_key/"+table["dataset"],"class":"entity_key","positive":bool(table["primary_key"])})
                for c in table["columns"]:
                    pii = c["name"] in {"phone_synthetic","national_id_synthetic","lxdh","sfzh","sjhm"}
                    labels.append({"id":"sensitive/"+table["dataset"]+"."+c["name"],"class":"sensitive","positive":pii})
            labels.append({"id":"metric/unchecked_total","class":"metric","positive":False})
            labels.append({"id":"metric/order_count","class":"metric","positive":True})
            if schema == "t6":
                labels.append({"id":"code_table/candidate-4","class":"code_table","positive":True})
            else:
                labels.extend([
                    {"id":"code_table/customer_status","class":"code_table","positive":True},
                    {"id":"code_table/customer_class","class":"code_table","positive":False},
                    {"id":"metric/class_dependent_count","class":"metric","positive":False}])
            if args.open_pool:
                for edge in command["draft"]["graph"]["edges"]:
                    if edge["id"].startswith("candidate-"):
                        continue
                    parent = next(t for t in tables if t["dataset"] == edge["to"])
                    if parent["primary_key"] == edge["columns"]["to"] and any(
                            "code" in n for n in edge["columns"]["to"]):
                        pair = (edge["from"], tuple(edge["columns"]["from"]),
                                edge["to"], tuple(edge["columns"]["to"]))
                        labels.append(dict(id="code_table/"+edge["id"], **{
                            "class":"code_table", "positive":pair in gold_pairs}))
            gold = dict(protocol="oo.d28-gold/v1", dataset=dataset, items=labels,
                        scope="open key-like pairs" if args.open_pool else "fixed candidate pool")
            declared = db.query(
                "select coalesce(json_agg(json_build_object('child',n.nspname||'.'||c.relname,"
                "'parent',pn.nspname||'.'||p.relname,'columns',"
                "(select json_agg(a.attname order by u.i) from unnest(f.conkey) with ordinality u(k,i)"
                " join pg_attribute a on a.attrelid=c.oid and a.attnum=u.k),'keys',"
                "(select json_agg(a.attname order by u.i) from unnest(f.confkey) with ordinality u(k,i)"
                " join pg_attribute a on a.attrelid=p.oid and a.attnum=u.k))),'[]'::json)"
                " from pg_constraint f join pg_class c on c.oid=f.conrelid"
                " join pg_namespace n on n.oid=c.relnamespace join pg_class p on p.oid=f.confrelid"
                " join pg_namespace pn on pn.oid=p.relnamespace where f.contype='f' and n.nspname="
                + lib.literal(schema))
            declared_pairs = {(f["child"],tuple(f["columns"]),f["parent"],tuple(f["keys"]))
                              for f in declared}
            baseline = dict(protocol="oo.d28-gold-baseline/v1", gold_sha256=d28.digest(gold),
                evidence_run_id=out.name, review_mode="human_authored_gold",
                sources=["testing/engines/gold/postgres.sql", "testing/engines/seed-common/generate.py"]
                        if schema == "t6" else ["bench/datagen/src/catalog.rs", "bench/datagen/src/rows.rs"],
                l1_review="not usable: l1-final-evidence.json records counts, not per-relation decisions",
                adopted_relations=["relation/"+e["id"] for e in command["draft"]["graph"]["edges"]
                    if (e["from"],tuple(e["columns"]["from"]),e["to"],tuple(e["columns"]["to"])) in declared_pairs])
            manifest["datasets"].append(dict(name=dataset,
                adjudication=save(out / (prefix+"-adjudication.json"),document),
                gold=save(out / (prefix+"-gold.json"),gold),
                baseline=save(out / (prefix+"-baseline.json"),baseline)))
            # Compile the actual Python output with the Rust consumer.
            overrides = dict(protocol="oo.adjudication-override/v1",
                             summary_sha256=document["summary_sha256"],items={})
            envelope = dict(adjudication_input=(out / input_ref["path"]).read_text(),
                            adjudication=document,overrides=overrides,
                            confirmation=dict(protocol="oo.publish-confirm/v1",
                                summary_sha256=document["summary_sha256"],overrides_sha256=lib.digest(overrides),
                                reason="Synthetic D28 integration",confirmed_at="2026-11-09T12:00:00Z"))
            compiled = run([str(ROOT / "target/debug/selfbuild-budget")],
                           lib.canonical({"operation":"reviewed","command":envelope}))
            save(out / (prefix+"-compiled.json"), json.loads(compiled))
        save(out / "manifest.json", manifest)
        return 0
    finally:
        if created:
            # Only the randomly named container created by this invocation.
            subprocess.run(["docker","rm","--force","--volumes",name],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)


if __name__ == "__main__":
    raise SystemExit(main())
