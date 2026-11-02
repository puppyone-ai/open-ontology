#!/usr/bin/env python3
"""Resume a real target-committed, unacknowledged Oracle transaction.

Uses an isolated, already published baseline and immutable capture files.
Fault injection drops one successful Loader response, not source data or ACKs.
"""
import argparse
import decimal
import hashlib
import json
import os
from pathlib import Path
import shutil
import secrets
import sqlite3
import subprocess
import sys
import time
import uuid


def read(path):
    return json.loads(Path(path).read_text())


def save(path, value):
    with Path(path).open("x") as f:
        json.dump(value, f, indent=2)
        f.write("\n")
    Path(path).chmod(0o600)


def ref(path):
    path = Path(path).resolve()
    return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                bytes=path.stat().st_size)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", type=Path, required=True)
    ap.add_argument("--baseline", type=Path, required=True)
    ap.add_argument("--baseline-artifacts", type=Path, required=True)
    ap.add_argument("--baseline-collection", type=Path)
    ap.add_argument("--pins", type=Path, required=True)
    ap.add_argument("--source-config", type=Path, required=True)
    ap.add_argument("--successor-index", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--gold", type=Path)
    ap.add_argument("--gold-sha256", default="cc320388be06e20d0a5c521d6892425ec1a8a9079e532d93d84797d206701393")
    ap.add_argument("--expected-rows", type=int, default=12000)
    ap.add_argument("--fault", choices=["reply_drop", "worker_crash", "final_reply_drop"], default="reply_drop")
    ap.add_argument("--action", choices=["prepare", "step", "inspect", "publish-audit"], required=True)
    a = ap.parse_args()
    root, out = a.root.resolve(), a.out.resolve()
    context = read(a.baseline)
    assert context["source_mutated"] is False
    assert context["database"].startswith("oracle_tail_")
    pins = read(a.pins)
    for pin in pins.values():
        assert ref(pin["path"])["sha256"] == pin["sha256"]
    if a.action == "prepare":
        out.mkdir(mode=0o700, exist_ok=False)
    commands = []

    def run(label, argv, value=None, check=True):
        stamp = time.time_ns()
        start = time.monotonic()
        p = subprocess.run(list(map(str, argv)), input=value, capture_output=True,
                           start_new_session=label == "step")
        for suffix, buf in [("stdout", p.stdout), ("stderr", p.stderr)]:
            f = out/f"{label}-{stamp}.{suffix}"
            f.write_bytes(buf)
            f.chmod(0o600)
        result = dict(label=label, argv=list(map(str, argv)), exit=p.returncode,
                      seconds=round(time.monotonic()-start, 3), evidence=str(out/f"{label}-{stamp}"))
        commands.append(result)
        save(out/f"{label}-{stamp}.json", result)
        if check and p.returncode:
            raise RuntimeError(f"{label}: exit {p.returncode}; evidence retained")
        return p

    def worker(label, *args, check=True):
        return run(label, [pins["job-worker"]["path"], "--config", context["worker_config"], *args], check=check)

    def inspect():
        source = context["source_id"]
        assert source.replace("_", "").isalnum()
        sql = f"""SELECT jsonb_build_object(
          'tasks',(SELECT jsonb_agg(jsonb_build_object('id',id,'state',state,
            'generation',generation,'attempt',attempt,'run_at',run_at,'lease_until',lease_until)) FROM jobs.tasks),
          'runs',(SELECT jsonb_agg(jsonb_build_object('task',r.task_id,'state',r.state,
            'generation',r.lease_generation,'l0',r.l0,'l0_pos',r.l0_pos::text,
            'last_ack',c.seq,'receipts',(SELECT count(*) FROM _oo.c1_typed_receipt
             WHERE source=r.source AND task=r.task_id)))
            FROM _oo.native_c1_baseline_run r LEFT JOIN _oo.c1_typed_checkpoint c
            ON c.source=r.source AND c.task=r.task_id WHERE r.source='{source}'),
          'pending_acks',(SELECT count(*) FROM jw_native.pending_acks WHERE acknowledged_at IS NULL),
          'watermark',(SELECT jsonb_build_object('source_generation',source_generation,
            'target_generation',target_generation,'pos',applied_pos::text,'state',state)
            FROM _oo.source_watermark WHERE source='{source}'));"""
        p = run("state", ["docker", "exec", "-i", "oo-mysql-c1-20261027-pg17",
            "psql", "-X", "-U", "postgres", "-d", context["database"], "-At",
            "-v", "ON_ERROR_STOP=1"], sql.encode())
        state = json.loads(p.stdout)
        print(json.dumps(state))
        return state

    if a.action == "prepare":
        pipeline = pins["oo-pipeline"]["path"]
        ack = read(a.baseline_artifacts/"ack.json")
        # Real baseline publication, preserving the original collection and ACK.
        handoff = out/"baseline-handoff.json"
        save(handoff, dict(worker_config=context["worker_config"],
            control=dict(database=context["database"]),
            pins=dict(loader=pins["oo-pipeline"], admission=pins["oo-source-admission"])))
        publisher = out/"publisher.json"
        save(publisher, pins["oo-pipeline"])
        exports = out/"baseline-export"
        run("prepare-export", [sys.executable, root/"tools/job-worker/tests/fixtures/c1_v3_product/prepare_export.py",
            "--artifacts", a.baseline_artifacts, "--runtime-handoff", handoff, "--publisher", publisher, "--out", exports])
        if a.baseline_collection:
            collection = read(a.baseline_collection)
            assert collection["protocol"] == "oo.pipeline.sealed-import-collection/v1"
            assert all(entry["source_id"] == ack["source_id"] and
                       entry["source_generation"] == ack["source_generation"] and
                       entry["target_generation"] == ack["target_generation"]
                       for entry in collection["entries"])
            shutil.copyfile(a.baseline_collection, out/"baseline-collection.json")
            save(out/"retained-baseline-collection.json", ref(a.baseline_collection))
        else:
            run("sealed-collection", [sys.executable, root/"tools/source-onboarding/export_native_collection.py",
                "--operator", exports/"collection-operator.json", "--out", out/"baseline-collection.json"])
        request = read(exports/"publication-request.json")
        run("baseline-publish", [pipeline, "native-c1-publication"], json.dumps(request).encode())
        config = read(a.source_config)
        sys.path.insert(0, str(root/"tools/oracle-preview"))
        import rollover
        suffix = rollover.prepare(config["buffer_index"], ref(a.successor_index),
                                  ref(a.baseline_artifacts/"ack.json"), out/"suffix")
        code = out/"source-code"
        code.mkdir(mode=0o700)
        for key, filename in [("helper", "producer.py"), ("bounded_source", "bounded_source.py"),
                              ("buffer_cursor", "buffer_cursor.py"), ("rollover_module", "rollover.py")]:
            source = root/"tools/oracle-preview"/filename
            shutil.copyfile(source, code/filename)
            (code/filename).chmod(0o600)
            config[key] = ref(code/filename)
        for key in ("rollover", "buffer_index", "bundle"):
            config[key] = suffix[key]
        config["materializer"] = dict(path=pipeline, sha256=pins["oo-pipeline"]["sha256"],
                                      args=["native-oracle-c1-materialize"])
        save(out/"producer-config.json", config)
        marker = out/"dropped-committed-reply.json"
        wrapper = out/"drop-once-loader.py"
        wrapper.write_text(f"""#!{sys.executable}
import json, os, signal, subprocess, sys
from pathlib import Path
raw=sys.stdin.buffer.read()
request=json.loads(raw)
p=subprocess.run([{pipeline!r},*sys.argv[1:]],input=raw,capture_output=True)
selected=(request.get('operation')=='apply' and request.get('batch',{{}}).get('kind')=='cdc')
if {a.fault!r}=='final_reply_drop':
    selected=sys.argv[1:] == ['import-native-c1-baseline-v3']
if p.returncode==0 and selected:
    marker=Path({str(marker)!r})
    try:
        with marker.open('x') as f:
            f.write(p.stdout.decode()); f.flush(); os.fsync(f.fileno())
        marker.chmod(0o600)
        if {a.fault!r}=='worker_crash':
            os.killpg(os.getpgrp(),signal.SIGKILL)
        sys.stderr.write('injected loss after target commit, before typed ACK\\n')
        sys.exit(97)
    except FileExistsError:
        pass
sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)
""".replace("before typed ACK\n", "before typed ACK\\n"))
        wrapper.chmod(0o700)
        manifest = read(context["manifest_path"])
        manifest["binding"] = str(uuid.uuid4())
        manifest["loader"] = {**{k: ref(wrapper)[k] for k in ("path", "sha256")},
                              "args": ["import-native-c1-baseline-v3"]}
        manifest["producer"]["args"] = ["c1-baseline-stream-v3", "--config", str(out/"producer-config.json"),
                                       "--out", str(out/"capture-{task}-{generation}")]
        manifest["producer"].pop("bytes", None)
        save(out/"manifest.json", manifest)
        save(out/"context.json", dict(context, binding=manifest["binding"], baseline_ack_sha256=ref(a.baseline_artifacts/"ack.json")["sha256"],
             source_generation=ack["source_generation"]+1, fault=a.fault,
             source_reconnect=False, source_mutation=False))
        worker("put", "native-put", "--manifest", str(out/"manifest.json"))
        worker("fire", "native-fire", "--project", manifest["project_id"])
        worker("disable", "native-enable", "--binding", manifest["binding"], "--enabled", "false")
    elif a.action == "step":
        manifest = read(out/"manifest.json")
        result = worker("step", "native-step", "--project", manifest["project_id"],
                        "--worker", "oracle-recovery-owned", check=False)
        inspect()
        print("step_exit="+str(result.returncode))
    elif a.action == "publish-audit":
        assert a.gold is not None
        assert ref(a.gold)["sha256"] == a.gold_sha256
        recovery_context = read(out/"context.json")
        expected_generation = recovery_context.get("source_generation", read(a.baseline_artifacts/"ack.json")["source_generation"]+1)
        audit = out/"published-audit"
        audit.mkdir(mode=0o700, exist_ok=True)
        manifest = read(out/"manifest.json")
        artifacts = audit/"artifacts"
        if not artifacts.exists():
            worker("artifacts", "native-artifacts", "--binding", manifest["binding"], "--out", str(artifacts))
        ack = read(artifacts/"ack.json")
        loaded = read(artifacts/"load.json")
        assert ack["acknowledged"] and ack["task_state"] == "succeeded"
        assert ack["receipt"] == loaded and ack["source_generation"] == expected_generation
        exports = audit/"exports"
        if not exports.exists():
            run("prepare-export", [sys.executable, root/"tools/job-worker/tests/fixtures/c1_v3_product/prepare_export.py",
                "--artifacts", artifacts, "--runtime-handoff", out/"baseline-handoff.json",
                "--publisher", out/"publisher.json", "--out", exports])
        if not (audit/"collection.json").exists():
            run("sealed-collection", [sys.executable, root/"tools/source-onboarding/export_native_collection.py",
                "--operator", exports/"collection-operator.json", "--out", audit/"collection.json"])
        run("publish", [pins["oo-pipeline"]["path"], "native-c1-publication"],
            json.dumps(read(exports/"publication-request.json")).encode())

        def pg(label, sql, role="postgres", password=None, check=True):
            argv = ["docker", "exec", "-i"]
            # Password goes only to the owned container environment, never argv.
            prior = os.environ.get("PGPASSWORD")
            try:
                if password:
                    os.environ["PGPASSWORD"] = password
                    argv += ["-e", "PGPASSWORD"]
                return run(label, [*argv, "oo-mysql-c1-20261027-pg17", "psql", "-X", "-h", "127.0.0.1",
                    "-U", role, "-d", context["database"], "-At", "-v", "ON_ERROR_STOP=1"],
                    sql.encode(), check=check)
            finally:
                if prior is None:
                    os.environ.pop("PGPASSWORD", None)
                else:
                    os.environ["PGPASSWORD"] = prior

        entry = loaded["entries"][0]
        assert len(loaded["entries"]) == 1 and entry["row_count"] == a.expected_rows
        columns = []
        for name in ["ID", "AMOUNT", "LABEL", "PAYLOAD"]:
            columns.append(pg("mapping-"+name, f"""SELECT _oo.lookup_physical_mapping(
             source_id,config_digest,source_generation,'column',
             _oo.c1_storage_ident('{entry["schema"]}'),_oo.c1_storage_ident('{entry["table"]}'),
             _oo.c1_storage_ident('{name}')) FROM _oo.full_import_run WHERE run_id='{entry["run_id"]}';""").stdout.decode().strip())
        assert all(c.startswith("pc_") and c.replace("_", "").isalnum() for c in columns)
        assert entry["physical_id"].startswith("pt_") and entry["physical_id"].replace("_", "").isalnum()
        quoted = ['"'+c+'"' for c in columns]
        relation = '_oo."'+entry["physical_id"]+'"'
        reader = "ora_rec_"+secrets.token_hex(6)
        password = secrets.token_hex(24)
        pg("reader-create", f"CREATE ROLE {reader} LOGIN NOSUPERUSER NOINHERIT NOBYPASSRLS PASSWORD '{password}';"
           f"ALTER ROLE {reader} SET default_transaction_read_only='on';"
           f"GRANT CONNECT ON DATABASE {context['database']} TO {reader};GRANT USAGE ON SCHEMA _oo TO {reader};")
        query = f"SELECT json_build_array({quoted[0]}::text,{quoted[1]}::text,{quoted[2]},encode({quoted[3]},'hex')) FROM {relation};"
        denied = pg("pregrant", query, reader, password, False)
        assert denied.returncode != 0 and b"permission denied for table" in denied.stderr
        pg("reader-grant", f"GRANT SELECT ({','.join(quoted)}) ON {relation} TO {reader};")
        rows = [json.loads(line) for line in pg("rows", query, reader, password).stdout.splitlines()]
        with sqlite3.connect(f"file:{a.gold.resolve()}?mode=ro&immutable=1", uri=True) as db:
            gold = [json.loads(r[0]) for r in db.execute("SELECT row_json FROM final")]
        decimal.getcontext().prec = 50
        normalize = lambda row: (int(row[0]), None if row[1] is None else decimal.Decimal(row[1]),
                                 row[2], None if row[3] is None else bytes.fromhex(row[3]))
        assert len(rows) == len(gold) == len({r[0] for r in rows}) == a.expected_rows
        assert sorted(map(normalize, rows)) == sorted(map(normalize, gold))
        for label, sql in [("hidden", f"SELECT _oo_pos FROM {relation}"),
                           ("control", "SELECT * FROM jw_native.source_progress")]:
            assert pg(label, sql, reader, password, False).returncode != 0
        state = inspect()
        run_state = next(r for r in state["runs"] if r["task"] == ack["receipt"]["source_instance"]["lease_id"])
        transactions = len(read(out/"suffix/index.json")["transactions"])
        assert run_state["last_ack"] == run_state["receipts"] == transactions
        assert state["pending_acks"] == 0 and state["watermark"]["state"] == "live"
        marker = read(out/"dropped-committed-reply.json")
        if recovery_context.get("fault", "reply_drop") != "final_reply_drop":
            assert marker["protocol"] == "oo.c1.typed-receipt/v3" and marker["replay"] is False
            assert marker["task"] == run_state["task"] and marker["generation"] < run_state["generation"]
            exact = pg("replayed-receipt", f"""SELECT digest FROM _oo.c1_typed_receipt
             WHERE source='{ack["source_id"]}' AND task='{run_state["task"]}' AND seq={marker["sequence"]};""")
            assert exact.stdout.decode().strip() == marker["digest"]
        save(audit/"receipt.json", dict(passed=True, case=recovery_context.get("fault", "reply_drop"),
             rows=a.expected_rows, typed_cells=a.expected_rows*4, differences=0, checkpoint=transactions, receipts=transactions,
             task=run_state["task"], lease_generation=run_state["generation"],
             source_generation=expected_generation, target_generation=loaded["target_generation"], independent_gold=ref(a.gold),
             marker=ref(out/"dropped-committed-reply.json"), actual_state=state,
             hidden_position_denied=True, control_denied=True, ordinary_reader=reader,
             source_mutations=0, source_connected=False, gateway_tested=False))
    else:
        inspect()


if __name__ == "__main__":
    main()
