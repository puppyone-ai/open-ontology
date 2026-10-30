#!/usr/bin/env python3
"""New initial-source fixture: reconcile a published ordinary reader with pinned gold.

Reuses the existing Oracle audit's target-only executor; never opens Oracle.
"""
import hashlib
import json
from pathlib import Path
import re
import secrets
import sqlite3
import sys

import audit


def main():
    expected_receipt = json.loads(Path(sys.argv[2]).read_text())
    source = audit.CONTEXT["source_id"]
    assert source == "oracle_initial_resume_20261028"
    gold = expected_receipt["gold"]
    assert hashlib.sha256(Path(gold["path"]).read_bytes()).hexdigest() == gold["sha256"]
    state = json.loads(audit.pg("control", """SELECT jsonb_build_object(
 'job_state',(SELECT state FROM jobs.tasks ORDER BY created_at DESC LIMIT 1),
 'pending_acks',(SELECT count(*) FROM jw_native.pending_acks WHERE acknowledged_at IS NULL),
 'acknowledged',(SELECT bool_and(acknowledged_at IS NOT NULL) FROM _oo.native_c1_baseline_typed),
 'engine',(SELECT engine FROM _oo.source_instance),
 'codec',(SELECT position_codec FROM _oo.source_watermark),
 'state',(SELECT state FROM _oo.source_watermark),
 'buffer_ordinal',(SELECT applied_pos::text FROM _oo.source_watermark),
 'published_runs',(SELECT count(*) FROM _oo.full_import_run WHERE state='published'));""").stdout)
    assert state == {"job_state": "succeeded", "pending_acks": 0, "acknowledged": True,
                     "engine": "oracle", "codec": "oracle-capture-sequence/v1",
                     "state": "live", "buffer_ordinal": "1", "published_runs": 1}
    physical = audit.pg("physical", "SELECT physical_name FROM _oo.full_import_table").stdout.decode().strip()
    assert re.fullmatch(r"[a-z][a-z0-9_]{0,62}", physical)
    entry = json.loads(audit.pg("entry", "SELECT receipt->'entries'->0 FROM _oo.native_c1_baseline_typed").stdout)
    assert entry["schema"] == "OO_SNAP28_01" and entry["table"] == "SNAPSHOT_ROWS"
    mapped = []
    for name in ("ID", "AMOUNT", "LABEL", "PAYLOAD"):
        value = audit.pg("mapping-"+name, f"""SELECT _oo.lookup_physical_mapping(
 source_id,config_digest,source_generation,'column',_oo.c1_storage_ident('OO_SNAP28_01'),
 _oo.c1_storage_ident('SNAPSHOT_ROWS'),_oo.c1_storage_ident('{name}'))
 FROM _oo.full_import_run WHERE source_id='{source}'""").stdout.decode().strip()
        assert re.fullmatch(r"[a-z][a-z0-9_]{0,62}", value)
        mapped.append('"'+value+'"')
    role, password = "ora_source_read_"+secrets.token_hex(8), secrets.token_hex(24)
    secret = audit.PRIVATE/"initial-source-reader.pw"
    with secret.open("x") as f:
        f.write(password)
    secret.chmod(0o600)
    audit.pg("reader-scope", f"""CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT PASSWORD '{password}';
ALTER ROLE {role} SET default_transaction_read_only='on';
GRANT CONNECT ON DATABASE {audit.DB} TO {role};
GRANT USAGE ON SCHEMA _oo TO {role};
GRANT SELECT ({','.join(mapped)}) ON _oo."{physical}" TO {role};""")
    read = f"""SELECT json_build_array({mapped[0]}::text,{mapped[1]}::text,
{mapped[2]}::text,encode({mapped[3]},'hex')) FROM _oo."{physical}";"""
    actual = [json.loads(line) for line in audit.pg("published-read", read, role, password).stdout.splitlines()]
    denied = audit.pg("control-denied", "SELECT count(*) FROM jobs.tasks", role, password, False)
    hidden = audit.pg("hidden-position-denied", f'SELECT _oo_pos FROM _oo."{physical}" LIMIT 1',
                      role, password, False)
    assert denied.returncode != 0 and hidden.returncode != 0
    with sqlite3.connect(f"file:{gold['path']}?mode=ro&immutable=1", uri=True) as db:
        expected = [json.loads(row[0]) for row in db.execute("SELECT row_json FROM final ORDER BY id")]
    actual.sort(key=lambda row: int(row[0]))
    assert actual == expected and len(actual) == expected_receipt["rows"] == 10001
    cells = sum(len(row) for row in actual)
    assert cells == expected_receipt["typed_cells"] == 40004
    hashes = {}
    for name, rows in (("gold", expected), ("ordinary-reader", actual)):
        raw = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
        (audit.OUT/(name+".json")).write_bytes(raw)
        hashes[name] = hashlib.sha256(raw).hexdigest()
    result = {"actual_regular_reader": True, "rows": len(actual), "typed_cells": cells,
              "differences": 0, "source_id": source, "snapshot_scn": expected_receipt["snapshot_scn"],
              "cut_scn": expected_receipt["cut_scn"], "publication": True, "control": state,
              "source_contacts": 0, "control_read_denied": True, "hidden_position_denied": True,
              "gold": gold, "hashes": hashes, "source_initial_undo_expiry_claimed": False,
              "reader_role": role, "database": audit.DB}
    (audit.OUT/"result.json").write_text(json.dumps(result, sort_keys=True, indent=2)+"\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
