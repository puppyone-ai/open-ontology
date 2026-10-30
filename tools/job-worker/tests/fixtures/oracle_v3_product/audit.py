#!/usr/bin/env python3
"""Independent published-reader reconciliation for the frozen extended buffer."""
from pathlib import Path
import hashlib
import json
import os
import re
import secrets
import sqlite3
import subprocess
import sys

RUN = Path(sys.argv[1])
CONTEXT = json.loads((RUN / "context.json").read_text())
DB = CONTEXT["database"]
PG = "oo-mysql-c1-20261027-pg17"
OUT = RUN / "published-reader"
OUT.mkdir(exist_ok=False)
PRIVATE = Path(CONTEXT["private_dir"])


def pg(label, sql, role="postgres", password=None, check=True):
    env = os.environ.copy()
    argv = ["docker", "exec", "-i"]
    if password:
        env["PGPASSWORD"] = password
        argv += ["-e", "PGPASSWORD"]
    argv += [PG, "psql", "-X", "-h", "127.0.0.1", "-U", role, "-d", DB, "-At", "-v", "ON_ERROR_STOP=1"]
    p = subprocess.run(argv, input=sql.encode(), env=env, capture_output=True)
    (OUT / (label + ".stdout")).write_bytes(p.stdout)
    (OUT / (label + ".stderr")).write_bytes(p.stderr.replace(password.encode(), b"[redacted]") if password else p.stderr)
    (OUT / (label + ".command.json")).write_text(json.dumps(dict(argv=argv, exit=p.returncode)) + "\n")
    if check:
        assert p.returncode == 0, label
    return p


def main():
    journal_path = os.environ.get("OO_ORACLE_FROZEN_JOURNAL")
    expected_rows = int(os.environ.get("OO_ORACLE_EXPECTED_ROWS", "10001"))
    expected_ordinal = os.environ.get("OO_ORACLE_EXPECTED_ORDINAL", "4")
    c5 = CONTEXT.get("method") == "C5"
    control_sql = """SELECT jsonb_build_object(
 'job_state',(SELECT state FROM jobs.tasks ORDER BY created_at DESC LIMIT 1),
 'pending_acks',(SELECT count(*) FROM jw_native.pending_acks WHERE acknowledged_at IS NULL),
 'acknowledged',(SELECT bool_and(acknowledged_at IS NOT NULL) FROM _oo.native_c1_baseline_typed),
 'engine',(SELECT engine FROM _oo.source_instance),
 'codec',(SELECT position_codec FROM _oo.source_watermark),
 'state',(SELECT state FROM _oo.source_watermark),
 'source_time_unknown',(SELECT source_committed_at IS NULL FROM _oo.source_watermark),
 'buffer_ordinal',(SELECT applied_pos::text FROM _oo.source_watermark),
 'published_runs',(SELECT count(*) FROM _oo.full_import_run WHERE state='published'));"""
    if c5:
        control_sql = control_sql.replace(
            "(SELECT bool_and(acknowledged_at IS NOT NULL) FROM _oo.native_c1_baseline_typed)",
            "(SELECT count(*)=1 FROM jw_native.watermarks w JOIN jobs.tasks j ON j.id=w.task_id WHERE j.state='succeeded')")
        control_sql = control_sql.replace("(SELECT engine FROM _oo.source_instance)",
                                          "(SELECT request->>'source_engine' FROM jw_native.occurrences)")
    state = json.loads(pg("control", control_sql).stdout)
    assert state["job_state"] == "succeeded" and state["pending_acks"] == 0 and state["acknowledged"]
    assert state["engine"] == "oracle" and state["codec"] == ("snapshot-zero/v1" if c5 else "oracle-capture-sequence/v1")
    assert state["state"] == "live" and state["published_runs"] == 1 and state["buffer_ordinal"] == expected_ordinal
    assert state["source_time_unknown"]
    physical = pg("physical", "SELECT physical_name FROM _oo.full_import_table").stdout.decode().strip()
    assert re.fullmatch(r"[a-z][a-z0-9_]{0,62}", physical)
    source = CONTEXT["source_id"]
    entry_sql = ("SELECT jsonb_build_object('schema',request->>'logical_schema','table',request->>'logical_table') FROM jw_native.occurrences"
                 if c5 else "SELECT receipt->'entries'->0 FROM _oo.native_c1_baseline_typed")
    entry = json.loads(pg("source-entry", entry_sql).stdout)
    schema, table = entry["schema"], entry["table"]
    assert all(re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,127}", s) for s in [schema, table])
    mapped = []
    for name in ["ID", "AMOUNT", "LABEL", "PAYLOAD"]:
        value = pg("mapping-" + name, f"SELECT _oo.lookup_physical_mapping(source_id,config_digest,source_generation,'column',_oo.c1_storage_ident('{schema}'),_oo.c1_storage_ident('{table}'),_oo.c1_storage_ident('{name}')) FROM _oo.full_import_run WHERE source_id='{source}'").stdout.decode().strip()
        assert re.fullmatch(r"[a-z][a-z0-9_]{0,62}", value)
        mapped.append('"' + value + '"')
    role = "oracle_read_" + DB.removeprefix("oracle_tail_").removeprefix("oracle_c5_")
    password = secrets.token_hex(24)
    p = PRIVATE / "published-reader.pw"
    p.write_text(password)
    p.chmod(0o600)
    pg("reader-scope", f"""CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT PASSWORD '{password}';
ALTER ROLE {role} SET default_transaction_read_only='on';
GRANT CONNECT ON DATABASE {DB} TO {role};
GRANT USAGE ON SCHEMA _oo TO {role};
GRANT SELECT ({','.join(mapped)}) ON _oo.\"{physical}\" TO {role};""")
    sql = f"SELECT json_build_array({mapped[0]}::text,{mapped[1]}::text,{mapped[2]}::text,encode({mapped[3]},'hex')) FROM _oo.\"{physical}\";"
    rows = [json.loads(line) for line in pg("published-read", sql, role, password).stdout.splitlines()]
    denied = pg("control-denied", "SELECT count(*) FROM jobs.tasks", role, password, False)
    assert denied.returncode != 0
    hidden = pg("hidden-position-denied", f'SELECT _oo_pos FROM _oo."{physical}" LIMIT 1', role, password, False)
    assert hidden.returncode != 0
    if journal_path:
        assert source == "oracle_frozen_iud_20261027" and table == "IUD_4000_4000_2000"
        path = Path(journal_path)
        assert hashlib.sha256(path.read_bytes()).hexdigest() == os.environ["OO_ORACLE_FROZEN_JOURNAL_SHA256"]
        journal_table = os.environ.get("OO_ORACLE_JOURNAL_TABLE", "baseline")
        assert journal_table in ("baseline", "final")
        with sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True) as db:
            expected = [json.loads(row[0]) for row in db.execute("SELECT row_json FROM " + journal_table + " ORDER BY id")]
    else:
        fixture = RUN.parent.parent / "oracle/c1-r2/independent-final-binary"
        manifest_raw = (fixture / "manifest.json").read_bytes()
        assert hashlib.sha256(manifest_raw).hexdigest() == "654ff8c3879faedd429f22140beacf0df87a34f698003f522c304e6fc73894be"
        manifest = json.loads(manifest_raw)
        expected = []
        for part in manifest["parts"]:
            raw = (fixture / part["file"]).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == part["sha256"]
            expected.extend(json.loads(raw)["rows"])
    rows.sort(key=lambda r: int(r[0]))
    expected.sort(key=lambda r: int(r[0]))
    assert rows == expected and len(rows) == expected_rows
    hashes = {}
    for name, value in [("source", expected), ("published-reader", rows)]:
        raw = json.dumps(value, separators=(",", ":")).encode()
        (OUT / (name + ".json")).write_bytes(raw)
        hashes[name] = hashlib.sha256(raw).hexdigest()
    (OUT / "result.json").write_text(json.dumps(dict(
        actual_regular_reader=True, rows=expected_rows, typed_differences=0, control_read_denied=True,
        method="C5" if c5 else "C1", buffer_ack_claimed=not c5,
        hashes=hashes, control=state, live_source_reconnect=False, oracle_server_contacted=False,
        source_matrix="frozen_iud independent journal" if journal_path else "retained 3000I/4000U/3000D + edges, not frozen_iud"), indent=2) + "\n")
    print(OUT)


if __name__ == "__main__":
    main()
