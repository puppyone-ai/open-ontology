#!/usr/bin/env python3
"""Owned target-only recovery using the frozen 10000-row Oracle input.

No Oracle socket, source mutation, capture process, or source checkpoint write.
The only injected losses are target receipt/worker ACK delivery on one real task.
"""
import argparse
import contextlib
import decimal
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import sqlite3
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[5]
PG = "oo-mysql-c1-20261027-pg17"
SOURCE = "oracle_frozen_iud_20261027"


def read(path):
    return json.loads(Path(path).read_text())


def ref(path):
    path = Path(path).resolve()
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "bytes": path.stat().st_size}


def save(path, value):
    with Path(path).open("x") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
    Path(path).chmod(0o600)


def run(out, label, argv, data=None, check=True, env=None, session=False):
    prefix = out / (label + "-" + str(time.time_ns()))
    started = time.monotonic()
    proc = subprocess.run(list(map(str, argv)), input=data, capture_output=True,
                          env=env, start_new_session=session, timeout=240)
    for suffix, raw in (("stdout", proc.stdout), ("stderr", proc.stderr)):
        Path(str(prefix) + "." + suffix).write_bytes(raw)
    save(Path(str(prefix) + ".json"), {"argv": list(map(str, argv)), "exit": proc.returncode,
                                     "seconds": time.monotonic() - started})
    if check and proc.returncode:
        raise RuntimeError(f"{label}: exit{proc.returncode}; preserved {prefix}")
    return proc


def pg(out, label, database, sql, check=True, role="postgres", password=None):
    env = os.environ.copy()
    argv = ["docker", "exec", "-i"]
    if password:
        env["PGPASSWORD"] = password
        argv += ["-e", "PGPASSWORD"]
    return run(out, label, [*argv, PG, "psql", "-X", "-h", "127.0.0.1", "-U", role,
                           "-d", database, "-At", "-v", "ON_ERROR_STOP=1"], sql.encode(),
               check=check, env=env)


def prepare(args):
    out = args.out.resolve()
    out.mkdir(mode=0o700, exist_ok=False)
    pins = read(args.pins)
    for value in pins.values():
        assert ref(value["path"])["sha256"] == value["sha256"]
    config = read(args.source_config)
    index = read(config["buffer_index"]["path"])
    manifest = read(config["bundle"]["path"])
    assert manifest["final_rows"] == 10000 and not index["transactions"]
    assert manifest["boundary"]["final_position"] == "0"
    preserved = [config["bundle"], config["buffer_index"], index["context"]]
    context = read(index["context"]["path"])
    preserved += [context["raw_spool"], *context["source_resume"].values()]
    preserved = [v for v in preserved if isinstance(v, dict) and "path" in v]
    for value in preserved:
        assert ref(value["path"])["sha256"] == value["sha256"]
    save(out / "source-pins.json", preserved)
    for key, name in (("helper", "producer.py"), ("bounded_source", "bounded_source.py"),
                      ("buffer_cursor", "buffer_cursor.py"), ("rollover_module", "rollover.py")):
        config[key] = ref(ROOT / "tools/oracle-preview" / name)
    config["materializer"] = {**pins["oo-pipeline"], "args": ["native-oracle-c1-materialize"]}
    wrappers(out, pins)
    code = out / "source-code"
    for key, name in (("helper", "producer.py"), ("bounded_source", "bounded_source.py"),
                      ("buffer_cursor", "buffer_cursor.py"), ("rollover_module", "rollover.py")):
        config[key] = ref(code / name)
    save(out / "prepared-input.json", config)

    # Reuse existing target deploy/bootstrap; stop before it can run a job.
    os.environ.update(OO_C1_INTEGRATION_EVIDENCE=str(out), OO_ORACLE_TAIL_PINS=str(args.pins.resolve()),
                      OO_ORACLE_PRODUCER_CONFIG=str(out / "prepared-input.json"))
    os.environ.pop("OO_ORACLE_PREPARE_ONLY", None)
    module_path = Path(__file__).with_name("run.py")
    spec = importlib.util.spec_from_file_location("oracle_initial_owned_bootstrap", module_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    old_run = module.run
    class Prepared(Exception):
        pass
    def bootstrap_run(name, *a, **kw):
        if name == "native-put":
            raise Prepared()
        return old_run(name, *a, **kw)
    module.run = bootstrap_run
    try:
        module.main()
    except Prepared:
        pass
    target_context = read(module.OUT / "context.json")
    assert target_context["database"].startswith("oracle_tail_")
    save(out / "target.json", target_context)
    save(out / "pins.json", pins)
    save(out / "base-context.json", ref(module.OUT / "context.json"))
    save(out / "gold.json", ref(args.gold))
    # Installation is explicit for independent SQL testing even before the
    # shared installer hook is merged. Existing targets are never upgraded.
    migration = ROOT / "crates/oo-pipeline/migrations/0097_native_c1_oracle_fill_resume.sql"
    exists = pg(out, "check-0097", target_context["database"],
                "SELECT to_regprocedure('_oo.c1_oracle_fill_open(text,uuid,bigint,integer,jsonb,bigint)') IS NOT NULL;")
    if exists.stdout.strip() == b"f":
        pg(out, "install-0097-new-owned-only", target_context["database"],
           "BEGIN;\n" + migration.read_text() + "\nCOMMIT;")
    save(out / "migration-pin.json", ref(migration))
    schedule = read(target_context["manifest_path"])
    schedule["loader"] = {**{k: v for k, v in ref(out / "loader-loss.py").items() if k != "bytes"},
                          "args": ["import-native-c1-baseline-v3"]}
    schedule["producer"].pop("bytes", None)
    save(out / "manifest.json", schedule)
    print(json.dumps({"prepared": True, "target": ref(out / "target.json"),
                      "database": target_context["database"], "source_connected": False}))


def wrappers(out, pins):
    pipeline = pins["oo-pipeline"]["path"]
    code = out / "source-code"
    code.mkdir(mode=0o700)
    for name in ("bounded_source.py", "buffer_cursor.py", "rollover.py"):
        shutil.copyfile(ROOT / "tools/oracle-preview" / name, code / name)
    shutil.copyfile(ROOT / "tools/oracle-preview/producer.py", code / "producer_impl.py")
    loader = f"""#!{sys.executable}
import json,os,signal,subprocess,sys
from pathlib import Path
raw=sys.stdin.buffer.read();request=json.loads(raw)
p=subprocess.run([{pipeline!r},*sys.argv[1:]],input=raw,capture_output=True)
if p.returncode==0 and request.get('operation')=='apply' and request.get('batch',{{}}).get('kind')=='fill' and request['batch']['sequence']==4:
    try:
        with Path({str(out/'fault-before-ack.json')!r}).open('x') as f:
            json.dump({{'receipt':json.loads(p.stdout),'request':request}},f);f.flush();os.fsync(f.fileno())
        os.killpg(os.getpgrp(),signal.SIGKILL)
    except FileExistsError:pass
sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)
"""
    proxy = f"""#!{sys.executable}
from producer_impl import *
import json,os,signal,subprocess,sys,threading
from pathlib import Path
def main():
 p=subprocess.Popen([sys.executable,'-B',{str(code/'producer_impl.py')!r},*sys.argv[1:]],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=sys.stderr.buffer)
 def copy_in():
    for raw in sys.stdin.buffer:
        ack=json.loads(raw);tag=None
        if ack.get('stage')=='typed_batch' and ack.get('receipt',{{}}).get('sequence')==8:tag='after-ack'
        if ack.get('stage')=='snapshot_end':tag='table-complete'
        if ack.get('stage')=='end':tag='sealed-end'
        if tag:
            try:
                with (Path({str(out)!r})/('fault-'+tag+'.json')).open('x') as f:
                    json.dump(ack,f);f.flush();os.fsync(f.fileno())
                os.killpg(os.getpgrp(),signal.SIGKILL)
            except FileExistsError:pass
        p.stdin.write(raw);p.stdin.flush()
    p.stdin.close()
 threading.Thread(target=copy_in,daemon=True).start()
 for raw in p.stdout:
    sys.stdout.buffer.write(raw);sys.stdout.buffer.flush()
 sys.exit(p.wait())
if __name__=='__main__':main()
"""
    for path, text in ((out / "loader-loss.py", loader), (code / "producer.py", proxy)):
        path.write_text(text)
        path.chmod(0o700)


def state(out):
    context = read(out / "target.json")
    database = context["database"]
    sql = f"""SELECT jsonb_build_object(
      'tasks',(SELECT jsonb_agg(jsonb_build_object('id',id,'state',state,'generation',generation,
        'attempt',attempt,'run_at',run_at,'lease_until',lease_until)) FROM jobs.tasks),
      'runs',(SELECT jsonb_agg(jsonb_build_object('task',r.task_id,'state',r.state,'generation',r.lease_generation,
        'source_epoch',r.source_epoch,'l0',r.l0,'l_end',r.l_end::text,'checkpoint',c.seq,
        'max_receipt',(SELECT coalesce(max(seq),0) FROM _oo.c1_typed_receipt WHERE source=r.source AND task=r.task_id),
        'receipts',(SELECT count(*) FROM _oo.c1_typed_receipt WHERE source=r.source AND task=r.task_id),
        'tables',(SELECT jsonb_object_agg(name,snapshot_complete) FROM _oo.c1_typed_table WHERE source=r.source AND task=r.task_id)))
        FROM _oo.native_c1_baseline_run r LEFT JOIN _oo.c1_typed_checkpoint c ON c.source=r.source AND c.task=r.task_id
        WHERE r.source='{SOURCE}'),
      'pending_acks',(SELECT count(*) FROM jw_native.pending_acks WHERE acknowledged_at IS NULL),
      'watermarks',(SELECT jsonb_agg(to_jsonb(w)) FROM _oo.source_watermark w));"""
    result = json.loads(pg(out, "state", database, sql).stdout)
    save(out / ("state-" + str(time.time_ns()) + ".json"), result)
    return result


def step(out, start=False, manifest=None):
    pins, context = read(out / "pins.json"), read(out / "target.json")
    def worker(label, *argv):
        return run(out, label, [pins["job-worker"]["path"], "--config", context["worker_config"], *argv],
                   check=False, session=label == "step")
    if start:
        assert worker("put", "native-put", "--manifest", manifest or out / "manifest.json").returncode == 0
        assert worker("fire", "native-fire", "--project", context["project"]).returncode == 0
        assert worker("disable", "native-enable", "--binding", context["binding"], "--enabled", "false").returncode == 0
    result = worker("step", "native-step", "--project", context["project"], "--worker", "oracle-initial-fill-owned")
    current = state(out)
    print(json.dumps({"exit": result.returncode, "actual": current}), flush=True)
    return result, current


def publish(out):
    pins, context = read(out / "pins.json"), read(out / "target.json")
    worker, loader = pins["job-worker"]["path"], pins["oo-pipeline"]["path"]
    artifacts = out / "artifacts"
    run(out, "artifacts", [worker, "--config", context["worker_config"], "native-artifacts",
                           "--binding", context["binding"], "--out", artifacts])
    ack = read(artifacts / "ack.json")
    assert ack["acknowledged"] and ack["task_state"] == "succeeded" and ack["source_generation"] == 1
    assert ack["applied_pos"] == "0"
    handoff = {"worker_config": context["worker_config"], "control": {"database": context["database"]},
               "pins": {"loader": pins["oo-pipeline"], "admission": pins["oo-source-admission"]}}
    save(out / "runtime-handoff.json", handoff)
    save(out / "publisher.json", pins["oo-pipeline"])
    run(out, "export-requests", [sys.executable, ROOT / "tools/job-worker/tests/fixtures/c1_v3_product/prepare_export.py",
                                 "--artifacts", artifacts, "--runtime-handoff", out / "runtime-handoff.json",
                                 "--publisher", out / "publisher.json", "--out", out / "exports"])
    run(out, "collection", [sys.executable, ROOT / "tools/source-onboarding/export_native_collection.py",
                            "--operator", out / "exports/collection-operator.json", "--out", out / "collection.json"])
    run(out, "publish", [loader, "native-c1-publication"],
        json.dumps(read(out / "exports/publication-request.json")).encode())
    loaded = read(artifacts / "load.json")
    entry = loaded["entries"][0]
    assert len(loaded["entries"]) == 1 and entry["row_count"] == 10000
    columns = []
    for name in ("ID", "AMOUNT", "LABEL", "PAYLOAD"):
        value = pg(out, "column-" + name, context["database"], f"""SELECT _oo.lookup_physical_mapping(
          source_id,config_digest,source_generation,'column',_oo.c1_storage_ident('{entry["schema"]}'),
          _oo.c1_storage_ident('{entry["table"]}'),_oo.c1_storage_ident('{name}'))
          FROM _oo.full_import_run WHERE run_id='{entry["run_id"]}';""").stdout.decode().strip()
        assert value.startswith("pc_") and value.replace("_", "").isalnum()
        columns.append('"' + value + '"')
    reader, password = "ora_fill_" + secrets.token_hex(6), secrets.token_hex(24)
    pwfile = out / "reader.password"
    pwfile.write_text(password)
    pwfile.chmod(0o600)
    role_sql = (f"CREATE ROLE {reader} LOGIN NOSUPERUSER NOINHERIT NOBYPASSRLS PASSWORD '{password}';"
                f"ALTER ROLE {reader} SET default_transaction_read_only='on';"
                f"GRANT CONNECT ON DATABASE {context['database']} TO {reader};"
                f"GRANT USAGE ON SCHEMA _oo TO {reader};")
    pg(out, "reader-create", context["database"], role_sql)
    physical = entry["physical_id"]
    assert physical.startswith("pt_") and physical.replace("_", "").isalnum()
    relation = '_oo."' + physical + '"'
    query = (f"SELECT json_build_array({columns[0]}::text,{columns[1]}::text,{columns[2]},"
             f"encode({columns[3]},'hex')) FROM {relation};")
    assert pg(out, "pregrant-denied", context["database"], query, False, reader, password).returncode != 0
    pg(out, "reader-grant", context["database"], f"GRANT SELECT ({','.join(columns)}) ON {relation} TO {reader};")
    rows = [json.loads(raw) for raw in pg(out, "reader-rows", context["database"], query, True, reader, password).stdout.splitlines()]
    gold = read(out / "gold.json")
    assert ref(gold["path"]) == gold
    with contextlib.closing(sqlite3.connect(f"file:{gold['path']}?mode=ro&immutable=1", uri=True)) as db:
        expected = [json.loads(raw) for raw, in db.execute("SELECT row_json FROM baseline")]
    norm = lambda row: (int(row[0]), decimal.Decimal(row[1]), row[2], bytes.fromhex(row[3]))
    assert len(rows) == len(expected) == len({row[0] for row in rows}) == 10000
    assert sorted(map(norm, rows)) == sorted(map(norm, expected))
    for label, query in (("hidden", f"SELECT _oo_pos FROM {relation}"), ("control", "SELECT * FROM jobs.tasks")):
        assert pg(out, label + "-denied", context["database"], query, False, reader, password).returncode != 0
    observed = state(out)
    task = observed["tasks"][0]
    assert len(observed["tasks"]) == 1 and task["state"] == "succeeded"
    r = observed["runs"][0]
    assert r["checkpoint"] == r["receipts"] == r["max_receipt"] == 40
    assert observed["pending_acks"] == 0 and observed["watermarks"][0]["state"] == "live"
    for pin in read(out / "source-pins.json"):
        assert ref(pin["path"])["sha256"] == pin["sha256"]
    save(out / "ACCEPTANCE.json", {"rows": 10000, "typed_cells": 40000, "differences": 0,
         "same_task": task["id"], "lease_generation": task["generation"], "checkpoint": 40,
         "source_generation": 1, "target_generation": loaded["target_generation"],
         "ordinary_reader": reader, "pregrant_hidden_control_denied": True, "published": True,
         "faults": {name: ref(out / ("fault-" + name + ".json"))
                   for name in ("before-ack", "after-ack", "table-complete", "sealed-end")},
         "source_pins": ref(out / "source-pins.json"), "gold": gold, "source_connected": False,
         "source_recaptured": False, "source_initial_snapshot_recovery_accepted": False,
         "scope": "TARGET immutable initial fill recovery only"})


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("action", choices=["prepare", "start", "step", "state", "publish"])
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--pins", type=Path)
    ap.add_argument("--source-config", type=Path)
    ap.add_argument("--gold", type=Path)
    ap.add_argument("--manifest", type=Path)
    args = ap.parse_args()
    if args.action == "prepare":
        prepare(args)
    elif args.action in ("start", "step"):
        step(args.out, args.action == "start", args.manifest)
    elif args.action == "state":
        print(json.dumps(state(args.out)))
    else:
        publish(args.out)
