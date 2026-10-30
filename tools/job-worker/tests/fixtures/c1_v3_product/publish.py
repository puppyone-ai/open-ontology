#!/usr/bin/env python3
"""Publish an already control-ACKed owned run through the existing C1 SQL.

This checks data publication and a fixture-scoped read login. It does not
manufacture OOS, Pack, business approval, or release-coordinator activation.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys

RUN = Path(sys.argv[1])
LOADER = Path(sys.argv[2])
CTX = json.loads((RUN / "context.json").read_text())
HOME = Path("/tmp/oo-mysql-c1-20261027")
PG = "oo-mysql-c1-20261027-pg17"
MYSQL = "oo-mysql-c1-20261027-mysql57"
OUT = RUN / "publication"
OUT.mkdir(exist_ok=False)
PRIVATE = Path(CTX["private_dir"])


def pg(sql):
    p = subprocess.run(["docker", "exec", "-i", PG, "psql", "-X", "-U", "postgres",
                        "-d", CTX["database"], "-At", "-v", "ON_ERROR_STOP=1"],
                       input=sql.encode(), capture_output=True)
    assert p.returncode == 0, p.stderr.decode()
    return p.stdout.decode().strip()


def main():
    task = pg("SELECT id FROM jobs.tasks WHERE state='succeeded'")
    assert re.fullmatch(r"[a-f0-9-]{36}", task)
    result = json.loads(pg(f"SELECT result FROM jw_native.occurrences WHERE task_id='{task}'"))
    target = dict(host="127.0.0.1", port=25487, database=CTX["database"],
                  runtime_principal="postgres", password_file=str(HOME / "secrets/pg-super.pw"),
                  ca_file=str(HOME / "certs/ca.pem"))
    request = dict(protocol="oo.pipeline.native-c1-publication/v1", operation="publish",
                   source_id=CTX["source_id"], task_id=task,
                   request_digest=result["request_digest"], target=target)
    # Wrong digest refuses without changing publication.
    bad = dict(request, request_digest="0" * 64)
    p = subprocess.run([LOADER, "native-c1-publication"], input=json.dumps(bad).encode(), capture_output=True)
    (OUT / "wrong-digest.stdout").write_bytes(p.stdout)
    assert p.returncode != 0
    p = subprocess.run([LOADER, "native-c1-publication"], input=json.dumps(request).encode(), capture_output=True)
    (OUT / "publish.stdout").write_bytes(p.stdout)
    (OUT / "publish.stderr").write_bytes(p.stderr)
    assert p.returncode == 0, p.stdout.decode()
    receipt = json.loads(p.stdout)
    assert receipt["publication"]["outcome"] == "published"
    again = subprocess.run([LOADER, "native-c1-publication"], input=json.dumps(request).encode(), capture_output=True)
    (OUT / "replay.stdout").write_bytes(again.stdout)
    assert again.returncode == 0 and json.loads(again.stdout)["publication"]["outcome"] == "replay"
    entry = result["entries"][0]
    physical = entry["physical_id"]
    assert re.fullmatch(r"pt_[a-z0-9_]+", physical)
    mapping = result["source_instance"]
    columns = []
    for name in ["id", "code", "v", "amount"]:
        columns.append(pg("SELECT _oo.lookup_physical_mapping("
                          f"'{CTX['source_id']}','{mapping['config_digest']}',1,'column',"
                          f"'{CTX['source_database']}','items','{name}')"))
    assert all(re.fullmatch(r"pc_[a-z0-9_]+", name) for name in columns)
    role = "c1read_" + CTX["database"].split("_", 1)[1]
    pw = secrets.token_hex(24)
    pwfile = PRIVATE / "reader.pw"
    pwfile.write_text(pw)
    pwfile.chmod(0o600)
    # Fixture operator provisions an explicit narrow read scope, not a new IAM API.
    pg(f"CREATE ROLE {role} LOGIN NOSUPERUSER PASSWORD '{pw}';"
       f"GRANT CONNECT ON DATABASE {CTX['database']} TO {role};"
       f"GRANT USAGE ON SCHEMA _oo TO {role};")
    quoted = ",".join('"' + name + '"' for name in columns)
    select = "SELECT json_build_array(" + ",".join('"' + name + '"::text' for name in columns) + f") FROM _oo.{physical}"
    env = os.environ.copy()
    env["PGPASSWORD"] = pw
    args = ["docker", "exec", "-e", "PGPASSWORD", "-i", PG, "psql", "-X", "-h", "127.0.0.1",
            "-U", role, "-d", CTX["database"], "-At", "-v", "ON_ERROR_STOP=1"]
    denied = subprocess.run(args, input=select.encode(), env=env, capture_output=True)
    assert denied.returncode != 0, "reader had no approved grant yet"
    pg(f"GRANT SELECT({quoted}) ON _oo.{physical} TO {role};")
    read = subprocess.run(args, input=select.encode(), env=env, capture_output=True)
    assert read.returncode == 0, "authorized reader failed"
    actual = sorted(json.loads(line) for line in read.stdout.decode().splitlines())
    source = subprocess.run(["docker", "exec", "-i", MYSQL, "sh", "-c",
                             'MYSQL_PWD="$(cat /secrets/mysql-root.pw)" mysql -uroot -N -B --raw'],
                            input=f"SELECT JSON_ARRAY(CAST(id AS CHAR),code,v,CAST(amount AS CHAR)) FROM {CTX['source_database']}.items".encode(),
                            capture_output=True)
    assert source.returncode == 0
    expected = sorted(json.loads(line) for line in source.stdout.decode().splitlines())
    assert len(actual) == 12000 and actual == expected
    for label, rows in [("source", expected), ("reader", actual)]:
        (OUT / f"{label}-rows.json").write_text(json.dumps(rows, separators=(",", ":")))
    state = pg(f"SELECT state FROM _oo.source_watermark WHERE source='{CTX['source_id']}'")
    assert state == "live"
    proof = dict(data_publication=True, semantic_release=False, gateway_pointer_changed=False,
                 reader=role, rows=12000, independent_differences=0,
                 source_sha256=hashlib.sha256((OUT / "source-rows.json").read_bytes()).hexdigest(),
                 reader_sha256=hashlib.sha256((OUT / "reader-rows.json").read_bytes()).hexdigest(),
                 loader_sha256=hashlib.sha256(LOADER.read_bytes()).hexdigest(),
                 publication_receipt_sha256=receipt["receipt_sha256"],
                 jobs_task=task, source_state=state, wrong_digest_refused=True,
                 identical_publication_replay=True, pregrant_read_refused=True)
    (OUT / "result.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof))


if __name__ == "__main__":
    main()
