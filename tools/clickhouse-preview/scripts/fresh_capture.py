#!/usr/bin/env python3
"""Capture a new owned source with the retained reader; never relabel old bytes.

Creates a fresh isolated synthetic fixture. DBA provisioning and product reads
use different identities. Keeps its named source and evidence for consumers.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/native-20261012-fresh"
SECRETS = Path("/tmp/oo-dl01w1-40-return/secrets")
CH = "oo-dl01w1-40-gpt-fresh-ch"
NET = "oo-dl01w1-40-gpt-fresh-net"
CH_IMAGE = "clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
RUST_IMAGE = "rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
DRIVER_SHA = "8b88485c0940e298a8edad500731f0a7688c61a135a943b217f9597e191c7f93"


def run(label, argv, env=None):
    p = subprocess.run(argv, capture_output=True, env=env)
    (OUT / (label + ".stdout")).write_bytes(p.stdout)
    (OUT / (label + ".stderr")).write_bytes(p.stderr)
    (OUT / (label + ".exit")).write_text(str(p.returncode) + "\n")
    if p.returncode:
        raise RuntimeError(label + " failed; retained raw evidence")
    return p.stdout


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    for f in ["reader.password", "dba.password", "users.d/oo-users.xml"]:
        if not (SECRETS / f).is_file():
            raise RuntimeError("private fixture credential file missing")
    run("network", ["docker", "network", "create", "--subnet", "172.16.144.0/24", NET])
    run("source-start", ["docker", "run", "-d", "--pull", "never", "--name", CH,
        "--network", NET, "--ulimit", "nofile=262144:262144",
        "--label", "oo.owner=oo-dl01w1-40", "--label", "oo.kind=owned-synthetic-source",
        "--mount", f"type=bind,source={SECRETS}/users.d/oo-users.xml,target=/etc/clickhouse-server/users.d/oo-users.xml,readonly",
        "--mount", f"type=bind,source={ROOT}/testing/dl01-driver-preflight/clickhouse/config/limits.xml,target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly",
        "--mount", f"type=bind,source={SECRETS}/reader.password,target=/etc/oo/reader.password,readonly",
        "--mount", f"type=bind,source={SECRETS}/dba.password,target=/etc/oo/dba.password,readonly", CH_IMAGE])
    env = dict(os.environ, OO_CUSTOMER_CH=CH, OO_CUSTOMER_OUT=str(OUT / "source"))
    run("fixture-and-independent-source", [sys.executable, str(Path(__file__).with_name("source_customer.py"))], env)
    (OUT / "reader.user").write_text("oo_dl01w1_40_reader\n")
    base = ["docker", "run", "--rm", "--pull", "never", "--network", NET,
        "--mount", "type=volume,source=oo-dl01w1-40-target,target=/target,readonly",
        "--mount", f"type=bind,source={OUT},target=/evidence",
        "--mount", f"type=bind,source={SECRETS}/reader.password,target=/private/reader.password,readonly",
        "--entrypoint", "/target/driver/debug/ch-reader", RUST_IMAGE]
    actual = run("producer-hash", ["docker", "run", "--rm", "--pull", "never",
        "--mount", "type=volume,source=oo-dl01w1-40-target,target=/target,readonly",
        "--entrypoint", "sha256sum", RUST_IMAGE, "/target/driver/debug/ch-reader"]).decode().split()[0]
    if actual != DRIVER_SHA:
        raise RuntimeError("retained producer identity changed")
    for name, cols in [("five", "reading_id, ts, amount, note, qty"),
                       ("full", "reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64")]:
        sql = f"SELECT {cols} FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT ArrowStream"
        run("capture-" + name, base + ["arrow", "--endpoint", f"http://{CH}:8123",
            "--user-file", "/evidence/reader.user", "--password-file", "/private/reader.password",
            "--sql", sql, "--query-id", "oo-dl01w1-40-gpt-fresh-" + name,
            "--out", f"/evidence/{name}-capture.json", "--ipc-out", f"/evidence/{name}.arrow"])
    hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in OUT.glob("*.arrow")}
    (OUT / "identity.json").write_text(json.dumps({"protocol": "oo.clickhouse.fresh-capture/v1",
        "producer_sha256": actual, "historical_producer_sha256": "e62dda3084cca369487e20175178f9055371f028d45db6f35638ddc2d6edf9a1",
        "reuses_historical_capture": False, "arrow_sha256": hashes, "source_container": CH,
        "network": NET, "fixture_kind": "owned_synthetic", "resource_retained": True}, indent=2) + "\n")
    print(json.dumps({"capture": str(OUT), "producer_sha256": actual, "source": CH}))


if __name__ == "__main__":
    main()
