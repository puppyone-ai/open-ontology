"""Real oo-load/Agent DB checks on this runner's disposable fixtures only."""
import json
import os
from pathlib import Path
import sys
import threading
import time
import subprocess

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/oo-test"))
from document_compare import compare_documents


def mongo_command(run, script):
    prefix = ('const fs=require("fs"); const a=db.getSiblingDB("admin");'
              'a.auth("admin",fs.readFileSync("/run/fixture/admin-password","utf8"));'
              'const f=db.getSiblingDB("fixture");')
    _, output = run.command(["docker", "exec", "-i", "oo-f-doc-" + run.id,
                             "mongosh", "--quiet"], prefix + script)
    return [json.loads(line.split("OO_DOCUMENT:", 1)[1]) for line in output.splitlines()
            if "OO_DOCUMENT:" in line and line.split("OO_DOCUMENT:", 1)[1].startswith("{")]


def source_rows(run, port):
    if run.family == "mongodb":
        exported = mongo_command(run, 'print("OO_DOCUMENT:"+EJSON.stringify('
                                 '{rows:f.events.find().toArray()},{relaxed:false}));')
        if len(exported) != 1:
            raise RuntimeError("MongoDB fixture export framing")
        records = exported[0]["rows"]
        rows = []
        for d in records:
            value = d.get("value")
            scalar = None if value is None else int(next(iter(value.values())))
            rows.append([json.dumps(d["_id"], separators=(",", ":"), sort_keys=True),
                         scalar, json.dumps(d)])
    else:
        response = run.request(port, "POST", "/events/_search",
                               {"size": 1000, "query": {"match_all": {}}})
        if response["hits"]["total"]["value"] != len(response["hits"]["hits"]):
            raise RuntimeError("fixture export truncated")
        rows = [[h["_id"], h["_source"].get("value"), json.dumps(h["_source"])]
                for h in response["hits"]["hits"]]
    return sorted(rows, key=lambda row: row[0])


def mutate(run, port, cycle):
    if run.family == "mongodb":
        if cycle == 0:
            mongo_command(run, 'f.events.updateOne({_id:0},{$set:{value:777}});'
                          'f.events.deleteOne({_id:198});'
                          'f.events.insertOne({_id:400,value:400});')
        else:
            mongo_command(run, 'f.events.updateOne({_id:100},{$set:{value:888}});')
    else:
        if cycle == 0:
            run.request(port, "POST", "/events/_update/0", {"doc": {"value": 777}})
            run.request(port, "DELETE", "/events/_doc/198")
            run.request(port, "PUT", "/events/_doc/400", {"key": "0400", "value": 400})
        else:
            run.request(port, "POST", "/events/_update/100", {"doc": {"value": 888}})
        run.request(port, "POST", "/events/_refresh")


def acceptance(run, port, ca, olap):
    binary = ROOT / "tools/oo-load/target/debug/oo-load"
    if not binary.is_file():
        raise RuntimeError("build oo-load document-sources before starting fixtures")
    target = olap.AgentDB(run)
    target.start()
    store = run.private
    olap.private(store / "source-password", run.secrets["reader"])
    source = {"id": "fixture", "engine": run.family, "host": "127.0.0.1",
              "port": port, "database": "fixture", "user": "reader",
              "password": {"secret_ref": "source-password"},
              "scope": {"include": ["fixture.events"], "exclude": []}}
    columns = [{"name": "value", "kind": "int64"}]
    if ca:
        source["tls_ca"] = {"secret_ref": "source-ca"}
        olap.private(store / "source-ca", Path(ca).read_text())
    env_name = "OO_MONGO_COLLECTION_CONFIG" if run.family == "mongodb" else "OO_SEARCH_INDEX_CONFIG"
    config = {"events": columns} if run.family == "mongodb" else {
        "events": {"columns": columns, "range_field": "key",
                   "boundaries": [f"{n:04}" for n in range(10, 200, 10)]}}
    olap.write_json(store / "columns.json", config)
    os.environ[env_name] = str(store / "columns.json")
    olap.write_json(store / "stack.json", {
        "version": "oo.stack/v1", "sources": [source],
        "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                  "model": "unused", "key": {"secret_ref": "unused"}, "context_window": "auto"}})
    base_target = {"host": "127.0.0.1", "port": target.port, "database": "postgres",
                   "user": "postgres", "password": {"secret_ref": "target-password"},
                   "local_fixture": True}
    report = {"T0": "not_accepted", "T1": "not_run", "T2": "not_run", "T8": "not_accepted",
              "C1": "not_accepted" if run.family == "mongodb" else "unsupported",
              "reason_T0": "product_doctor_not_wired",
              "reason_T8": "source_connection_peak_not_measured",
              "consistency": "S0", "fixture_rows": 200, "cycles": []}

    def save():
        olap.write_json(run.path / "product.json", report)

    def load(suffix, cycle=None):
        olap.write_json(store / "target.json", dict(base_target, run_id=run.id + suffix))
        argv = [str(binary), "--stack", str(store / "stack.json"), "--source", "fixture",
                "--target", str(store / "target.json"), "--secrets-dir", str(store),
                "--chunk-rows", "64"]
        if cycle:
            olap.write_json(store / "cycle.json", {"cycle": cycle, "full_read_every": 100})
            argv += ["--c5", str(store / "cycle.json")]
        completed = subprocess.run(argv, capture_output=True, timeout=600)
        (run.path / (suffix + ".stdout")).write_bytes(run.redact(completed.stdout))
        (run.path / (suffix + ".stderr")).write_bytes(run.redact(completed.stderr))
        error = run.redact(completed.stderr).decode()
        rows = [json.loads(line) for line in completed.stdout.decode().splitlines()
                if line.startswith("{")]
        if completed.returncode or len(rows) != 1:
            raise RuntimeError("product load failed: " + error[:300])
        return rows[0]

    def comparison(expected, relation):
        # Read jsonb as text so JSON numeric values never pass through binary float.
        raw = target.sql(f'SELECT json_build_array("_id","value","_document"::text)'
                         f' FROM {relation} ORDER BY "_id" COLLATE "C";')
        actual = [json.loads(line) for line in raw.splitlines() if line.startswith("[")]
        result = compare_documents(expected, actual, document_column=2)
        if not result["equal"]:
            olap.write_json(run.path / "difference.json", result)
            raise RuntimeError("Agent DB document comparison differs")
        types = target.sql(f"SELECT pg_typeof(_document)::text FROM {relation} LIMIT 1;").strip()
        if types != "jsonb":
            raise RuntimeError("document target is not jsonb")
        return result

    save()
    try:
        initial = source_rows(run, port)
        stop, started = threading.Event(), threading.Event()
        errors = []

        def writer():
            try:
                while not stop.wait(0.03):
                    installed = target.sql("SELECT to_regclass('_oo_load.chunks') IS NOT NULL;").strip()
                    if installed == "t" and int(target.sql("SELECT count(*) FROM _oo_load.chunks;").strip()):
                        started.set()
                        mutate(run, port, 0)
                        return
                raise RuntimeError("writer did not overlap snapshot")
            except Exception as error:
                errors.append(str(error))

        thread = threading.Thread(target=writer)
        thread.start()
        try:
            snapshot = load("-snapshot")
        finally:
            stop.set()
            thread.join(timeout=60)
        if thread.is_alive() or errors or not started.is_set():
            raise RuntimeError("concurrent writer failed: " + str(errors))
        report["snapshot"] = snapshot
        report["comparison"] = comparison(initial, snapshot["target"])
        report["T1"] = "passed"
        save()
        for cycle in range(1, 4):
            if cycle == 3:
                mutate(run, port, cycle)
            expected = source_rows(run, port)
            receipt = load("-c5-" + str(cycle), cycle)
            compared = comparison(expected, receipt["target"])
            if cycle == 2 and any(receipt[k] != 0 for k in
                                  ("payload_bytes", "source_read_bytes", "target_write_bytes",
                                   "new_generations", "ranges_replaced")):
                raise RuntimeError("idle C5 rewrote payload or generation")
            report["cycles"].append({"receipt": receipt, "comparison": compared})
            save()
        report["T2"] = "passed"
        report["C5"] = "passed"
    except Exception as error:
        report["failure"] = str(error)
    finally:
        save()
    return report
