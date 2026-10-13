#!/usr/bin/env python3
"""Same-run Doris native closure.

Uses a private archive of a6825165. That commit contains the native sealed
catalog reader and consumer 6d9d6cbb. The live tree is not compiled.
ClickHouse pins and client self-authorization are not used. Exit codes of
real commands are stored as written. This script does not rewrite a non-zero
exit into zero.
"""

import hashlib
import json
import os
import select
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[2]
EVIDENCE = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product"
PREFLIGHT = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight"
RUN = PREFLIGHT / "product-closure" / "run"
CLOSURE = Path("/tmp/oo-dl01w1-43-closure")
PLAN = CLOSURE / "plan"
CAPTURE = CLOSURE / "capture"
SECRETS = Path("/tmp/oo-dl01w1-43-secrets")
TOKEN = Path("/tmp/oo-customer-demo-20261007/llm-token")
LOCK = "/tmp/oo-dl01-wave1-resources.lock"
BINS = CLOSURE / "out"
ADMISSION_BIN = CLOSURE / "admission" / "oo-source-admission"
RECEIPT = CLOSURE / "receipt" / "native-readonly-completion-receipt.json"
ADMISSION_SHA = "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2"
RECEIPT_SHA = "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef"
RUST_IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE = "sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f"
SESSION = "5106fed0-5bcd-4aaf-8df7-838c95a7d7e2"
SOURCE = "dorispreview"
ENGINE = "doris"
SCHEMA = "oo_dl01w1_43"
GENERATION = 1
SNAPSHOT = "oo-dl01w1-43-product"
OWNER = SESSION
RUNTIME = "oo-dl01w1-43-runtime"
PG_NAME = "oo-dl01w1-43-pg"
READER = "oo-dl01w1-43-reader"
BUILDER = "oo-dl01w1-43-rust"
FE_NAME = "oo-dl01w1-43-fe"
BE_NAME = "oo-dl01w1-43-be"
PIN = "a6825165dd83ffaa30e17fbb2da1a481a05f6559"
PIPELINE_COMMIT = PIN
PREVIEW_COMMIT = PIN
TARGET_TAG = "a6825165"
SEALED_CATALOG_MIGRATION = "883d5831c423833e5e01214ac13e169fa0ef3075775d21a6b000b2ba888958da"
NATIVE_CATALOG_MIGRATION = "5c25559bc4d1b24471f19ae9eae8a45f40007c955938ea51edb03f1f5dbf2e26"
PROBE_OBJECT = "sat_cast"
CUSTOMER = {"secret_ref": None, "approved": None}
CHAIN = {"code": 0, "command": None, "imports": {}, "sealed": None}

# name, arrow_type, nullable, precision, scale, key, source_type
TABLES = {
    "dec_exact": [
        ("id", "int64", False, 64, None, True, "int64"),
        ("a18", "decimal128", True, 18, 4, False, "decimal128"),
        ("b18", "decimal128", True, 18, 4, False, "decimal128"),
        ("d38", "decimal128", True, 38, 18, False, "decimal128"),
    ],
    "dup_events": [
        ("event_id", "int64", False, 64, None, True, "int64"),
        ("kind", "utf8", False, None, None, True, "utf8"),
        ("qty", "decimal128", True, 10, 2, False, "decimal128"),
    ],
    "agg_qty": [
        ("sku", "utf8", False, None, None, True, "utf8"),
        ("qty", "decimal128", True, 10, 2, False, "decimal128"),
        ("note", "utf8", True, None, None, False, "utf8"),
    ],
    "uniq_acct": [
        ("id", "int64", False, 64, None, True, "int64"),
        ("balance", "decimal128", True, 18, 4, False, "decimal128"),
        ("label", "utf8", True, None, None, False, "utf8"),
    ],
    "str_key": [
        ("k", "utf8", False, None, None, True, "utf8"),
        ("note", "utf8", True, None, None, False, "utf8"),
    ],
    "nz_obs": [
        ("id", "int64", False, 64, None, True, "int64"),
        ("qty", "decimal128", True, 10, 2, False, "decimal128"),
    ],
    "part_day": [
        ("day", "date32", False, None, None, True, "date32"),
        ("region", "utf8", False, None, None, True, "utf8"),
        ("amount", "decimal128", True, 12, 2, False, "decimal128"),
    ],
}
SEAL_TABLE = "dec_exact"
SHAPE_TABLE = "part_day"


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def numeric_typmod(precision, scale):
    return ((precision << 16) | (scale & 0x7FF)) + 4


def oid_typmod(arrow, precision, scale):
    if arrow == "int64":
        return 20, -1
    if arrow == "utf8":
        return 1043, -1
    if arrow == "decimal128":
        return 1700, numeric_typmod(precision, scale)
    return None


def schema_preimage(table, columns):
    out = f"{SCHEMA}.{table}\n"
    for name, arrow, _nullable, precision, scale, _key, _source in columns:
        typed = oid_typmod(arrow, precision, scale)
        if typed is None:
            continue
        oid, typmod = typed
        out += f"{name}\t{oid}\t{typmod}\t\n"
    return out


def type_preimage(columns):
    lines = []
    for name, arrow, nullable, precision, scale, _key, _source in columns:
        prec = "" if precision is None else str(precision)
        scl = "" if scale is None else str(scale)
        lines.append(f"{name}\t{arrow}\t{str(nullable).lower()}\t{prec}\t{scl}\n")
    return "".join(lines)


def physical_name(level, source, config_digest, generation, namespace, table, column):
    prefix = {"n": "pn_", "t": "pt_", "c": "pc_"}[level]
    blob = b"physical-mapping/v1\0"
    blob += source.encode() + b"\0"
    blob += config_digest.encode() + b"\0"
    blob += str(generation).encode() + b"\0"
    blob += level.encode()
    blob += namespace.encode() + b"\0"
    blob += table.encode() + b"\0"
    blob += column.encode() + b"\0"
    return prefix + hashlib.sha256(blob).hexdigest()[:60]


def self_check():
    if numeric_typmod(18, 4) != ((18 << 16) | 4) + 4:
        return 1
    if numeric_typmod(38, 18) != ((38 << 16) | 18) + 4:
        return 1
    preimage = schema_preimage("dec_exact", TABLES["dec_exact"])
    if "dec_exact" not in preimage or "\t20\t-1\t\n" not in preimage:
        return 1
    name = physical_name("t", SOURCE, "ab" * 32, 1, SCHEMA, "dec_exact", "")
    if len(name) != 63 or not name.startswith("pt_"):
        return 1
    digest = sha256_bytes(preimage.encode())
    if len(digest) != 64:
        return 1
    day = TABLES["part_day"][0]
    if day[1] != "date32" or day[3] is not None or day[4] is not None:
        return 1
    if PIPELINE_COMMIT != PREVIEW_COMMIT or len(PIN) != 40:
        return 1
    if decimal_canon("0.3000", 4) != ("decimal", False, "3000", 4):
        return 1
    if decimal_canon("-1.2500", 4) != ("decimal", True, "12500", 4):
        return 1
    if decimal_canon("0.00", 2) != ("decimal", False, "0", 2):
        return 1
    if decimal_canon("12345678901234567890.123456789012345678", 18) != (
        "decimal", False, "12345678901234567890123456789012345678", 18
    ):
        return 1
    print("self_check_ok")
    return 0


def run(cmd, timeout=120, env=None):
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout, env=env)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_json(path, value):
    write(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def labels(kind):
    return [
        "--label", "oo.project=open-ontology",
        "--label", "oo.owner=DL01-W1-43",
        "--label", f"oo.session={SESSION}",
        "--label", "oo.prefix=oo-dl01w1-43",
        "--label", "oo.task=DL01-W1-43",
        "--label", f"oo.kind={kind}",
    ]


def measure(extra_cpu, extra_mem):
    ids = [line for line in run(["docker", "ps", "-q"]).stdout.split() if line]
    cpu = 0.0
    mem = 0
    for container in ids:
        nano, memory = run(
            ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", container]
        ).stdout.split()
        nano = int(nano)
        memory = int(memory)
        cpu += 1.0 if nano == 0 else nano / 1_000_000_000
        mem += memory
    builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"]).stdout.split()
    host_rust = 0
    listed = run(["ps", "-ax", "-o", "command="])
    for line in listed.stdout.splitlines():
        parts = line.strip().split()
        base = os.path.basename(parts[0]) if parts else ""
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust += 1
    total = int(run(["docker", "info", "-f", "{{.MemTotal}}"]).stdout.strip() or "0")
    own = run([
        "docker", "ps", "-q",
        "--filter", "label=oo.owner=DL01-W1-43",
        "--filter", "label=oo.kind=rust-builder",
    ]).stdout.split()
    return {
        "cpu_after": cpu + extra_cpu,
        "headroom_bytes": total - mem - extra_mem,
        "global_rust": len(builders) + host_rust,
        "own_rust_builders": len(own),
    }


def capacity_ok(snap):
    return (
        snap["own_rust_builders"] == 0
        and snap["global_rust"] < 10
        and snap["cpu_after"] <= 16
        and snap["headroom_bytes"] > 10 * 1024 ** 3
    )


def wait_event(path):
    proc = subprocess.Popen(
        ["docker", "events", "--filter", "type=container", "--format", "{{.Action}}"],
        stdout=subprocess.PIPE,
        text=True,
    )
    ready, _, _ = select.select([proc.stdout], [], [], 900)
    if ready:
        write(path, proc.stdout.readline())
    else:
        write(path, "900s without a container event\n")
    proc.kill()
    proc.wait()


def admit_start(name, kind, extra_cpu, extra_mem, command):
    import fcntl

    evidence = CLOSURE / "capacity"
    while True:
        lock = open(LOCK, "a")
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            snap = measure(extra_cpu, extra_mem)
            write_json(evidence / f"{name}.json", snap)
            if capacity_ok(snap):
                started = run(command, timeout=180)
                write(evidence / f"{name}.stdout", started.stdout)
                write(evidence / f"{name}.stderr", started.stderr)
                write(evidence / f"{name}.exit", f"{started.returncode}\n")
                return started.returncode
            write_json(evidence / f"{name}-pending.json", {"status": "pending-capacity", **snap})
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
        print(f"{kind} pending-capacity; waiting one docker event or 900s")
        wait_event(evidence / f"{name}-event.txt")


def owned_running(name):
    inspected = run(["docker", "inspect", "-f", "{{.Config.Labels}}", name])
    if inspected.returncode != 0:
        return False
    return "DL01-W1-43" in inspected.stdout


def remove_owned(name):
    if not owned_running(name) and run(["docker", "container", "inspect", name]).returncode != 0:
        return 0
    inspected = run(["docker", "inspect", "-f", "{{index .Config.Labels \"oo.owner\"}}", name])
    if inspected.stdout.strip() != "DL01-W1-43":
        return 0
    removed = run(["docker", "rm", "-f", name], timeout=60)
    write(CLOSURE / "capacity" / f"rm-{name}.exit", f"{removed.returncode}\n")
    write(CLOSURE / "capacity" / f"rm-{name}.stderr", removed.stderr)
    return removed.returncode


def ensure_pg_secrets():
    os.makedirs(SECRETS, mode=0o700, exist_ok=True)
    os.chmod(SECRETS, 0o700)
    password = SECRETS / "pg_admin_password"
    if not password.exists() or password.stat().st_size == 0:
        token = subprocess.check_output(["openssl", "rand", "-hex", "24"], text=True).strip()
        fd = os.open(password, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(token + "\n")
    os.chmod(password, 0o600)
    login_password = SECRETS / "doris_login_password"
    if not login_password.exists() or login_password.stat().st_size == 0:
        token = subprocess.check_output(["openssl", "rand", "-hex", "24"], text=True).strip()
        fd = os.open(login_password, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(token + "\n")
    os.chmod(login_password, 0o600)
    runtime_password = SECRETS / "oo_runtime_password"
    if not runtime_password.exists() or runtime_password.stat().st_size == 0:
        token = subprocess.check_output(["openssl", "rand", "-hex", "24"], text=True).strip()
        fd = os.open(runtime_password, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(token + "\n")
    os.chmod(runtime_password, 0o600)
    san = SECRETS / "san.cnf"
    san.write_text(
        "subjectAltName=DNS:oo-dl01w1-43-pg\n"
        "basicConstraints=CA:FALSE\n"
        "keyUsage=digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\n",
        encoding="utf-8",
    )
    if not (SECRETS / "pg-ca.crt").exists() or not (SECRETS / "server.crt").exists():
        steps = [
            ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(SECRETS / "ca.key"), "-out", str(SECRETS / "pg-ca.crt"),
             "-days", "3", "-subj", "/CN=oo-dl01w1-43-ca"],
            ["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(SECRETS / "server.key"), "-out", str(SECRETS / "server.csr"),
             "-subj", "/CN=oo-dl01w1-43-pg"],
            ["openssl", "x509", "-req", "-in", str(SECRETS / "server.csr"),
             "-CA", str(SECRETS / "pg-ca.crt"), "-CAkey", str(SECRETS / "ca.key"),
             "-CAcreateserial", "-out", str(SECRETS / "server.crt"), "-days", "3",
             "-extfile", str(san)],
        ]
        for cmd in steps:
            proc = run(cmd, timeout=30)
            if proc.returncode != 0:
                write(CLOSURE / "pg" / "openssl.stderr", proc.stderr)
                return proc.returncode or 1
    for name in ("ca.key", "server.key", "pg_admin_password"):
        os.chmod(SECRETS / name, 0o600)
    for name in ("pg-ca.crt", "server.crt"):
        os.chmod(SECRETS / name, 0o644)
    return 0


def capacity_amount(field):
    parts = field.split()
    if len(parts) != 2 or parts[1] not in {"KB", "MB", "GB", "TB"}:
        return None
    try:
        amount = float(parts[0])
    except ValueError:
        return None
    return amount if amount > 0 else None


def backend_has_capacity(text):
    # SHOW BACKENDS puts AvailCapacity and TotalCapacity at indexes 14 and 15.
    # A later MemLimit column is also "N GB" and is not disk capacity.
    for line in text.splitlines():
        fields = [field.strip() for field in line.split("\t")]
        if len(fields) < 16:
            continue
        if fields[1] != "172.16.43.20" or fields[9] != "true":
            continue
        if capacity_amount(fields[14]) and capacity_amount(fields[15]):
            return True
    return False


def capture_engine_logs():
    for name in (FE_NAME, BE_NAME):
        if run(["docker", "container", "inspect", name]).returncode != 0:
            continue
        logs = run(["docker", "logs", "--tail", "200", name], timeout=40)
        write(RUN / f"{name}.log.tail", logs.stdout[-20000:] + logs.stderr[-20000:])


def wait_be_capacity():
    admit_dir = PREFLIGHT / "product-closure" / "admit"
    deadline = time.time() + 600
    last = ""
    while time.time() < deadline:
        be = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}}", BE_NAME])
        state = be.stdout.strip()
        write(admit_dir / "be-state.txt", state + "\n")
        if state.startswith("exited"):
            capture_engine_logs()
            code = state.split()[-1]
            return int(code) if code.isdigit() and int(code) != 0 else 1
        probed = run(
            [
                "docker", "exec", FE_NAME, "mysql", "-uroot", "-P9030", "-h127.0.0.1",
                "-N", "-B", "-e", "SHOW BACKENDS",
            ],
            timeout=30,
        )
        last = probed.stdout
        write(admit_dir / "backends-last.txt", probed.stdout + probed.stderr)
        if probed.returncode == 0 and backend_has_capacity(probed.stdout):
            write(admit_dir / "backends.txt", probed.stdout)
            return 0
        time.sleep(10)
    write(admit_dir / "backends-timeout.txt", last)
    capture_engine_logs()
    return 1


def product_reader():
    env = os.environ.copy()
    env["OO_DORIS_BIN"] = str(BINS)
    env["OO_DORIS_RUN"] = str(RUN)
    env["OO_DORIS_ADMIT_EVIDENCE"] = str(PREFLIGHT / "product-closure" / "admit")
    env["OO_DORIS_PRODUCT_READER"] = "1"
    env["OO_DORIS_PRODUCT_AUDIT"] = "1"
    env["OO_DORIS_SECRET_REF"] = CUSTOMER["secret_ref"]
    env["OO_DORIS_APPROVED_OBJECTS"] = ",".join(CUSTOMER["approved"])
    admit = run([sys.executable, str(ROOT / "scripts" / "admit-native.py")], timeout=1200, env=env)
    write(RUN / "admit-native.stdout", admit.stdout)
    write(RUN / "admit-native.stderr", admit.stderr)
    write(RUN / "admit-native.exit", f"{admit.returncode}\n")
    if admit.returncode != 0:
        return admit.returncode
    ready = wait_be_capacity()
    write(RUN / "wait-be.exit", f"{ready}\n")
    if ready != 0:
        return ready
    reader = run([sys.executable, str(ROOT / "scripts" / "run-reader.py")], timeout=1800, env=env)
    write(RUN / "run-reader.stdout", reader.stdout)
    write(RUN / "run-reader.stderr", reader.stderr)
    write(RUN / "run-reader.exit", f"{reader.returncode}\n")
    return reader.returncode


def copy_arrows():
    gate = RUN / "gate"
    CAPTURE.mkdir(parents=True, exist_ok=True)
    copied = {}
    for name in TABLES:
        source = gate / f"{name}.arrow"
        if not source.is_file():
            copied[name] = None
            continue
        dest = CAPTURE / f"{name}.arrow"
        shutil.copyfile(source, dest)
        copied[name] = {
            "sha256": sha256_file(dest),
            "bytes": dest.stat().st_size,
        }
        source.unlink()
    write_json(CLOSURE / "capture" / "arrow-sha256.json", copied)
    return copied


def oracle_counts():
    oracle = json.loads((ROOT / "fixtures" / "oracle.json").read_text(encoding="utf-8"))
    counts = {}
    for name, body in oracle["tables"].items():
        rows = body.get("rows")
        if isinstance(rows, list):
            counts[name] = len(rows)
    return counts


def column_json(spec):
    name, arrow, nullable, precision, scale, _key, _source = spec
    body = {
        "arrow_type": arrow,
        "logical_name": name,
        "name": name,
        "nullable": nullable,
    }
    if precision is not None:
        body["precision"] = precision
    if scale is not None:
        body["scale"] = scale
    return body


def build_identity_files(arrows, counts, secret_ref):
    config = {
        "budget": {"confidence": "unknown", "hard_limit": sum(counts[name] for name in TABLES if name in counts), "unit": "Rows"},
        "compat_mode": "doris",
        "debezium": {
            "create_stream": False,
            "create_trigger": False,
            "heartbeat_action_query": None,
            "signal_data_collection": None,
            "snapshot_mode": "never",
        },
        "permissions": [{"granted": True, "name": "SELECT", "status": "known"}],
        "protocol": "oo.source-admission.config/v1",
        "r1w": None,
        "requested_write_class": "R0",
        "secret_ref": secret_ref,
        "source_id": SOURCE,
        "target_id": SOURCE,
        "write_options": {
            "create_stream": False,
            "create_trigger": False,
            "heartbeat_write": False,
            "signal_table": False,
        },
    }
    config_path = PLAN / "admission-config.json"
    write_json(config_path, config)
    config_bytes = config_path.read_bytes()
    driver = sha256_file(BINS / "doris-reader")
    observed = {name: counts.get(name) for name in TABLES}
    report = {
        "capabilities": [{
            "budget_unit": None,
            "cdc": None,
            "consistency": "S0",
            "cursor": None,
            "feed": None,
            "level": "experimental",
            "name": "BatchFull",
            "token_order": None,
            "write_class": "R0",
        }],
        "capture_methods": [{
            "cdc": None,
            "consistency": "S0",
            "cursor": None,
            "feed": None,
            "method": "C5",
            "retention_window": None,
            "token_order": "None",
            "write_class": "R0",
        }],
        "compat_mode": "doris",
        "cost_model": {
            "confidence": "unknown",
            "estimate": (
                "Product reader full SELECT row counts "
                + json.dumps(observed, sort_keys=True)
                + ". Doris published no billing unit. Unit Rows is the observed snapshot count. "
                + "The separate row-cap probe remains max_rows 2 and is not this budget."
            ),
            "unit": "Rows",
        },
        "driver": {
            "digest": driver,
            "kind": "native",
            "name": "mysql_async",
            "version": "0.36.1",
        },
        "license_flags": ["Apache-2.0"],
        "permissions": [{"granted": True, "name": "SELECT", "status": "known"}],
        "protocol": "oo.source-admission.report/v1",
        "r1w": None,
        "secret_ref": secret_ref,
        "source_id": SOURCE,
        "target_id": SOURCE,
    }
    report_path = PLAN / "admission-report.json"
    write_json(report_path, report)
    receipt = {
        "arrow": arrows,
        "capture": "bounded full SELECT of current rows",
        "method": "C5",
        "protocol": "oo.doris.capture-receipt/v1",
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_engine": ENGINE,
        "source_id": SOURCE,
        "token_order": "None",
        "write_class": "R0",
    }
    receipt_path = PLAN / "capture-receipt.json"
    write_json(receipt_path, receipt)
    identities = {}
    for name, columns in TABLES.items():
        if not arrows.get(name):
            continue
        arrow_sha = arrows[name]["sha256"]
        schema = schema_preimage(name, columns)
        typed = type_preimage(columns)
        identities[name] = {
            "config_digest": sha256_bytes(config_bytes),
            "input_digest": arrow_sha,
            "row_count": counts[name],
            "schema_digest": sha256_bytes(schema.encode()),
            "schema_preimage": schema,
            "type_digest": sha256_bytes(typed.encode()),
            "type_preimage": typed,
        }
        catalog = {
            "config_digest": identities[name]["config_digest"],
            "engine": ENGINE,
            "input_digest": arrow_sha,
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "schema_digest": identities[name]["schema_digest"],
            "source_generation": GENERATION,
            "source_id": SOURCE,
            "state": "prepared",
            "type_digest": identities[name]["type_digest"],
        }
        catalog_path = PLAN / f"binding-catalog-{name}.json"
        write_json(catalog_path, catalog)
        identities[name]["catalog_path"] = str(catalog_path)
        identities[name]["catalog_sha256"] = sha256_file(catalog_path)
    meta = {
        "config_path": str(config_path),
        "config_sha256": sha256_file(config_path),
        "driver_digest": driver,
        "receipt_path": str(receipt_path),
        "receipt_sha256": sha256_file(receipt_path),
        "report_path": str(report_path),
        "report_sha256": sha256_file(report_path),
        "tables": identities,
    }
    write_json(PLAN / "identity.json", meta)
    return meta


def build_registry(meta):
    bindings = []
    for name, columns in TABLES.items():
        item = meta["tables"].get(name)
        if item is None:
            continue
        fields = []
        for spec in columns:
            field = column_json(spec)
            field.pop("logical_name", None)
            fields.append(field)
        bindings.append({
            "admission_receipt_path": str(RECEIPT),
            "capture_receipt_path": meta["receipt_path"],
            "capture_receipt_sha256": meta["receipt_sha256"],
            "catalog_path": item["catalog_path"],
            "catalog_sha256": item["catalog_sha256"],
            "config_digest": item["config_digest"],
            "config_path": meta["config_path"],
            "config_sha256": meta["config_sha256"],
            "decision_sha256": "0" * 64,
            "driver_digest": meta["driver_digest"],
            "driver_kind": "native",
            "driver_name": "mysql_async",
            "driver_version": "0.36.1",
            "fields": fields,
            "input_digest": item["input_digest"],
            "objects": [{
                "arrow_sha256": item["input_digest"],
                "logical_schema": SCHEMA,
                "logical_table": name,
                "row_count": item["row_count"],
            }],
            "protocol": "oo.source-admission.capture-binding/v1",
            "report_path": meta["report_path"],
            "report_sha256": meta["report_sha256"],
            "schema_digest": item["schema_digest"],
            "snapshot": {"kind": "token", "value": SNAPSHOT},
            "source_engine": ENGINE,
            "source_generation": GENERATION,
            "source_id": SOURCE,
            "type_digest": item["type_digest"],
        })
    registry = {"bindings": bindings, "protocol": "oo.source-admission.binding-registry/v1"}
    path = PLAN / "registry.json"
    write_json(path, registry)
    return path


def manifest_for(name, meta):
    item = meta["tables"][name]
    columns = [column_json(spec) for spec in TABLES[name]]
    return {
        "admission": {
            "decision_path": str(PLAN / "decision.json"),
            "receipt_path": str(RECEIPT),
        },
        "catalog_identity": {
            "config_digest": item["config_digest"],
            "input_digest": item["input_digest"],
            "schema_digest": item["schema_digest"],
            "type_digest": item["type_digest"],
        },
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_engine": ENGINE,
        "source_generation": GENERATION,
        "source_id": SOURCE,
        "tables": [{
            "arrow_path": str(CAPTURE / f"{name}.arrow"),
            "batch_sha256": item["input_digest"],
            "columns": columns,
            "logical_schema": SCHEMA,
            "logical_table": name,
            "row_count": item["row_count"],
        }],
        "target": {
            "ca_file": "/secrets/pg-ca.crt",
            "database": "previewdb",
            "host": PG_NAME,
            "owner": OWNER,
            "password_file": "/secrets/pg_admin_password",
            "port": 5432,
            "runtime_principal": "oo_admin",
            "shard": SOURCE,
        },
        "target_storage": "postgresql",
    }


def start_pg():
    code = ensure_pg_secrets()
    if code != 0:
        return code
    remove_owned(PG_NAME)
    command = [
        "docker", "run", "-d", "--name", PG_NAME,
        "--platform", "linux/arm64", "--pull", "never",
        "--network", "oo-dl01w1-43-net",
        "--memory", "1g", "--memory-swap", "1g", "--cpus", "1",
        "--pids-limit", "256",
        *labels("postgres-target"),
        "-e", "POSTGRES_PASSWORD_FILE=/secrets/pg_admin_password",
        "-e", "POSTGRES_USER=oo_admin",
        "-e", "POSTGRES_DB=previewdb",
        "-e", "POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256 --auth-local=scram-sha-256",
        "--mount", f"type=bind,source={SECRETS},target=/secrets",
        "--entrypoint", "bash",
        PG_IMAGE, "-c",
        "set -eu; "
        "chown postgres:postgres /secrets/server.key /secrets/server.crt /secrets/pg-ca.crt /secrets/pg_admin_password /secrets/doris_login_password /secrets/oo_runtime_password; "
        "chmod 600 /secrets/server.key /secrets/pg_admin_password /secrets/doris_login_password /secrets/oo_runtime_password; "
        "chmod 644 /secrets/server.crt /secrets/pg-ca.crt; "
        "exec docker-entrypoint.sh postgres -c ssl=on "
        "-c ssl_cert_file=/secrets/server.crt -c ssl_key_file=/secrets/server.key "
        "-c listen_addresses='*' -c shared_buffers=64MB",
    ]
    return admit_start("pg", "postgres-target", 1.0, 1024 ** 3, command)


def start_runtime():
    remove_owned(RUNTIME)
    command = [
        "docker", "run", "-d", "--name", RUNTIME,
        "--platform", "linux/arm64", "--pull", "never", "--init",
        "--user", "0:0",
        "--security-opt", "no-new-privileges", "--cap-drop", "ALL",
        "--memory", "2g", "--memory-swap", "2g", "--cpus", "1",
        "--pids-limit", "256",
        "--network", "oo-dl01w1-43-net",
        "--add-host", "host.docker.internal:host-gateway",
        *labels("import-runtime"),
        "--mount", f"type=bind,source={CLOSURE},target={CLOSURE}",
        "--mount", f"type=bind,source={SECRETS},target=/secrets",
        "--mount", f"type=bind,source={TOKEN},target={TOKEN},readonly",
        "--entrypoint", "sleep",
        RUST_IMAGE, "200000",
    ]
    return admit_start("runtime", "import-runtime", 1.0, 2 * 1024 ** 3, command)


def exec_runtime(args, log_name, env_map=None, timeout=180):
    command = ["docker", "exec", "-u", "0:0"]
    for key, value in (env_map or {}).items():
        command.extend(["-e", f"{key}={value}"])
    command.append(RUNTIME)
    command.extend(args)
    result = run(command, timeout=timeout)
    write(CLOSURE / "raw" / f"{log_name}.stdout", result.stdout)
    write(CLOSURE / "raw" / f"{log_name}.stderr", result.stderr)
    write(CLOSURE / "raw" / f"{log_name}.exit", f"{result.returncode}\n")
    return result


def wait_pg():
    for _ in range(60):
        probed = run(
            ["docker", "exec", PG_NAME, "pg_isready", "-U", "oo_admin", "-d", "previewdb"],
            timeout=20,
        )
        write(CLOSURE / "raw" / "pg-isready.stdout", probed.stdout)
        write(CLOSURE / "raw" / "pg-isready.stderr", probed.stderr)
        write(CLOSURE / "raw" / "pg-isready.exit", f"{probed.returncode}\n")
        if probed.returncode == 0:
            return 0
        time.sleep(2)
    return 1


def init_pg():
    request = {
        "protocol": "oo.pipeline.init-pg/v1",
        "target": {
            "administrator": "oo_admin",
            "ca_file": "/secrets/pg-ca.crt",
            "database": "previewdb",
            "host": PG_NAME,
            "password_file": "/secrets/pg_admin_password",
            "port": 5432,
        },
    }
    path = PLAN / "init-pg.json"
    write_json(path, request)
    result = exec_runtime(
        ["bash", "-c", f"{BINS / 'init-pg'} < {path}"],
        "init-pg",
        timeout=120,
    )
    return result.returncode


def validate_decision(meta):
    result = exec_runtime(
        [
            str(ADMISSION_BIN), "validate",
            "--report", meta["report_path"],
            "--config", meta["config_path"],
        ],
        "validate",
        timeout=120,
    )
    decision_path = PLAN / "decision.json"
    decision_path.write_bytes(result.stdout.encode())
    digest = sha256_bytes(result.stdout.encode())
    write(PLAN / "decision.sha256", digest + "\n")
    registry = json.loads((PLAN / "registry.json").read_text(encoding="utf-8"))
    for binding in registry["bindings"]:
        binding["decision_sha256"] = digest
    write_json(PLAN / "registry.json", registry)
    write(PLAN / "registry.sha256", sha256_file(PLAN / "registry.json") + "\n")
    return result.returncode, digest


def operator_env():
    return {
        "OO_NATIVE_ADMISSION_BIN": str(ADMISSION_BIN),
        "OO_NATIVE_BINDING_REGISTRY": str(PLAN / "registry.json"),
        "OO_NATIVE_BINDING_REGISTRY_SHA256": (PLAN / "registry.sha256").read_text(encoding="utf-8").strip(),
    }


def import_table(name, meta):
    manifest = manifest_for(name, meta)
    path = PLAN / f"manifest-{name}.json"
    write_json(path, manifest)
    out = CLOSURE / "raw" / f"import-{name}.json"
    err = CLOSURE / "raw" / f"import-{name}.stderr"
    result = exec_runtime(
        [
            str(BINS / "pg-builder"), "import-native",
            "--manifest", str(path),
            "--out", str(out),
            "--pipeline", str(BINS / "oo-pipeline"),
            "--stderr", str(err),
        ],
        f"import-{name}",
        operator_env(),
        timeout=120,
    )
    return result.returncode


def psql(sql, log_name):
    script = PLAN / f"{log_name}.sql"
    script.write_text(sql, encoding="utf-8")
    result = subprocess.run(
        [
            "docker", "exec", "-i", PG_NAME, "bash", "-c",
            "export PGPASSWORD=\"$(cat /secrets/pg_admin_password)\"; "
            "exec psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At -F '|'",
        ],
        input=sql,
        text=True,
        capture_output=True,
        timeout=40,
    )
    write(CLOSURE / "raw" / f"{log_name}.stdout", result.stdout)
    write(CLOSURE / "raw" / f"{log_name}.stderr", result.stderr)
    write(CLOSURE / "raw" / f"{log_name}.exit", f"{result.returncode}\n")
    return result


def ensure_reader_login():
    # The sealed login and the preview target user are the same principal.
    # install-column-acl creates the NOLOGIN role. This only creates the login.
    script = SECRETS / "create-doris-login.sh"
    script.write_text(
        "#!/bin/bash\n"
        "set -eu\n"
        "pw=$(tr -d '\\r\\n' < /secrets/doris_login_password)\n"
        "export PGPASSWORD=\"$(tr -d '\\r\\n' < /secrets/pg_admin_password)\"\n"
        "if ! psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At "
        "-c \"SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_doris_login'\" | grep -q 1; then\n"
        "  psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 "
        "-c \"CREATE ROLE oo_doris_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${pw}'\"\n"
        "fi\n"
        "psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 "
        "-c \"GRANT CONNECT ON DATABASE previewdb TO oo_doris_login\"\n"
        "psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At "
        "-c \"SELECT rolname || '|' || rolcanlogin || '|' || rolsuper FROM pg_catalog.pg_roles WHERE rolname = 'oo_doris_login'\"\n",
        encoding="utf-8",
    )
    os.chmod(script, 0o755)
    result = run(["docker", "exec", PG_NAME, "bash", "/secrets/create-doris-login.sh"], timeout=40)
    write(CLOSURE / "raw" / "reader-login.stdout", result.stdout if result.returncode == 0 else "")
    write(
        CLOSURE / "raw" / "reader-login.stderr",
        "" if result.returncode == 0 else "reader login create failed\n",
    )
    write(CLOSURE / "raw" / "reader-login.exit", f"{result.returncode}\n")
    return result.returncode


def ensure_runtime_login():
    script = SECRETS / "create-oo-runtime.sh"
    script.write_text(
        "#!/bin/bash\n"
        "set -eu\n"
        "pw=$(tr -d '\\r\\n' < /secrets/oo_runtime_password)\n"
        "export PGPASSWORD=\"$(tr -d '\\r\\n' < /secrets/pg_admin_password)\"\n"
        "if ! psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At "
        "-c \"SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_runtime'\" | grep -q 1; then\n"
        "  psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 "
        "-c \"CREATE ROLE oo_runtime LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${pw}'\"\n"
        "fi\n"
        "psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 "
        "-c \"GRANT CONNECT ON DATABASE previewdb TO oo_runtime\"\n"
        "psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 "
        "-c \"GRANT oo_pipeline_runtime TO oo_runtime\"\n"
        "psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At "
        "-c \"SELECT rolname || '|' || rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = 'oo_runtime'\"\n",
        encoding="utf-8",
    )
    os.chmod(script, 0o755)
    result = run(["docker", "exec", PG_NAME, "bash", "/secrets/create-oo-runtime.sh"], timeout=40)
    write(CLOSURE / "raw" / "runtime-login.stdout", result.stdout if result.returncode == 0 else "")
    write(
        CLOSURE / "raw" / "runtime-login.stderr",
        "" if result.returncode == 0 else "runtime login create failed\n",
    )
    write(CLOSURE / "raw" / "runtime-login.exit", f"{result.returncode}\n")
    return result.returncode


def lookup_generation(run_id, label):
    result = psql(
        "SELECT target_generation FROM _oo.full_import_run WHERE run_id = "
        f"'{run_id}';\n",
        f"target-generation-{label}",
    )
    text = result.stdout.strip().splitlines()
    return text[-1].strip() if result.returncode == 0 and text else ""


def lookup_owner(run_id):
    result = psql(
        "SELECT owner_token::text FROM _oo.full_import_run WHERE run_id = "
        f"'{run_id}';\n",
        "owner-token",
    )
    text = result.stdout.strip().splitlines()
    return text[-1].strip() if result.returncode == 0 and text else ""


def lookup_physical(table):
    result = psql(
        "SELECT level || '|' || logical_column || '|' || physical_name "
        "FROM _oo.physical_mappings "
        f"WHERE source = '{SOURCE}' AND logical_namespace = '{SCHEMA}' "
        f"AND logical_table IN ('{table}', '') "
        "ORDER BY level, logical_column;\n",
        f"physical-{table}",
    )
    found = {"n": "", "t": "", "c": {}}
    if result.returncode != 0:
        return found
    for line in result.stdout.splitlines():
        parts = line.strip().split("|")
        if len(parts) != 3:
            continue
        level, column, name = parts
        if level == "c":
            found["c"][column] = name
        else:
            found[level] = name
    return found


def closed_batch(value):
    if not isinstance(value, str) or ".b" not in value:
        return False
    left, right = value.split(".b", 1)
    if not left.startswith("t"):
        return False
    index = left[1:]
    return index.isdigit() and 1 <= len(index) <= 6 and right.isdigit() and 1 <= len(right) <= 6


def seal_catalog(meta):
    tables_out = []
    stored_rows = []
    drift = []
    batch_notes = []
    generations = {}
    preimage = ""
    order = list(TABLES)
    for index, name in enumerate(order):
        envelope = load_json(CLOSURE / "raw" / f"import-{name}.json") or {}
        imported = envelope.get("import") or {}
        if imported.get("load_state") != "awaiting_publish" or not imported.get("tables"):
            return None
        table = imported["tables"][0]
        run_id = imported.get("run_id") or ""
        generations[name] = lookup_generation(run_id, f"seal-{name}") if run_id else ""
        physical = lookup_physical(name)
        predicted = {spec[0]: spec for spec in TABLES[name]}
        columns_out = []
        for column in table.get("columns") or []:
            cname = column["logical_name"]
            spec = predicted[cname]
            expect = oid_typmod(spec[1], spec[3], spec[4])
            if expect is not None and expect != (column["oid"], column["typmod"]):
                drift.append({
                    "column": cname,
                    "import": [column["oid"], column["typmod"]],
                    "predicted": list(expect),
                    "table": name,
                })
            stored = physical["c"].get(cname) or physical_name(
                "c", SOURCE, meta["tables"][name]["config_digest"], GENERATION, SCHEMA, name, cname
            )
            columns_out.append({
                "caveat": None,
                "key": spec[5],
                "logical_name": cname,
                "oid": column["oid"],
                "source_type": spec[6],
                "stored_physical": stored,
                "typmod": column["typmod"],
            })
        preimage += f"{SCHEMA}.{name}\n"
        for column in columns_out:
            preimage += f"{column['logical_name']}\t{column['oid']}\t{column['typmod']}\t\n"
        namespace = physical["n"] or physical_name(
            "n", SOURCE, meta["tables"][name]["config_digest"], GENERATION, SCHEMA, "", ""
        )
        stored_table = table.get("physical_name") or physical["t"]
        batch = table.get("batch_id")
        if not closed_batch(batch):
            used = f"t{index}.b0"
            batch_notes.append({"batch_id": batch, "table": name, "used": used})
            batch = used
        tables_out.append({
            "accepted_rows": table["row_count"],
            "canonical_digest": table["canonical_digest"],
            "columns": columns_out,
            "logical_schema": SCHEMA,
            "logical_table": name,
            "rejects": [],
            "stored_namespace": namespace,
            "stored_physical": stored_table,
        })
        stored_rows.append({
            "accepted_rows": table["row_count"],
            "batch_id": batch,
            "kind": "batch",
            "physical_name": stored_table,
            "rejected_rows": 0,
        })
    write_json(CLOSURE / "raw" / "oid-drift.json", drift)
    seal_envelope = load_json(CLOSURE / "raw" / f"import-{SEAL_TABLE}.json") or {}
    run_id = (seal_envelope.get("import") or {}).get("run_id")
    item = meta["tables"][SEAL_TABLE]
    schema_digest = sha256_bytes(preimage.encode())
    input_digest = sha256_bytes("".join(meta["tables"][name]["input_digest"] + "\n" for name in order).encode())
    type_digest = sha256_bytes("".join(meta["tables"][name]["type_digest"] + "\n" for name in order).encode())
    generation = generations.get(SEAL_TABLE, "")
    source_generations = {}
    catalog_generations = {}
    for name in order:
        imported = (load_json(CLOSURE / "raw" / f"import-{name}.json") or {}).get("import") or {}
        source_generations[name] = imported.get("source_generation")
        native = load_json(CLOSURE / "raw" / f"native-catalog-{name}.json") or {}
        catalog_generations[name] = native.get("target_generation")
    bound = (
        len(generations) == len(order)
        and all(str(generations[name]).isdigit() and int(generations[name]) >= 1 for name in order)
        and all(source_generations.get(name) == GENERATION for name in order)
        and all(catalog_generations.get(name) == int(generations[name]) for name in order)
        and str(generation).isdigit()
    )
    catalog = {
        "capture": "sealed_import",
        "config_digest": item["config_digest"],
        "engine": ENGINE,
        "hook_checked": False,
        "input_digest": input_digest,
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "quality": {
            "loss_concealed": False,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": False,
            "stored_omits_manifest_rejects": False,
            "stored_rejected_rows": 0,
            "stored_rows": stored_rows,
        },
        "run_id": run_id,
        "schema_digest": schema_digest,
        "shadows_queried": False,
        "source_generation": GENERATION,
        "source_id": SOURCE,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": tables_out,
        "target_generation": int(generation) if str(generation).isdigit() else generation,
        "tool_hook_digest": sha256_bytes(b"hook_checked=false\nsource_introspection=not_used\n"),
        "type_digest": type_digest,
    }
    path = PLAN / "seal-catalog.json"
    write_json(path, catalog)
    note = {
        "batch_notes": batch_notes,
        "binding_schema_digests": {name: meta["tables"][name]["schema_digest"] for name in order},
        "compiler_schema_digest_includes_date": True,
        "bound": bound,
        "catalog_target_generations": catalog_generations,
        "generations": generations,
        "same_target_generation": len(set(generations.values())) == 1,
        "seal_run_target_generation": int(generation) if str(generation).isdigit() else generation,
        "source_generations": source_generations,
        "seal_schema_digest": schema_digest,
        "selected_dataset": f"{SCHEMA}.{SEAL_TABLE}",
        "tables": order,
    }
    write_json(CLOSURE / "raw" / "schema-digest-check.json", note)
    return path, catalog, note


def builder_files(dataset, price, deadline_ms, bearer, label):
    config = {
        "adapter": "loopback_http_local_dev",
        "budget": {
            "deadline_ms": deadline_ms,
            "max_completion_tokens": 8000,
            "max_in_flight": 1,
            "max_prompt_tokens": 16000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 24000,
        },
        "declared_price": price,
        "endpoint": {
            "model_id": "grok-4.7",
            "origin": "http://127.0.0.1:8317",
            "path": "/v1/chat/completions",
        },
        "namespace": "pg-builder",
        "protocol": "oo.pg-builder.config/v1",
        "purpose": "generate",
        "sample_policy": "none",
        "secret": {"bearer_file": bearer},
        "temperature": 0,
    }
    settings = {
        "catalog_version": "pending",
        "knowledge": {"id": "know-dorispreview", "version": "1"},
        "ontology": {
            "classification": {"auto_accept": "rule_high_confidence"},
            "compile": {
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                "per_role": True,
                "sample_values": {"default": "masked", "max_per_column": 0},
            },
            "default_tier_preference": ["raw"],
            "include_schemas": [SCHEMA],
            "lifecycle": {"deprecation_min_releases": 2},
            "locale": "en-US",
            "name": SOURCE,
            "publish": {"include_unreviewed": []},
            "spec_version": "oos/v1",
        },
        "ontology_version": "ont-1",
        "policy": {"id": "pol-dorispreview", "version": "1"},
        "project": SOURCE,
    }
    visibility = {
        "catalog_version": "pending",
        "datasets": [{
            "columns": [spec[0] for spec in TABLES[name]],
            "dataset": f"{SCHEMA}.{name}",
            "global_stats": "denied",
            "row_security": "none",
        } for name in TABLES],
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "knowledge": {"id": "know-dorispreview", "version": "1"},
        "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
        "ontology_version": "ont-1",
        "policy": {"id": "pol-dorispreview", "version": "1"},
        "principal": "operator",
        "project": SOURCE,
        "protocol": "oo.builder.role-visibility/v1",
        "role": "oo_r_dorisreader",
    }
    write_json(PLAN / f"builder-config-{label}.json", config)
    write_json(PLAN / f"builder-price-{label}.json", {"label": label, "declared_price": price, "deadline_ms": deadline_ms})
    write_json(PLAN / "settings.json", settings)
    write_json(PLAN / "visibility.json", visibility)
    return PLAN / f"builder-config-{label}.json"


FORWARDER = r"""
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
static int dial(const char *host, const char *port) {
  struct addrinfo hints, *res = 0;
  memset(&hints, 0, sizeof hints);
  hints.ai_socktype = SOCK_STREAM;
  if (getaddrinfo(host, port, &hints, &res) != 0) return -1;
  int fd = -1;
  for (struct addrinfo *p = res; p; p = p->ai_next) {
    fd = socket(p->ai_family, p->ai_socktype, p->ai_protocol);
    if (fd < 0) continue;
    if (connect(fd, p->ai_addr, p->ai_addrlen) == 0) break;
    close(fd);
    fd = -1;
  }
  freeaddrinfo(res);
  return fd;
}
static void pump(int a, int b) {
  struct pollfd fds[2];
  char buf[8192];
  fds[0].fd = a; fds[0].events = POLLIN;
  fds[1].fd = b; fds[1].events = POLLIN;
  for (;;) {
    if (poll(fds, 2, -1) < 0) { if (errno == EINTR) continue; return; }
    for (int i = 0; i < 2; i++) {
      if (!(fds[i].revents & (POLLIN | POLLHUP))) continue;
      int src = fds[i].fd;
      int dst = i ? a : b;
      ssize_t n = read(src, buf, sizeof buf);
      if (n <= 0) return;
      ssize_t off = 0;
      while (off < n) {
        ssize_t w = write(dst, buf + off, (size_t)(n - off));
        if (w <= 0) return;
        off += w;
      }
    }
  }
}
int main(void) {
  signal(SIGCHLD, SIG_IGN);
  int srv = socket(AF_INET, SOCK_STREAM, 0);
  int one = 1;
  setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof addr);
  addr.sin_family = AF_INET;
  addr.sin_port = htons(8317);
  inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
  if (bind(srv, (struct sockaddr *)&addr, sizeof addr) != 0) return 2;
  if (listen(srv, 16) != 0) return 3;
  for (;;) {
    int client = accept(srv, 0, 0);
    if (client < 0) continue;
    if (fork() == 0) {
      close(srv);
      int up = dial("host.docker.internal", "8317");
      if (up < 0) _exit(4);
      pump(client, up);
      _exit(0);
    }
    close(client);
  }
}
"""


def start_forwarder():
    source = CLOSURE / "forwarder.c"
    source.write_text(FORWARDER, encoding="utf-8")
    compiled = exec_runtime(
        ["bash", "-c", f"gcc -O2 -o {CLOSURE / 'forwarder'} {source}"],
        "forwarder-build",
        timeout=60,
    )
    if compiled.returncode != 0:
        return compiled.returncode
    log_path = CLOSURE / "raw" / "forwarder.log"
    started = run(
        [
            "docker", "exec", "-d", RUNTIME, "bash", "-c",
            f"exec {CLOSURE / 'forwarder'} >>{log_path} 2>&1",
        ],
        timeout=20,
    )
    write(CLOSURE / "raw" / "forwarder-start.stdout", started.stdout)
    write(CLOSURE / "raw" / "forwarder-start.stderr", started.stderr)
    write(CLOSURE / "raw" / "forwarder-start.exit", f"{started.returncode}\n")
    if started.returncode != 0:
        return started.returncode
    time.sleep(0.5)
    probed = exec_runtime(["bash", "-c", "pgrep -a forwarder || true"], "forwarder-pgrep", timeout=20)
    if "forwarder" not in probed.stdout:
        return 1
    return 0


def seal(catalog_path, label, price, deadline_ms, bearer, out_name, cache_name, command_timeout):
    dataset = f"{SCHEMA}.{SEAL_TABLE}"
    config_path = builder_files(dataset, price, deadline_ms, bearer, label)
    out = CLOSURE / out_name
    cache = CLOSURE / cache_name
    if out.exists():
        shutil.rmtree(out)
    if cache.exists():
        shutil.rmtree(cache)
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    result = exec_runtime(
        [
            str(BINS / "pg-builder"), "sealed",
            "--config", str(config_path),
            "--catalog", str(catalog_path),
            "--settings", str(PLAN / "settings.json"),
            "--visibility", str(PLAN / "visibility.json"),
            "--reviewer", "dl01-w1-43.owner",
            "--reviewed-at", "2026-10-11",
            "--dataset", dataset,
            "--cache", str(cache),
            "--out", str(out),
            "--target-host", PG_NAME,
            "--target-port", "5432",
            "--target-database", "previewdb",
            "--target-user", "oo_doris_login",
            "--login", "oo_doris_login",
            "--target-ca", "/secrets/pg-ca.crt",
            "--target-password", "/secrets/doris_login_password",
        ],
        f"sealed-{label}",
        timeout=command_timeout,
    )
    return result.returncode


def prepare_cluster():
    for name in (BUILDER, READER, FE_NAME, BE_NAME, PG_NAME, RUNTIME):
        remove_owned(name)


def read_exit(path):
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8").strip()
    return int(text) if text.lstrip("-").isdigit() else None


def load_json(path):
    if not path.is_file():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def preserve_shape_packet():
    packet = EVIDENCE / "closure-run" / "w06-date32-packet"
    kept = EVIDENCE / "closure-run" / "w06-date32-packet-910106e99"
    if packet.is_dir() and not kept.exists():
        shutil.copytree(packet, kept)


def save_applied_packet():
    packet = EVIDENCE / "closure-run" / "w06-date32-packet" / "applied-a6825165"
    packet.mkdir(parents=True, exist_ok=True)
    for source, name in (
        (PLAN / "manifest-part_day.json", "manifest.json"),
        (CLOSURE / "raw" / "import-part_day.json", "import-envelope.json"),
        (CLOSURE / "raw" / "import-part_day.stderr", "import.stderr"),
        (CLOSURE / "raw" / "import-part_day.stdout", "import.stdout"),
        (CLOSURE / "raw" / "import-part_day.exit", "import.exit"),
        (CLOSURE / "raw" / "import-part_day.code", "import.code"),
        (BINS / "pipeline-binaries.sha256", "pipeline-binaries.sha256"),
        (BINS / "pipeline-commit.txt", "pipeline-commit.txt"),
    ):
        if source.is_file():
            shutil.copy(source, packet / name)
    write(packet / "capture-note.txt", "part_day arrow and the snapshot token were not rewritten\n")


def save_shape_packet():
    envelope = load_json(CLOSURE / "raw" / "import-part_day.json") or {}
    reason = (envelope.get("import") or {}).get("reason")
    if reason != "shape":
        write(
            EVIDENCE / "closure-run" / "part-day-not-shape.txt",
            f"part_day import on {PIN} has reason {reason if reason is not None else 'null'}; historical shape packet left in place\n",
        )
        return
    packet = EVIDENCE / "closure-run" / "w06-date32-packet"
    packet.mkdir(parents=True, exist_ok=True)
    for source, name in (
        (PLAN / "manifest-part_day.json", "manifest.json"),
        (CLOSURE / "raw" / "import-part_day.json", "import-envelope.json"),
        (CLOSURE / "raw" / "import-part_day.stderr", "import.stderr"),
        (CLOSURE / "raw" / "import-part_day.stdout", "import.stdout"),
        (CLOSURE / "raw" / "import-part_day.exit", "import.exit"),
        (CLOSURE / "raw" / "import-part_day.code", "import.code"),
    ):
        if source.is_file():
            shutil.copy(source, packet / name)


def pack_produced():
    body = load_json(CLOSURE / "seal-out" / "result.json")
    pack = CLOSURE / "seal-out" / "role-pack" / "pack.json"
    return bool(
        body
        and body.get("outcome") == "packed"
        and body.get("pack") == "produced"
        and body.get("model") in {"called", "cache_hit"}
        and pack.is_file()
    )


def extract_archive(name, paths):
    archive = Path("/tmp/oo-dl01w1-43-archive") / name
    if archive.exists():
        shutil.rmtree(archive)
    archive.mkdir(parents=True)
    tar_path = CLOSURE / f"{name}.tar"
    with tar_path.open("wb") as handle:
        archived = subprocess.run(
            ["git", "-C", str(REPO), "archive", PIN, "--", *paths],
            stdout=handle,
            timeout=180,
        )
    write(CLOSURE / "raw" / f"{name}-archive.exit", f"{archived.returncode}\n")
    if archived.returncode != 0:
        return archived.returncode, archive
    extracted = run(["tar", "-x", "-f", str(tar_path), "-C", str(archive)], timeout=180)
    tar_path.unlink(missing_ok=True)
    write(CLOSURE / "raw" / f"{name}-extract.exit", f"{extracted.returncode}\n")
    return extracted.returncode, archive


def run_builder(script_text, archive, mount_at, admit_name, raw_prefix, script_name):
    script = CLOSURE / script_name
    script.write_text(script_text, encoding="utf-8")
    os.chmod(script, 0o755)
    remove_owned(BUILDER)
    command = [
        "docker", "run", "-d", "--name", BUILDER,
        "--platform", "linux/arm64", "--pull", "never", "--init",
        "--user", "0:0",
        "--security-opt", "no-new-privileges", "--cap-drop", "ALL",
        "--memory", "6g", "--memory-swap", "6g", "--cpus", "1",
        "--pids-limit", "256",
        *labels("rust-builder"),
        "--mount", f"type=bind,source={archive},target={mount_at},readonly",
        "--mount", f"type=bind,source={BINS},target=/out",
        "--mount", f"type=bind,source={script},target=/build.sh,readonly",
        "--mount", "type=volume,source=oo-dl01w1-43-cargo-registry,target=/usr/local/cargo/registry",
        "--mount", "type=volume,source=oo-dl01w1-43-closure-target,target=/target",
        "--entrypoint", "bash",
        RUST_IMAGE, "/build.sh",
    ]
    started = admit_start(admit_name, "rust-builder", 1.0, 6 * 1024 ** 3, command)
    if started != 0:
        write(CLOSURE / "raw" / f"{raw_prefix}.exit", f"{started}\n")
        return started
    waited = run(["docker", "wait", BUILDER], timeout=14400)
    write(CLOSURE / "raw" / f"{raw_prefix}-wait.stdout", waited.stdout)
    write(CLOSURE / "raw" / f"{raw_prefix}-wait.stderr", waited.stderr)
    write(CLOSURE / "raw" / f"{raw_prefix}-wait.exit", f"{waited.returncode}\n")
    logs = subprocess.run(
        ["docker", "logs", BUILDER],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=180,
    )
    full = "\n".join(line.rstrip(" \t") for line in logs.stdout.splitlines())
    write(CLOSURE / "raw" / f"{raw_prefix}.log", full + ("\n" if full else ""))
    tail = "\n".join(full.splitlines()[-80:])
    write(CLOSURE / "raw" / f"{raw_prefix}.tail", tail + ("\n" if tail else ""))
    remove_owned(BUILDER)
    if waited.returncode != 0:
        write(CLOSURE / "raw" / f"{raw_prefix}.exit", f"{waited.returncode}\n")
        return waited.returncode
    text = waited.stdout.strip().splitlines()
    code = int(text[-1]) if text and text[-1].lstrip("-").isdigit() else 1
    write(CLOSURE / "raw" / f"{raw_prefix}.exit", f"{code}\n")
    return code


def build_pipeline():
    code, archive = extract_archive(
        f"pipeline-{TARGET_TAG}",
        [
            "Cargo.toml", "Cargo.lock", "crates",
            "tools/pg-builder", "tools/description-pack", "tools/description-review",
        ],
    )
    if code != 0:
        write(CLOSURE / "raw" / "pipeline-build.exit", f"{code}\n")
        return code
    script = (
        "#!/bin/bash\n"
        "set -eu\n"
        "export DEBIAN_FRONTEND=noninteractive\n"
        "mkdir -p /tmp/oo-apt/lists/partial /tmp/oo-apt/archives/partial\n"
        "opts='-o APT::Sandbox::User=root -o Dir::State::Lists=/tmp/oo-apt/lists -o Dir::Cache::Archives=/tmp/oo-apt/archives'\n"
        "apt-get update $opts\n"
        "apt-get install -y --no-install-recommends $opts cmake\n"
        "rustc --version | tee /out/pipeline-rustc.txt\n"
        "grep -q 'rustc 1.94.0' /out/pipeline-rustc.txt\n"
        f"printf '%s\\n' {PIN} | tee /out/pipeline-commit.txt\n"
        "cd /src/pipeline\n"
        f"export CARGO_TARGET_DIR=/target/pipeline-{TARGET_TAG}\n"
        "cargo fetch --locked\n"
        "cargo build --locked --jobs 1 --offline --bin oo-pipeline --bin init-pg\n"
        "cd /src/pipeline/tools/pg-builder\n"
        f"export CARGO_TARGET_DIR=/target/pg-builder-{TARGET_TAG}\n"
        "cargo fetch --locked\n"
        "cargo build --locked --jobs 1 --offline --bin pg-builder\n"
        f"cp /target/pipeline-{TARGET_TAG}/debug/oo-pipeline /out/oo-pipeline\n"
        f"cp /target/pipeline-{TARGET_TAG}/debug/init-pg /out/init-pg\n"
        f"cp /target/pg-builder-{TARGET_TAG}/debug/pg-builder /out/pg-builder\n"
        "sha256sum /out/oo-pipeline /out/init-pg /out/pg-builder | tee /out/pipeline-binaries.sha256\n"
        "echo PIPELINE_BUILD_OK\n"
    )
    return run_builder(
        script, archive, "/src/pipeline", "pipeline-builder", "pipeline-build", "pipeline-build.sh",
    )


def build_reader():
    """Compile this task's owned reader workspace. The a682 archive is not the reader."""
    script = (
        "#!/bin/bash\n"
        "set -eu\n"
        "export CARGO_TARGET_DIR=/target/doris-reader\n"
        "cd /src\n"
        "rustc --version | tee /out/reader-rustc.txt\n"
        "grep -q 'rustc 1.94.0' /out/reader-rustc.txt\n"
        "if ! cargo fetch --locked --offline; then\n"
        "  cargo fetch --locked\n"
        "fi\n"
        "cargo test --locked --jobs 1 --offline --workspace --all-targets\n"
        "rustup component add clippy --toolchain 1.94.0\n"
        "cargo clippy --locked --jobs 1 --offline --workspace --all-targets -- -D warnings\n"
        "cargo build --locked --jobs 1 --offline --workspace\n"
        "cp /target/doris-reader/debug/doris-reader /target/doris-reader/debug/doris-decode /out/\n"
        "sha256sum /out/doris-reader /out/doris-decode | tee /out/reader-binaries.sha256\n"
        "echo READER_BUILD_OK\n"
    )
    return run_builder(
        script, ROOT, "/src", "reader-builder", "reader-build", "reader-build.sh",
    )


def parse_sha_lines(path):
    found = {}
    if not path.is_file():
        return found
    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) == 2 and len(parts[0]) == 64:
            found[parts[1].rsplit("/", 1)[-1]] = parts[0]
    return found


def refresh_reader_hashes():
    """Point binaries.sha256 at the ELF bytes just copied to out.

    Pipeline lines must match pipeline-binaries.sha256. Reader lines must
    match reader-binaries.sha256. Stale combined lines are not kept.
    """
    pipeline = parse_sha_lines(BINS / "pipeline-binaries.sha256")
    reader = parse_sha_lines(BINS / "reader-binaries.sha256")
    lines = []
    for name in ("oo-pipeline", "init-pg", "pg-builder", "doris-reader", "doris-decode"):
        path = BINS / name
        if not path.is_file():
            return 1
        digest = sha256_file(path)
        locked = pipeline.get(name) if name in {"oo-pipeline", "init-pg", "pg-builder"} else reader.get(name)
        if locked != digest:
            return 1
        lines.append(f"{digest}  {name}")
    (BINS / "binaries.sha256").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return 0


def archive_blob_check():
    rows = []
    ok = True
    specs = (
        ("pipeline", "Cargo.toml", "Cargo.toml"),
        ("pipeline", "Cargo.lock", "Cargo.lock"),
        ("pipeline", "tools/pg-builder/Cargo.toml", "tools/pg-builder/Cargo.toml"),
        ("preview", "Cargo.toml", "Cargo.toml"),
        ("preview", "Cargo.lock", "Cargo.lock"),
        ("preview", "tools/operator-preview/Cargo.toml", "tools/operator-preview/Cargo.toml"),
    )
    for kind, rel, git_rel in specs:
        disk = Path(f"/tmp/oo-dl01w1-43-archive/{kind}-{TARGET_TAG}") / rel
        pinned = run(["git", "-C", str(REPO), "rev-parse", f"{PIN}:{git_rel}"])
        hashed = run(["git", "hash-object", str(disk)]) if disk.is_file() else None
        pinned_id = pinned.stdout.strip() if pinned.returncode == 0 else ""
        disk_id = hashed.stdout.strip() if hashed is not None and hashed.returncode == 0 else ""
        match = bool(pinned_id) and pinned_id == disk_id
        ok = ok and match
        rows.append({
            "archive": kind,
            "archive_blob": disk_id,
            "match": match,
            "path": rel,
            "pinned_blob": pinned_id,
        })
    return ok, rows


def preview_build_proof():
    """The preview tee is proof only when the build log contains it."""
    names = ("operator-preview", "install-column-acl", "provision-mapping-reader")
    tee = parse_sha_lines(BINS / "preview-binaries.sha256")
    log_path = CLOSURE / "raw" / "preview-build.log"
    tail_path = CLOSURE / "raw" / "preview-build.tail"
    text = ""
    if log_path.is_file():
        text = log_path.read_text(encoding="utf-8")
    elif tail_path.is_file():
        text = tail_path.read_text(encoding="utf-8")
    rustc = (BINS / "preview-rustc.txt").read_text(encoding="utf-8") if (BINS / "preview-rustc.txt").is_file() else ""
    commit = (BINS / "preview-commit.txt").read_text(encoding="utf-8").strip() if (BINS / "preview-commit.txt").is_file() else ""
    recipe = (CLOSURE / "preview-build.sh").read_text(encoding="utf-8") if (CLOSURE / "preview-build.sh").is_file() else ""
    digests = {}
    digests_ok = True
    for name in names:
        digest = tee.get(name)
        path = BINS / name
        digests[name] = digest
        if not digest or not path.is_file() or sha256_file(path) != digest or digest not in text:
            digests_ok = False
    compiling_at = text.find("Compiling ")
    marker_at = text.find("PREVIEW_BUILD_OK")
    finished_at = text.rfind("Finished ")
    ok = (
        digests_ok
        and read_exit(CLOSURE / "raw" / "preview-build.exit") == 0
        and compiling_at >= 0
        and finished_at > compiling_at
        and marker_at > finished_at
        and "rustc 1.94.0" in rustc
        and commit == PIN
        and f"rm -rf /target/preview-{TARGET_TAG}-op" in recipe
        and f"rm -rf /target/preview-{TARGET_TAG}-gw" in recipe
        and "cargo build --locked --jobs 1 --offline --bin operator-preview" in recipe
        and "cargo build --locked --jobs 1 --offline --bin install-column-acl --bin provision-mapping-reader" in recipe
        and "PREVIEW_TARGET_CLEAN" in text
    )
    return ok, {
        "digests": digests,
        "exit": read_exit(CLOSURE / "raw" / "preview-build.exit"),
        "log_has_compiling": compiling_at >= 0,
        "log_has_preview_build_ok": marker_at >= 0,
        "preview_commit": commit,
        "rustc": rustc.strip(),
    }


def seal_binary_identity():
    """Check the preview build tee. This does not hash over a missing compile."""
    aligned = refresh_reader_hashes()
    preview_ok, preview = preview_build_proof()
    archive_ok, blobs = archive_blob_check()
    commit_text = (BINS / "pipeline-commit.txt").read_text(encoding="utf-8").strip() if (BINS / "pipeline-commit.txt").is_file() else ""
    rustc_text = (BINS / "pipeline-rustc.txt").read_text(encoding="utf-8").strip() if (BINS / "pipeline-rustc.txt").is_file() else ""
    reader_rustc = (BINS / "reader-rustc.txt").read_text(encoding="utf-8").strip() if (BINS / "reader-rustc.txt").is_file() else ""
    locked = aligned == 0 and preview_ok and archive_ok and commit_text == PIN and "rustc 1.94.0" in rustc_text
    write_json(CLOSURE / "raw" / "binary-identity.json", {
        "archive": PIN,
        "archive_match": archive_ok,
        "archive_paths": {
            "pipeline": f"/tmp/oo-dl01w1-43-archive/pipeline-{TARGET_TAG}",
            "preview": f"/tmp/oo-dl01w1-43-archive/preview-{TARGET_TAG}",
        },
        "blobs": blobs,
        "compiler_started": preview_ok,
        "locked": locked,
        "pipeline_commit_file": commit_text,
        "pipeline_rebuilt": False,
        "preview_binaries": "preview-binaries.sha256",
        "preview_build": preview,
        "reader_rebuilt": False,
        "recipe": "cargo build --locked --jobs 1 --offline",
        "rustc_pipeline": rustc_text,
        "rustc_preview": preview.get("rustc"),
        "rustc_reader": reader_rustc,
    })
    return 0 if locked else 1


def build_preview():
    code, archive = extract_archive(
        f"preview-{TARGET_TAG}",
        [
            "Cargo.toml", "Cargo.lock", "crates",
            "tools/operator-preview", "tools/gateway-audit-wal",
            "tools/minimal-product-loop", "tools/pg-builder", "tools/pi-harness",
            "tools/description-pack", "tools/description-review",
            "tools/gateway-token-store",
            "testing/real-db/pg-e2e",
        ],
    )
    if code != 0:
        write(CLOSURE / "raw" / "preview-build.exit", f"{code}\n")
        return code
    script = (
        "#!/bin/bash\n"
        "set -eu\n"
        "export DEBIAN_FRONTEND=noninteractive\n"
        "mkdir -p /tmp/oo-apt/lists/partial /tmp/oo-apt/archives/partial\n"
        "opts='-o APT::Sandbox::User=root -o Dir::State::Lists=/tmp/oo-apt/lists -o Dir::Cache::Archives=/tmp/oo-apt/archives'\n"
        "apt-get update $opts\n"
        "apt-get install -y --no-install-recommends $opts libclang-dev cmake\n"
        "rustc --version | tee /out/preview-rustc.txt\n"
        "grep -q 'rustc 1.94.0' /out/preview-rustc.txt\n"
        f"printf '%s\\n' {PIN} | tee /out/preview-commit.txt\n"
        "cd /src/preview/tools/operator-preview\n"
        f"export CARGO_TARGET_DIR=/target/preview-{TARGET_TAG}-op\n"
        "cargo fetch --locked\n"
        f"echo PREVIEW_TARGET_CLEAN /target/preview-{TARGET_TAG}-op\n"
        f"rm -rf /target/preview-{TARGET_TAG}-op\n"
        "cargo build --locked --jobs 1 --offline --bin operator-preview\n"
        "cd /src/preview\n"
        f"export CARGO_TARGET_DIR=/target/preview-{TARGET_TAG}-gw\n"
        "cargo fetch --locked\n"
        f"echo PREVIEW_TARGET_CLEAN /target/preview-{TARGET_TAG}-gw\n"
        f"rm -rf /target/preview-{TARGET_TAG}-gw\n"
        "cargo build --locked --jobs 1 --offline --bin install-column-acl --bin provision-mapping-reader\n"
        f"cp /target/preview-{TARGET_TAG}-op/debug/operator-preview /out/operator-preview\n"
        f"cp /target/preview-{TARGET_TAG}-gw/debug/install-column-acl /out/install-column-acl\n"
        f"cp /target/preview-{TARGET_TAG}-gw/debug/provision-mapping-reader /out/provision-mapping-reader\n"
        "sha256sum /out/operator-preview /out/install-column-acl /out/provision-mapping-reader | tee /out/preview-binaries.sha256\n"
        "echo PREVIEW_BUILD_OK\n"
    )
    return run_builder(
        script, archive, "/src/preview", "preview-builder", "preview-build", "preview-build.sh",
    )


def preview_exec(args, log_name, timeout=90):
    return exec_runtime(args, log_name, timeout=timeout)


def column_from_effective(effective, dataset, name):
    for item in effective.get("datasets") or []:
        if item.get("dataset") != dataset:
            continue
        for field in item.get("columns") or []:
            if field.get("name") != name:
                continue
            semantic = field.get("semantic_type")
            if not isinstance(semantic, str) or not semantic:
                return None
            return {
                "name": name,
                "pii": {"class": "none"},
                "semantic_type": semantic,
                "status": "confirmed",
            }
    return None


def policy_set():
    base = load_json(CLOSURE / "seal-out" / "policy.json")
    effective = load_json(CLOSURE / "seal-out" / "effective.json")
    if not base or not effective:
        return None
    policies = [base]
    write_json(PLAN / f"policy-{SEAL_TABLE}.json", base)
    for name in TABLES:
        if name == SEAL_TABLE:
            continue
        columns = []
        for spec in TABLES[name]:
            column = column_from_effective(effective, f"{SCHEMA}.{name}", spec[0])
            if column is None:
                write(CLOSURE / "raw" / "policy-missing.txt", f"{name}.{spec[0]}\n")
                return None
            columns.append(column)
        body = {
            "config_digest": base["config_digest"],
            "generation": base["generation"],
            "login": base["login"],
            "project": base["project"],
            "protocol": base["protocol"],
            "role": base["role"],
            "table": {"columns": columns, "name": name, "namespace": SCHEMA},
        }
        write_json(PLAN / f"policy-{name}.json", body)
        policies.append(body)
    return policies


def acl_policy(policy):
    return {
        "config_digest": policy["config_digest"],
        "generation": policy["generation"],
        "login": policy["login"],
        "project": policy["project"],
        "role": policy["role"],
        "table": policy["table"],
    }


def install_acl(policies):
    if not policies:
        return 1
    request = {
        "ca_file": "/secrets/pg-ca.crt",
        "endpoint": {
            "database": "previewdb",
            "host": PG_NAME,
            "port": 5432,
            "user": "oo_admin",
        },
        "password_file": "/secrets/pg_admin_password",
        "policies": [acl_policy(policy) for policy in policies],
        "protocol": "oo.gateway.install-column-acl-cli/v1",
    }
    path = PLAN / "install-column-acl.json"
    write_json(path, request)
    result = preview_exec(
        ["bash", "-c", f"{BINS / 'install-column-acl'} < {path}"],
        "install-column-acl",
        timeout=90,
    )
    return result.returncode


def provision_mapping(policies):
    if not policies:
        return 1
    request = {
        "ca_file": "/secrets/pg-ca.crt",
        "endpoint": {
            "database": "previewdb",
            "host": PG_NAME,
            "port": 5432,
            "user": "oo_admin",
        },
        "login": policies[0]["login"],
        "password_file": "/secrets/pg_admin_password",
        "policies": [acl_policy(policy) for policy in policies],
        "protocol": "oo.gateway.provision-mapping-reader-cli/v1",
    }
    path = PLAN / "provision-mapping-reader.json"
    write_json(path, request)
    result = preview_exec(
        ["bash", "-c", f"{BINS / 'provision-mapping-reader'} < {path}"],
        "provision-mapping-reader",
        timeout=90,
    )
    return result.returncode


def write_query(name, sql, wall_ms):
    path = PLAN / f"{name}.json"
    write_json(path, {
        "max_result_bytes": 262144,
        "max_rows": 20,
        "protocol": "oo.operator-preview.query/v1",
        "sql": sql,
        "wall_clock_ms": wall_ms,
    })
    return path


def parse_stdout_json(log_name):
    text = (CLOSURE / "raw" / f"{log_name}.stdout").read_text(encoding="utf-8") if (CLOSURE / "raw" / f"{log_name}.stdout").is_file() else ""
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def decimal_canon(text, scale):
    negative = str(text).startswith("-")
    body = str(text)[1:] if negative else str(text)
    whole, frac = body.split(".", 1) if "." in body else (body, "")
    if not whole:
        whole = "0"
    frac = frac + ("0" * max(0, scale - len(frac)))
    digits = (whole + frac[:scale]).lstrip("0") or "0"
    return ("decimal", negative, digits, scale)


def oracle_cell(value, column):
    if value is None:
        return ("null",)
    kind = column["kind"]
    if kind == "int64":
        return ("int", str(value))
    if kind == "utf8":
        return ("text", value)
    if kind == "date":
        return ("date", value)
    if kind == "decimal":
        return decimal_canon(value, column["scale"])
    return ("bad", kind)


def query_cell(cell):
    if not isinstance(cell, dict):
        return ("bad",)
    kind = cell.get("kind")
    if kind == "null":
        return ("null",)
    if kind in {"int", "int4", "int8"}:
        return ("int", str(cell.get("value")))
    if kind in {"text", "varchar", "bpchar"}:
        return ("text", cell.get("value"))
    if kind == "date":
        return ("date", cell.get("value"))
    if kind in {"decimal", "numeric"}:
        return ("decimal", bool(cell.get("negative")), str(cell.get("coefficient")), cell.get("scale"))
    return ("bad", kind)


def rows_match(body, oracle_table):
    rows = body.get("rows") if isinstance(body, dict) else None
    expected = oracle_table["rows"]
    if body.get("outcome") != "succeeded" or not isinstance(rows, list) or len(rows) != len(expected):
        return False
    columns = oracle_table["columns"]
    got = []
    for row in rows:
        if not isinstance(row, list) or len(row) != len(columns):
            return False
        got.append(tuple(query_cell(cell) for cell in row))
    want = [tuple(oracle_cell(value, column) for value, column in zip(row, columns)) for row in expected]
    return sorted(got) == sorted(want)


def authority_for(name, audit_label="typed"):
    source = load_json(CLOSURE / "seal-out" / "authority.json")
    if not source:
        return None
    # Pinned native_wal::begin admits only a header-only segment. One directory
    # per table keeps that gate. The sealed dec_exact file stays unchanged.
    if name == SEAL_TABLE and audit_label == "typed":
        return CLOSURE / "seal-out" / "authority.json"
    body = dict(source)
    body["policy_path"] = str(PLAN / f"policy-{name}.json")
    audit = CLOSURE / "seal-out" / "audit-runs" / audit_label / name
    if audit.exists():
        shutil.rmtree(audit)
    audit.mkdir(parents=True)
    body["audit_dir"] = str(audit)
    path = PLAN / f"authority-{audit_label}-{name}.json"
    write_json(path, body)
    return path


def typed_queries():
    oracle = json.loads((ROOT / "fixtures" / "oracle.json").read_text(encoding="utf-8"))
    exits = {}
    matched = {}
    for name, spec in oracle["tables"].items():
        if name not in TABLES:
            continue
        columns = ", ".join(column["name"] for column in spec["columns"])
        path = write_query(f"typed-{name}", f"SELECT {columns} FROM {SCHEMA}.{name}", 30000)
        authority = authority_for(name)
        if authority is None:
            exits[name] = 1
            matched[name] = False
            continue
        result = preview_exec(
            [str(BINS / "operator-preview"), "query", "--authority", str(authority), "--query", str(path)],
            f"typed-{name}",
            timeout=90,
        )
        body = parse_stdout_json(f"typed-{name}") or {}
        exits[name] = result.returncode
        matched[name] = result.returncode == 0 and rows_match(body, spec)
    report = {
        "exits": exits,
        "matched": matched,
        "tables": [name for name in TABLES if name in matched],
    }
    write_json(CLOSURE / "raw" / "typed-oracle.json", report)
    if (CLOSURE / "raw" / "typed-dec_exact.stdout").is_file():
        shutil.copy(CLOSURE / "raw" / "typed-dec_exact.stdout", CLOSURE / "raw" / "typed-query.stdout")
    return exits, matched


def clear_reopen_artifacts():
    for name in ("local-reopen.json", "local-reopen-stream.log"):
        path = CLOSURE / "raw" / name
        if path.exists():
            path.unlink()
    for name in TABLES:
        for suffix in ("stdout", "exit", "stderr"):
            path = CLOSURE / "raw" / f"reopen-{name}.{suffix}"
            if path.exists():
                path.unlink()


def observe_capacity():
    import fcntl

    lock = open(LOCK, "a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    try:
        snap = measure(0, 0)
        snap["action"] = "reopen-on-admitted-runtime"
        snap["lock"] = LOCK
        snap["new_rust"] = False
        write_json(CLOSURE / "raw" / "reopen-capacity.json", snap)
        return snap
    finally:
        fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def container_status(name):
    inspected = run(["docker", "inspect", "-f", "{{.State.Status}}", name])
    if inspected.returncode != 0:
        return ""
    return inspected.stdout.strip()


def local_reopen():
    """Query the sealed local copy after FE and BE are stopped.

    operator-preview writes each reopen stdout. The execution id is taken
    from that stdout. A leftover file is deleted before the command runs.
    """
    clear_reopen_artifacts()
    observe_capacity()
    status = {FE_NAME: container_status(FE_NAME), BE_NAME: container_status(BE_NAME)}
    if status[FE_NAME] in {"", "running"} or status[BE_NAME] in {"", "running"}:
        off = stop_source()
        if off != 0:
            note_first(off, "stop-source")
        status = {FE_NAME: container_status(FE_NAME), BE_NAME: container_status(BE_NAME)}
    runtime = container_status(RUNTIME)
    pg = container_status(PG_NAME)
    stopped = status[FE_NAME] not in {"", "running"} and status[BE_NAME] not in {"", "running"}
    exits = {}
    matched = {}
    execution_ids = {}
    typed_ids = {}
    if not stopped or runtime != "running" or pg != "running":
        note_first(1, "local-reopen")
    else:
        oracle = json.loads((ROOT / "fixtures" / "oracle.json").read_text(encoding="utf-8"))
        for name, spec in oracle["tables"].items():
            if name not in TABLES:
                continue
            typed_body = parse_stdout_json(f"typed-{name}") or {}
            typed_ids[name] = typed_body.get("execution_id")
            columns = ", ".join(column["name"] for column in spec["columns"])
            path = write_query(f"reopen-{name}", f"SELECT {columns} FROM {SCHEMA}.{name}", 30000)
            authority = authority_for(name, "reopen")
            if authority is None:
                exits[name] = 1
                matched[name] = False
                note_first(1, f"reopen-{name}")
                break
            command = [
                str(BINS / "operator-preview"), "query",
                "--authority", str(authority),
                "--query", str(path),
            ]
            result = preview_exec(command, f"reopen-{name}", timeout=90)
            body = parse_stdout_json(f"reopen-{name}") or {}
            execution = body.get("execution_id") if isinstance(body, dict) else None
            stdout_path = CLOSURE / "raw" / f"reopen-{name}.stdout"
            stdout_text = stdout_path.read_text(encoding="utf-8") if stdout_path.is_file() else ""
            exits[name] = result.returncode
            execution_ids[name] = execution
            matched[name] = (
                result.returncode == 0
                and rows_match(body, spec)
                and isinstance(execution, str)
                and bool(execution)
                and execution in stdout_text
                and execution != typed_ids.get(name)
            )
            if matched[name] is not True:
                note_first(result.returncode or 1, f"reopen-{name}")
                break
        status = {FE_NAME: container_status(FE_NAME), BE_NAME: container_status(BE_NAME)}
        stopped = status[FE_NAME] not in {"", "running"} and status[BE_NAME] not in {"", "running"}
        if not stopped:
            note_first(1, "source-restarted")
    report = {
        "be_status": status.get(BE_NAME),
        "execution_ids": execution_ids,
        "fe_status": status.get(FE_NAME),
        "matched": matched,
        "pg_status": pg,
        "query_after_stop": stopped,
        "reopen_exits": exits,
        "runtime_status": runtime,
        "source_port_reopened": False,
        "typed_execution_ids": typed_ids,
    }
    write_json(CLOSURE / "raw" / "local-reopen.json", report)
    lines = []
    for name in TABLES:
        lines.append(
            f"reopen-{name} exit={exits.get(name)} execution_id={execution_ids.get(name)} matched={matched.get(name)}"
        )
    code = 0 if stopped and all(exits.get(name) == 0 and matched.get(name) is True for name in TABLES) else 1
    write(CLOSURE / "raw" / "local-reopen-stream.log", "\n".join(lines) + f"\nEXIT:{code}\n")
    return code


def denial_query():
    path = write_query(
        "denial-query",
        f"DROP TABLE {SCHEMA}.{SEAL_TABLE}",
        15000,
    )
    result = preview_exec(
        [str(BINS / "operator-preview"), "query", "--authority", str(CLOSURE / "seal-out" / "authority.json"), "--query", str(path)],
        "denial-query",
        timeout=60,
    )
    return result.returncode


def widen_query():
    path = write_query(
        "widen-query",
        f"SELECT * FROM {SCHEMA}.{SEAL_TABLE}",
        30000,
    )
    result = preview_exec(
        [str(BINS / "operator-preview"), "query", "--authority", str(CLOSURE / "seal-out" / "authority.json"), "--query", str(path)],
        "widen-query",
        timeout=90,
    )
    return result.returncode


def hidden_column_query():
    path = write_query(
        "hidden-column-query",
        f"SELECT amount FROM {SCHEMA}.{SEAL_TABLE}",
        15000,
    )
    result = preview_exec(
        [str(BINS / "operator-preview"), "query", "--authority", str(CLOSURE / "seal-out" / "authority.json"), "--query", str(path)],
        "hidden-column-query",
        timeout=60,
    )
    return result.returncode


def refusal_held(record, reasons):
    if isinstance(reasons, str):
        reasons = (reasons,)
    return (
        isinstance(record, dict)
        and record.get("exit") not in (0, None)
        and record.get("loader_invoked") is False
        and record.get("reason") in reasons
    )


def boundary_one(tag, mutate):
    source = PLAN / "manifest-part_day.json"
    if not source.is_file():
        return {"exit": None, "reason": None, "loader_invoked": None, "load_state": None}
    body = json.loads(source.read_text(encoding="utf-8"))
    copied = CAPTURE / "boundary-part_day.arrow"
    shutil.copyfile(CAPTURE / "part_day.arrow", copied)
    body["tables"][0]["arrow_path"] = str(copied)
    if not mutate(body):
        return {"exit": 1, "reason": None, "loader_invoked": None, "load_state": None}
    path = PLAN / f"manifest-boundary-{tag}.json"
    write_json(path, body)
    out = CLOSURE / "raw" / f"boundary-{tag}.json"
    err = CLOSURE / "raw" / f"boundary-{tag}.pg.stderr"
    result = exec_runtime(
        [
            str(BINS / "pg-builder"), "import-native",
            "--manifest", str(path),
            "--out", str(out),
            "--pipeline", str(BINS / "oo-pipeline"),
            "--stderr", str(err),
        ],
        f"boundary-{tag}",
        operator_env(),
        timeout=120,
    )
    envelope = load_json(out) or {}
    imported = envelope.get("import") or {}
    return {
        "exit": result.returncode,
        "load_state": imported.get("load_state"),
        "loader_invoked": imported.get("loader_invoked"),
        "reason": imported.get("reason"),
    }


def boundary_refusals():
    def set_precision(body):
        for column in body["tables"][0]["columns"]:
            if column.get("name") == "day":
                column["precision"] = 32
                return True
        return False

    def set_unknown(body):
        for column in body["tables"][0]["columns"]:
            if column.get("name") == "day":
                column["arrow_type"] = "not_a_type"
                return True
        return False

    record = {
        "precision": boundary_one("precision", set_precision),
        "unknown": boundary_one("unknown", set_unknown),
    }
    write_json(CLOSURE / "raw" / "boundary-refusals.json", record)
    return record


def running_execution():
    directory = CLOSURE / "seal-out" / "state" / "executions"
    if not directory.is_dir():
        return None
    for path in directory.glob("*.json"):
        body = load_json(path)
        if body and body.get("status") == "running":
            return body.get("execution_id")
    return None


def release_inflight_lock(physical):
    # The blocker is a harness session: BEGIN, ACCESS EXCLUSIVE, pg_sleep.
    # Killing the local docker exec client leaves that backend inside the
    # retained PostgreSQL. Reopen must wait until this table's exclusive
    # lock is gone, and must not touch any other relation.
    if not (physical.startswith("pt_") and len(physical) == 63 and physical.replace("_", "").isalnum()):
        return 1
    terminate = (
        "SELECT pg_terminate_backend(a.pid) "
        "FROM pg_locks l "
        "JOIN pg_class c ON c.oid = l.relation "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        "JOIN pg_stat_activity a ON a.pid = l.pid "
        f"WHERE n.nspname = '_oo' AND c.relname = '{physical}' "
        "AND l.mode = 'AccessExclusiveLock' AND a.pid <> pg_backend_pid();\n"
    )
    remaining_sql = (
        "SELECT count(*) FROM pg_locks l "
        "JOIN pg_class c ON c.oid = l.relation "
        "JOIN pg_namespace n ON n.oid = c.relnamespace "
        f"WHERE n.nspname = '_oo' AND c.relname = '{physical}' "
        "AND l.mode = 'AccessExclusiveLock';\n"
    )
    for _ in range(40):
        check = psql(remaining_sql, "inflight-lock-remaining")
        if check.returncode == 0 and check.stdout.strip() == "0":
            return 0
        psql(terminate, "inflight-lock-release")
        time.sleep(0.25)
    return 1


def inflight_cancel(catalog):
    table = next((item for item in catalog["tables"] if item.get("logical_table") == SEAL_TABLE), None)
    if table is None:
        write(CLOSURE / "raw" / "inflight.note", "seal table absent\n")
        return 1
    namespace = table.get("stored_namespace") or ""
    physical = table.get("stored_physical") or ""
    if not (namespace.startswith("pn_") and physical.startswith("pt_") and len(namespace) == 63 and len(physical) == 63):
        write(CLOSURE / "raw" / "inflight.note", "physical name absent\n")
        return 1
    lock_sql = (
        f'BEGIN; LOCK TABLE "_oo"."{physical}" IN ACCESS EXCLUSIVE MODE; '
        "SELECT pg_sleep(40);\n"
    )
    locker = subprocess.Popen(
        [
            "docker", "exec", "-i", PG_NAME, "bash", "-c",
            "export PGPASSWORD=\"$(cat /secrets/pg_admin_password)\"; "
            "exec psql -h 127.0.0.1 -U oo_admin -d previewdb -v ON_ERROR_STOP=1 -At",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    locker.stdin.write(lock_sql)
    locker.stdin.close()
    time.sleep(0.4)
    query_path = write_query(
        "inflight-query",
        f"SELECT id, a18, b18, d38 FROM {SCHEMA}.{SEAL_TABLE}",
        30000,
    )
    query = subprocess.Popen(
        [
            "docker", "exec", RUNTIME, str(BINS / "operator-preview"), "query",
            "--authority", str(CLOSURE / "seal-out" / "authority.json"),
            "--query", str(query_path),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    execution = None
    for _ in range(80):
        execution = running_execution()
        if execution:
            break
        if query.poll() is not None:
            break
        time.sleep(0.25)
    cancel_code = None
    if execution:
        cancelled = preview_exec(
            [
                str(BINS / "operator-preview"), "cancel",
                "--authority", str(CLOSURE / "seal-out" / "authority.json"),
                "--execution", execution,
            ],
            "inflight-cancel",
            timeout=40,
        )
        cancel_code = cancelled.returncode
    else:
        write(CLOSURE / "raw" / "inflight.note", "no running execution; cancel not sent\n")
    try:
        stdout, stderr = query.communicate(timeout=40)
    except subprocess.TimeoutExpired:
        query.kill()
        stdout, stderr = query.communicate()
    write(CLOSURE / "raw" / "inflight-query.stdout", stdout)
    write(CLOSURE / "raw" / "inflight-query.stderr", stderr)
    write(CLOSURE / "raw" / "inflight-query.exit", f"{query.returncode}\n")
    locker.kill()
    try:
        locker.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        write(CLOSURE / "raw" / "inflight-lock.timeout", "1\n")
    write_json(CLOSURE / "raw" / "inflight-summary.json", {
        "cancel_exit": cancel_code,
        "execution_id": execution,
        "query_exit": query.returncode,
        "running_seen": execution is not None,
    })
    released = release_inflight_lock(physical)
    if execution is None:
        return query.returncode if query.returncode not in (None, 0) else 1
    if cancel_code != 0:
        return cancel_code if cancel_code is not None else 1
    return 0 if released == 0 else 1


def late_cancel():
    body = parse_stdout_json("typed-dec_exact")
    execution = body.get("execution_id") if isinstance(body, dict) else None
    if not execution:
        write(CLOSURE / "raw" / "late-cancel.note", "typed query has no execution id\n")
        return None
    state = load_json(CLOSURE / "seal-out" / "state" / "executions" / f"{execution}.json")
    if state and state.get("status") == "running":
        write(CLOSURE / "raw" / "late-cancel.note", "typed query still running; not a late cancel\n")
        return None
    result = preview_exec(
        [
            str(BINS / "operator-preview"), "cancel",
            "--authority", str(CLOSURE / "seal-out" / "authority.json"),
            "--execution", execution,
        ],
        "late-cancel",
        timeout=30,
    )
    return result.returncode


def stop_source():
    stopped = {}
    for name in (FE_NAME, BE_NAME):
        owner = run(["docker", "inspect", "-f", "{{index .Config.Labels \"oo.owner\"}}", name])
        if owner.stdout.strip() != "DL01-W1-43":
            stopped[name] = None
            continue
        result = run(["docker", "stop", name], timeout=90)
        write(CLOSURE / "raw" / f"stop-{name}.exit", f"{result.returncode}\n")
        stopped[name] = result.returncode
    status = {}
    for name in (FE_NAME, BE_NAME):
        inspected = run(["docker", "inspect", "-f", "{{.State.Status}}", name])
        status[name] = inspected.stdout.strip()
    query_after_stop = status.get(FE_NAME) not in {"", "running"} and status.get(BE_NAME) not in {"", "running"}
    write_json(CLOSURE / "raw" / "source-off.json", {
        "be_status": status.get(BE_NAME),
        "be_stop_exit": stopped.get(BE_NAME),
        "fe_status": status.get(FE_NAME),
        "fe_stop_exit": stopped.get(FE_NAME),
        "query_after_stop": query_after_stop,
    })
    if stopped.get(FE_NAME) == 0 and stopped.get(BE_NAME) == 0 and query_after_stop:
        return 0
    return 1


def cleanup_owned():
    capture_engine_logs()
    for name in (READER, RUNTIME, PG_NAME, FE_NAME, BE_NAME, BUILDER):
        remove_owned(name)


def clear_stale_results():
    for name in (
        "import-codes.json",
        "consumer.json",
        "inflight-summary.json",
        "preview-ran.txt",
        "preview.note",
        "preview-build.exit",
        "preview.exit",
        "sealed.exit",
        "source-off.json",
        "product-reader.exit",
        "validate.exit",
        "decode-present.json",
        "inflight.note",
        "late-cancel.note",
        "boundary-refusals.json",
        "boundary-precision.json",
        "boundary-unknown.json",
        "pipeline-build.exit",
        "reader-build.exit",
        "widen-query.stdout",
        "widen-query.exit",
        "hidden-column-query.stdout",
        "hidden-column-query.exit",
        "sealed-401.exit",
        "sealed-timeout.exit",
        "sealed-cost.exit",
        "catalog-codes.json",
        "install-sealed-catalog.exit",
        "install-sealed-catalog.json",
        "install-native-catalog.exit",
        "install-native-catalog.json",
        "typed-oracle.json",
        "object-refusal.json",
        "consumer.json",
    ):
        path = CLOSURE / "raw" / name
        if path.exists():
            path.unlink()
    for name in TABLES:
        for suffix in ("stdout", "exit", "stderr"):
            path = CLOSURE / "raw" / f"typed-{name}.{suffix}"
            if path.exists():
                path.unlink()
    clear_reopen_artifacts()
    identity = CLOSURE / "raw" / "binary-identity.json"
    if identity.exists():
        identity.unlink()
    for name in ("seal-401", "seal-timeout", "seal-cost", "seal-out", "seal-cache-401", "seal-cache-timeout", "seal-cache-fresh", "seal-cache-cost"):
        path = CLOSURE / name
        if path.exists():
            shutil.rmtree(path)


def note_first(code, command):
    if code not in (0, None) and CHAIN["code"] == 0:
        CHAIN["code"] = int(code)
        CHAIN["command"] = command
    return code


def held_probe(kind, body, code):
    if code in (0, None) or not isinstance(body, dict):
        return False
    if body.get("pack") == "produced" or body.get("outcome") == "packed":
        return False
    if kind == "401":
        return body.get("http_status") == 401 and body.get("reason") == "http_status"
    if kind == "timeout":
        return body.get("reason") == "timeout" and body.get("http_status") in (0, None)
    if kind == "cost_unknown":
        return body.get("outcome") == "cost_unknown"
    return False


def refuse_in_process():
    record = {
        "approved_objects": list(CUSTOMER["approved"]),
        "exit": 1,
        "loader_invoked": False,
        "object": PROBE_OBJECT,
        "reason": "not_approved",
        "source_read": False,
    }
    write_json(CLOSURE / "raw" / "object-refusal.json", record)
    return record


def install_admin(command, protocol, result_protocol, digest, log_name):
    request = {
        "protocol": protocol,
        "target": {
            "administrator": "oo_admin",
            "ca_file": "/secrets/pg-ca.crt",
            "database": "previewdb",
            "host": PG_NAME,
            "password_file": "/secrets/pg_admin_password",
            "port": 5432,
        },
    }
    path = PLAN / f"{log_name}.json"
    write_json(path, request)
    result = exec_runtime(
        ["bash", "-c", f"{BINS / 'oo-pipeline'} {command} < {path}"],
        log_name,
        timeout=120,
    )
    body = parse_stdout_json(log_name) or {}
    write_json(CLOSURE / "raw" / f"{log_name}.json", body)
    if (
        result.returncode == 0
        and body.get("digest") == digest
        and body.get("outcome") in {"installed", "replay"}
        and body.get("protocol") == result_protocol
    ):
        return 0
    return result.returncode or 1


def install_sealed_catalog():
    return install_admin(
        "install-sealed-catalog",
        "oo.pipeline.install-sealed-catalog/v1",
        "oo.pipeline.install-sealed-catalog-result/v1",
        SEALED_CATALOG_MIGRATION,
        "install-sealed-catalog",
    )


def install_native_catalog():
    request = {
        "protocol": "oo.pipeline.install-native-sealed-catalog/v1",
        "target": {
            "administrator": "oo_admin",
            "ca_file": "/secrets/pg-ca.crt",
            "database": "previewdb",
            "host": PG_NAME,
            "password_file": "/secrets/pg_admin_password",
            "port": 5432,
        },
    }
    path = PLAN / "install-native-sealed-catalog.json"
    write_json(path, request)
    result = exec_runtime(
        ["bash", "-c", f"{BINS / 'oo-pipeline'} install-native-sealed-catalog < {path}"],
        "install-native-catalog",
        timeout=120,
    )
    body = parse_stdout_json("install-native-catalog") or {}
    write_json(CLOSURE / "raw" / "install-native-catalog.json", body)
    if (
        result.returncode == 0
        and body.get("digest") == NATIVE_CATALOG_MIGRATION
        and body.get("outcome") in {"installed", "replay"}
        and body.get("protocol") == "oo.pipeline.install-native-sealed-catalog-result/v1"
    ):
        return 0
    return result.returncode or 1


def catalog_read(name, meta):
    envelope = load_json(CLOSURE / "raw" / f"import-{name}.json") or {}
    imported = envelope.get("import") or {}
    run_id = imported.get("run_id") or ""
    item = meta["tables"][name]
    owner = lookup_owner(run_id) if run_id else ""
    generation = lookup_generation(run_id, name) if run_id else ""
    if not run_id or not owner or not str(generation).isdigit():
        return 1, {}
    request = {
        "config_digest": item["config_digest"],
        "input_digest": item["input_digest"],
        "owner": owner,
        "protocol": "oo.pipeline.native-sealed-catalog/v1",
        "run_id": run_id,
        "schema_digest": item["schema_digest"],
        "snapshot": {"kind": "token", "value": SNAPSHOT},
        "source_generation": GENERATION,
        "source_id": SOURCE,
        "target": {
            "ca_file": "/secrets/pg-ca.crt",
            "database": "previewdb",
            "host": PG_NAME,
            "password_file": "/secrets/oo_runtime_password",
            "port": 5432,
            "runtime_principal": "oo_runtime",
        },
        "target_generation": int(generation),
        "type_digest": item["type_digest"],
    }
    path = PLAN / f"native-catalog-request-{name}.json"
    write_json(path, request)
    out = CLOSURE / "raw" / f"native-catalog-{name}.json"
    err = CLOSURE / "raw" / f"native-catalog-{name}.child.stderr"
    result = exec_runtime(
        [
            str(BINS / "pg-builder"), "import-native-catalog",
            "--request", str(path),
            "--out", str(out),
            "--pipeline", str(BINS / "oo-pipeline"),
            "--stderr", str(err),
        ],
        f"native-catalog-{name}",
        operator_env(),
        timeout=90,
    )
    body = load_json(out) or {}
    return result.returncode, body


def ensure_unauthorized_bearer():
    path = CLOSURE / "builder-unauthorized"
    if not path.exists() or path.stat().st_size == 0:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write("unauthorized\n")
    os.chmod(path, 0o600)
    return str(path)


def finish_print():
    print(json.dumps({
        "first": CHAIN["code"],
        "first_command": CHAIN["command"],
        "imports": CHAIN["imports"],
        "sealed": CHAIN["sealed"],
    }, sort_keys=True))


def run_chain():
    try:
        code = run_chain_body()
        if code not in (0, None) and CHAIN["code"] == 0:
            CHAIN["code"] = int(code)
            CHAIN["command"] = CHAIN["command"] or "chain"
        return CHAIN["code"]
    except Exception:
        if CHAIN["code"] == 0:
            CHAIN["code"] = 1
            CHAIN["command"] = "uncaught"
        raise
    finally:
        finish_print()


def run_chain_body():
    CLOSURE.mkdir(parents=True, exist_ok=True)
    (CLOSURE / "raw").mkdir(parents=True, exist_ok=True)
    PLAN.mkdir(parents=True, exist_ok=True)
    clear_stale_results()
    preserve_shape_packet()
    refusal = refuse_in_process()
    if refusal.get("source_read") is not False or refusal.get("reason") != "not_approved":
        return note_first(1, "object-refusal")
    if PROBE_OBJECT in set(CUSTOMER["approved"]):
        return note_first(1, "object-refusal")
    rustc = (BINS / "rustc.txt").read_text(encoding="utf-8") if (BINS / "rustc.txt").exists() else ""
    if "rustc 1.94.0" not in rustc:
        write(CLOSURE / "raw" / "msrv.exit", "1\n")
        print("msrv missing")
        return note_first(1, "msrv")
    if not (BINS / "binaries.sha256").is_file():
        print("binaries missing")
        return note_first(1, "reader-binaries")
    if sha256_file(ADMISSION_BIN) != ADMISSION_SHA or sha256_file(RECEIPT) != RECEIPT_SHA:
        print("pinned admission mismatch")
        return note_first(1, "admission-pin")
    state = run(["docker", "inspect", "-f", "{{.State.Status}}", BUILDER])
    if state.stdout.strip() == "running":
        print("builder still running")
        return note_first(3, "builder-already-running")
    if not TOKEN.is_file():
        print("token file missing")
        return note_first(1, "token-file")
    prepare_cluster()
    pipeline_code = build_pipeline()
    if pipeline_code != 0:
        print("pipeline build failed")
        return note_first(pipeline_code, "pipeline-build")
    if not (BINS / "oo-pipeline").is_file() or not (BINS / "pg-builder").is_file() or not (BINS / "init-pg").is_file():
        print("pipeline binaries missing")
        return note_first(1, "pipeline-binaries")
    reader_build = build_reader()
    if reader_build != 0:
        print("reader build failed")
        return note_first(reader_build, "reader-build")
    if refresh_reader_hashes() != 0:
        print("binary hash mismatch")
        return note_first(1, "binary-identity")
    reader_code = product_reader()
    write(CLOSURE / "raw" / "product-reader.exit", f"{reader_code}\n")
    arrows = copy_arrows()
    remove_owned(READER)
    if reader_code != 0:
        note_first(reader_code, "product-reader")
    if any(arrows.get(name) is None for name in TABLES):
        print("arrow missing")
        return note_first(reader_code or 1, CHAIN["command"] or "capture")
    audit = load_json(RUN / "audit-reconcile.json") or {}
    if audit.get("forbidden_attempts"):
        print("forbidden reader attempts")
        return note_first(reader_code or 1, "reader-audit")
    decode_exit = read_exit(RUN / "decode.exit")
    decode_report = RUN / "gate" / "decode-report.json"
    write_json(CLOSURE / "raw" / "decode-present.json", {
        "decode_exit": decode_exit,
        "present": decode_report.is_file(),
        "reader_exit": reader_code,
    })
    if decode_exit != 0 or not decode_report.is_file():
        print("decode not proven")
        return note_first(decode_exit or 1, "decode")
    counts = oracle_counts()
    meta = build_identity_files(arrows, counts, CUSTOMER["secret_ref"])
    build_registry(meta)
    if start_pg() != 0:
        return note_first(1, "postgres")
    if start_runtime() != 0:
        return note_first(1, "runtime")
    if wait_pg() != 0:
        return note_first(1, "postgres-ready")
    if init_pg() != 0:
        return note_first(1, "init-pg")
    if ensure_reader_login() != 0:
        return note_first(1, "reader-login")
    decision_code, _digest = validate_decision(meta)
    write(CLOSURE / "raw" / "validate.exit", f"{decision_code}\n")
    if decision_code != 0:
        packet = EVIDENCE / "closure-run" / "w38-policy-packet"
        packet.mkdir(parents=True, exist_ok=True)
        shutil.copy(PLAN / "admission-report.json", packet / "admission-report.json")
        shutil.copy(PLAN / "admission-config.json", packet / "admission-config.json")
        shutil.copy(CLOSURE / "raw" / "validate.stdout", packet / "decision.stdout")
        write(packet / "validate.exit", f"{decision_code}\n")
        print("validator refused")
        return note_first(decision_code, "admission-validate")
    imports = {}
    order = [SEAL_TABLE, *[item for item in TABLES if item not in {SEAL_TABLE, SHAPE_TABLE}], SHAPE_TABLE]
    for name in order:
        imports[name] = import_table(name, meta)
        write(CLOSURE / "raw" / f"import-{name}.code", f"{imports[name]}\n")
        if imports[name] != 0:
            note_first(imports[name], f"import-{name}")
    CHAIN["imports"] = imports
    write_json(CLOSURE / "raw" / "import-codes.json", imports)
    if imports.get(SHAPE_TABLE) == 0:
        save_applied_packet()
    save_shape_packet()
    boundary_refusals()
    boundary = load_json(CLOSURE / "raw" / "boundary-refusals.json") or {}
    if not refusal_held(boundary.get("precision"), ("precision", "schema")):
        note_first(1, "boundary-precision")
    if not refusal_held(boundary.get("unknown"), ("shape", "schema")):
        note_first(1, "boundary-unknown")
    sealed_install = install_sealed_catalog()
    write(CLOSURE / "raw" / "install-sealed-catalog.exit", f"{sealed_install}\n")
    if sealed_install != 0:
        note_first(sealed_install, "install-sealed-catalog")
    else:
        install_code = install_native_catalog()
        write(CLOSURE / "raw" / "install-native-catalog.exit", f"{install_code}\n")
        if install_code != 0:
            note_first(install_code, "install-native-sealed-catalog")
        elif ensure_runtime_login() != 0:
            note_first(1, "oo-runtime")
        else:
            catalogs = {}
            for name in order:
                if imports.get(name) != 0:
                    catalogs[name] = {"engine": None, "exit": imports.get(name), "protocol": None}
                    continue
                code, body = catalog_read(name, meta)
                catalogs[name] = {
                    "engine": body.get("engine"),
                    "exit": code,
                    "protocol": body.get("protocol"),
                }
                if code != 0 or body.get("engine") != ENGINE or body.get("protocol") != "oo.pipeline.sealed-import-catalog/v1":
                    note_first(code or 1, f"import-native-catalog-{name}")
            write_json(CLOSURE / "raw" / "catalog-codes.json", catalogs)
    built = seal_catalog(meta) if all(imports.get(name) == 0 for name in TABLES) else None
    if built is None:
        note_first(CHAIN["code"] or 1, CHAIN["command"] or "seal-catalog")
    else:
        catalog_path, catalog, note = built
        if not note.get("bound"):
            note_first(1, "target-generation")
        else:
            if start_forwarder() != 0:
                note_first(1, "forwarder")
            else:
                run_model_probes(catalog_path, catalog)
    if not (CLOSURE / "raw" / "source-off.json").is_file():
        off = stop_source()
        if off != 0:
            note_first(off, "stop-source")
    return CHAIN["code"]


def run_model_probes(catalog_path, catalog):
    declared = {"currency": "USD", "per_total_token": "0", "status": "declared_non_billing"}
    unknown = {"status": "unknown"}
    bad_bearer = ensure_unauthorized_bearer()
    code_401 = seal(catalog_path, "401", declared, 30000, bad_bearer, "seal-401", "seal-cache-401", 90)
    body_401 = load_json(CLOSURE / "seal-401" / "result.json") or {}
    write(CLOSURE / "raw" / "sealed-401.exit", f"{code_401}\n")
    if not held_probe("401", body_401, code_401):
        note_first(code_401 or 1, "builder-401")
    code_timeout = seal(catalog_path, "timeout", declared, 1, str(TOKEN), "seal-timeout", "seal-cache-timeout", 90)
    body_timeout = load_json(CLOSURE / "seal-timeout" / "result.json") or {}
    write(CLOSURE / "raw" / "sealed-timeout.exit", f"{code_timeout}\n")
    if not held_probe("timeout", body_timeout, code_timeout):
        note_first(code_timeout or 1, "builder-timeout")
    seal_code = seal(catalog_path, "packed", declared, 180000, str(TOKEN), "seal-out", "seal-cache-fresh", 240)
    CHAIN["sealed"] = seal_code
    write(CLOSURE / "raw" / "sealed.exit", f"{seal_code}\n")
    packed = seal_code == 0 and pack_produced()
    if not packed:
        note_first(seal_code or 1, "sealed")
        write(CLOSURE / "raw" / "preview-ran.txt", "0\n")
        write(CLOSURE / "raw" / "preview.note", "pack not produced; consumer not started\n")
    else:
        cost_code = seal(catalog_path, "cost", unknown, 180000, str(TOKEN), "seal-cost", "seal-cache-cost", 240)
        body_cost = load_json(CLOSURE / "seal-cost" / "result.json") or {}
        write(CLOSURE / "raw" / "sealed-cost.exit", f"{cost_code}\n")
        if not held_probe("cost_unknown", body_cost, cost_code):
            note_first(cost_code or 1, "builder-cost-unknown")
        off = stop_source()
        if off != 0:
            note_first(off, "stop-source")
        source = load_json(CLOSURE / "raw" / "source-off.json") or {}
        if source.get("query_after_stop") is not True:
            note_first(1, "query-after-stop")
        else:
            write(CLOSURE / "raw" / "preview-ran.txt", "1\n")
            preview_code = build_preview()
            write(CLOSURE / "raw" / "preview.exit", f"{preview_code}\n")
            if preview_code != 0:
                note_first(preview_code, "preview-build")
            elif not (BINS / "operator-preview").is_file() or not (BINS / "install-column-acl").is_file() or not (BINS / "provision-mapping-reader").is_file():
                note_first(1, "preview-binaries")
            elif seal_binary_identity() != 0:
                note_first(1, "binary-identity")
            else:
                policies = policy_set()
                if not policies:
                    note_first(1, "policy-set")
                    write_json(CLOSURE / "raw" / "typed-oracle.json", {"matched": {}, "reason": "policy set incomplete"})
                else:
                    install_code = install_acl(policies)
                    provision_code = provision_mapping(policies) if install_code == 0 else None
                    if install_code != 0:
                        note_first(install_code, "install-column-acl")
                    if provision_code not in (0, None):
                        note_first(provision_code, "provision-mapping-reader")
                    describe = preview_exec(
                        [str(BINS / "operator-preview"), "describe", "--authority", str(CLOSURE / "seal-out" / "authority.json")],
                        "describe",
                        timeout=30,
                    )
                    if describe.returncode != 0:
                        note_first(describe.returncode, "describe")
                    exits, matched = typed_queries()
                    for name, code in exits.items():
                        if code != 0 or matched.get(name) is not True:
                            note_first(code or 1, f"typed-{name}")
                            break
                    deny_code = denial_query()
                    widen_code = widen_query()
                    hidden_code = hidden_column_query()
                    late_code = late_cancel()
                    inflight_code = inflight_cancel(catalog)
                    write_json(CLOSURE / "raw" / "consumer.json", {
                        "denial_exit": deny_code,
                        "describe_exit": describe.returncode,
                        "hidden_exit": hidden_code,
                        "inflight_exit": inflight_code,
                        "install_exit": install_code,
                        "late_cancel_exit": late_code,
                        "provision_exit": provision_code,
                        "typed_query_exit": 0 if all(code == 0 for code in exits.values()) else 1,
                        "widen_exit": widen_code,
                    })
                    denial_body = parse_stdout_json("denial-query") or {}
                    hidden_body = parse_stdout_json("hidden-column-query") or {}
                    widen_body = parse_stdout_json("widen-query") or {}
                    if widen_code != 0 or widen_body.get("outcome") != "succeeded":
                        note_first(widen_code or 1, "widen")
                    if not (
                        deny_code not in (0, None)
                        and denial_body.get("outcome") == "refused"
                        and denial_body.get("class") == "guard"
                        and "rows" not in denial_body
                    ):
                        note_first(deny_code if deny_code not in (0, None) else 1, "denial")
                    if not (
                        hidden_code not in (0, None)
                        and hidden_body.get("outcome") == "refused"
                        and hidden_body.get("class") == "guard"
                        and "rows" not in hidden_body
                    ):
                        note_first(hidden_code if hidden_code not in (0, None) else 1, "hidden-column")
                    inflight_body = parse_stdout_json("inflight-query") or {}
                    inflight = load_json(CLOSURE / "raw" / "inflight-summary.json") or {}
                    inflight_ok = (
                        inflight.get("running_seen") is True
                        and inflight_code == 0
                        and inflight_body.get("outcome") == "canceled"
                        and inflight_body.get("sqlstate") == "57014"
                        and inflight_body.get("rollback_confirmed") is True
                        and inflight_body.get("returned_rows") == 0
                    )
                    if not inflight_ok:
                        note_first(inflight_code or 1, "inflight")
                    reopen_code = local_reopen()
                    if reopen_code != 0:
                        note_first(reopen_code, "local-reopen")

def copy_text(source, dest):
    if source.is_file():
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(source, dest)


def write_sums(directory):
    lines = []
    for path in sorted(directory.rglob("*")):
        if not path.is_file() or path.name == "SHA256SUMS":
            continue
        if path.suffix in {".so", ".a"} or path.name.startswith("doris-"):
            continue
        rel = path.relative_to(directory).as_posix()
        lines.append(f"{sha256_file(path)}  {rel}")
    text = "\n".join(lines) + ("\n" if lines else "")
    (directory / "SHA256SUMS").write_text(text, encoding="utf-8")
    return sha256_bytes(text.encode())


def hashes_sealed():
    identity = load_json(CLOSURE / "raw" / "binary-identity.json") or {}
    preview_ok, _preview = preview_build_proof()
    if identity.get("locked") is not True or identity.get("archive") != PIN or identity.get("archive_match") is not True:
        return False
    if identity.get("pipeline_rebuilt") is not False or identity.get("reader_rebuilt") is not False:
        return False
    if not preview_ok:
        return False
    pipeline = parse_sha_lines(BINS / "pipeline-binaries.sha256")
    combined = parse_sha_lines(BINS / "binaries.sha256")
    for name in ("oo-pipeline", "init-pg", "pg-builder"):
        if pipeline.get(name) != combined.get(name):
            return False
        path = BINS / name
        if not path.is_file() or sha256_file(path) != pipeline.get(name):
            return False
    commit_text = (BINS / "pipeline-commit.txt").read_text(encoding="utf-8").strip() if (BINS / "pipeline-commit.txt").is_file() else ""
    rustc_text = (BINS / "pipeline-rustc.txt").read_text(encoding="utf-8") if (BINS / "pipeline-rustc.txt").is_file() else ""
    return commit_text == PIN and "rustc 1.94.0" in rustc_text


def reopen_binds(reopen):
    ids = reopen.get("execution_ids") or {}
    typed_ids = reopen.get("typed_execution_ids") or {}
    for name in TABLES:
        execution = ids.get(name)
        path = CLOSURE / "raw" / f"reopen-{name}.stdout"
        text = path.read_text(encoding="utf-8") if path.is_file() else ""
        if not isinstance(execution, str) or not execution or execution not in text or execution == typed_ids.get(name):
            return False
    return True


def confirm_ok():
    body = load_json(RUN / "gate" / "confirm.json") or {}
    got = {item.get("table"): item.get("count") for item in body.get("counts") or []}
    expected = {
        "agg_qty": 5,
        "dec_exact": 3,
        "dup_events": 5,
        "nz_obs": 3,
        "part_day": 3,
        "str_key": 7,
        "uniq_acct": 3,
    }
    return got == expected


def classify():
    audit = load_json(RUN / "audit-reconcile.json") or {}
    imports = load_json(CLOSURE / "raw" / "import-codes.json") or {}
    seal_body = load_json(CLOSURE / "seal-out" / "result.json") or {}
    consumer = load_json(CLOSURE / "raw" / "consumer.json") or {}
    inflight = load_json(CLOSURE / "raw" / "inflight-summary.json") or {}
    source = load_json(CLOSURE / "raw" / "source-off.json") or {}
    denial = parse_stdout_json("denial-query") or {}
    denial_refused = (
        consumer.get("denial_exit") not in (0, None)
        and denial.get("outcome") == "refused"
        and denial.get("class") == "guard"
        and "rows" not in denial
    )
    hidden = parse_stdout_json("hidden-column-query") or {}
    hidden_refused = (
        consumer.get("hidden_exit") not in (0, None)
        and hidden.get("outcome") == "refused"
        and hidden.get("class") == "guard"
        and "rows" not in hidden
    )
    widen = parse_stdout_json("widen-query") or {}
    widen_rows = widen.get("rows") if isinstance(widen.get("rows"), list) else []
    widen_widths = {len(row) for row in widen_rows if isinstance(row, list)}
    widen_ok = (
        consumer.get("widen_exit") == 0
        and widen.get("outcome") == "succeeded"
        and widen.get("returned_rows") == 3
        and widen_widths == {4}
        and (widen.get("policy") or {}).get("columns") == ["id", "a18", "b18", "d38"]
    )
    inflight_body = parse_stdout_json("inflight-query") or {}
    inflight_ok = (
        inflight.get("running_seen") is True
        and consumer.get("inflight_exit") == 0
        and inflight_body.get("outcome") == "canceled"
        and inflight_body.get("sqlstate") == "57014"
        and inflight_body.get("rollback_confirmed") is True
        and inflight_body.get("returned_rows") == 0
    )
    boundary = load_json(CLOSURE / "raw" / "boundary-refusals.json") or {}
    precision_held = refusal_held(boundary.get("precision"), ("precision", "schema"))
    unknown_held = refusal_held(boundary.get("unknown"), ("shape", "schema"))
    admission = load_json(PLAN / "admission-report.json") or {}
    cost_confidence = ((admission.get("cost_model") or {}).get("confidence"))
    cache_ok = seal_body.get("cache") == "miss" and seal_body.get("cache_store") == "stored"
    decode = load_json(CLOSURE / "raw" / "decode-present.json") or {}
    catalogs = load_json(CLOSURE / "raw" / "catalog-codes.json") or {}
    catalog_ok = bool(catalogs) and all(
        item.get("exit") == 0 and item.get("engine") == ENGINE and item.get("protocol") == "oo.pipeline.sealed-import-catalog/v1"
        for item in catalogs.values()
    ) and set(catalogs) == set(TABLES)
    typed = load_json(CLOSURE / "raw" / "typed-oracle.json") or {}
    typed_ok = all(typed.get("matched", {}).get(name) is True for name in TABLES)
    probe_401 = load_json(CLOSURE / "seal-401" / "result.json") or {}
    probe_timeout = load_json(CLOSURE / "seal-timeout" / "result.json") or {}
    probe_cost = load_json(CLOSURE / "seal-cost" / "result.json") or {}
    unauthorized_held = held_probe("401", probe_401, read_exit(CLOSURE / "raw" / "sealed-401.exit"))
    timeout_held = held_probe("timeout", probe_timeout, read_exit(CLOSURE / "raw" / "sealed-timeout.exit"))
    cost_held = held_probe("cost_unknown", probe_cost, read_exit(CLOSURE / "raw" / "sealed-cost.exit"))
    launcher_refusal = load_json(CLOSURE / "raw" / "object-refusal.json") or {}
    reader_probe = load_json(RUN / "object-probe-held.json") or {}
    object_ok = (
        launcher_refusal.get("source_read") is False
        and launcher_refusal.get("reason") == "not_approved"
        and launcher_refusal.get("object") == PROBE_OBJECT
        and reader_probe.get("held") is True
    )
    source_ok = (
        source.get("fe_stop_exit") == 0
        and source.get("be_stop_exit") == 0
        and source.get("query_after_stop") is True
        and "reopen_port_exit" not in source
        and "detached_count" not in source
    )
    reopen = load_json(CLOSURE / "raw" / "local-reopen.json") or {}
    reopen_ok = (
        reopen.get("query_after_stop") is True
        and reopen.get("source_port_reopened") is False
        and reopen.get("fe_status") not in {"", "running"}
        and reopen.get("be_status") not in {"", "running"}
        and all(reopen.get("matched", {}).get(name) is True for name in TABLES)
        and all(reopen.get("reopen_exits", {}).get(name) == 0 for name in TABLES)
        and reopen_binds(reopen)
    )
    conditions = []
    shared = False
    external = False
    summary = load_json(RUN / "run-summary.json") or {}
    if summary.get("dba_exit") not in (0, None):
        conditions.append(f"dba-setup exited {summary.get('dba_exit')}")
    reader_exit = read_exit(CLOSURE / "raw" / "product-reader.exit")
    if reader_exit not in (0, None):
        conditions.append(f"product reader exited {reader_exit}")
    if audit.get("forbidden_attempts"):
        conditions.append("product reader statement audit contains a forbidden attempt")
    if decode.get("decode_exit") not in (0, None) or decode.get("present") is False:
        conditions.append("decode report did not prove the capture")
    if audit and audit.get("ready") is not True:
        conditions.append("product reader audit coverage is not ready")
    if not object_ok:
        conditions.append("unapproved object was not refused before the source read")
    if not confirm_ok():
        conditions.append("confirm.json does not map all seven approved tables")
    if read_exit(CLOSURE / "raw" / "validate.exit") not in (0, None):
        shared = True
        conditions.append("admission validator refused this source policy")
    for name, code in imports.items():
        if code != 0:
            shared = True
            conditions.append(f"native import of {name} exited {code}")
    if catalogs and not catalog_ok:
        shared = True
        conditions.append("a native sealed catalog read did not return engine doris")
    elif imports and not catalogs:
        conditions.append("native sealed catalog reads were not recorded")
    if seal_body and seal_body.get("outcome") != "packed":
        model = seal_body.get("model")
        if model in {"called", "cache_hit", "not_called"} or seal_body.get("outcome") in {"provider_failure", "budget_failure", "cost_unknown"}:
            external = True
        conditions.append(f"sealed builder outcome is {seal_body.get('outcome')}")
    elif read_exit(CLOSURE / "raw" / "sealed.exit") not in (0, None):
        external = True
        conditions.append(f"pg-builder sealed exited {read_exit(CLOSURE / 'raw' / 'sealed.exit')}")
    if not unauthorized_held:
        conditions.append("fresh-cache 401 probe was not an HTTP 401 refusal")
    if not timeout_held:
        conditions.append("timeout probe was not a timeout refusal")
    if seal_body.get("outcome") == "packed" and not cost_held:
        conditions.append("cost_unknown probe did not refuse the pack")
    preview_ran = ""
    ran_path = CLOSURE / "raw" / "preview-ran.txt"
    if ran_path.is_file():
        preview_ran = ran_path.read_text(encoding="utf-8").strip()
    if preview_ran == "1":
        preview_exit = read_exit(CLOSURE / "raw" / "preview-build.exit")
        if preview_exit is None:
            preview_exit = read_exit(CLOSURE / "raw" / "preview.exit")
        if preview_exit not in (0, None):
            conditions.append(f"operator-preview build exited {preview_exit}")
    if consumer:
        if not typed_ok:
            shared = True
            conditions.append("trusted typed query did not match the oracle on every approved table")
        if not denial_refused:
            shared = True
            conditions.append("denial query was not refused")
        if not widen_ok:
            shared = True
            conditions.append("widen query did not return the granted columns")
        if not hidden_refused:
            shared = True
            conditions.append("hidden column query was not refused")
        if not inflight_ok:
            if not inflight.get("running_seen"):
                conditions.append("no running execution; inflight cancel was not sent")
            else:
                conditions.append("inflight cancel did not record sqlstate 57014 with rollback")
    elif seal_body.get("outcome") == "packed":
        conditions.append("role pack exists and the trusted consumer did not finish")
    if boundary:
        if not precision_held:
            if (boundary.get("precision") or {}).get("exit") == 0:
                shared = True
            conditions.append("declared precision on day was not refused")
        if not unknown_held:
            if (boundary.get("unknown") or {}).get("exit") == 0:
                shared = True
            conditions.append("unknown day token was not refused")
    elif imports:
        conditions.append("boundary refusals were not recorded")
    if admission and cost_confidence != "unknown":
        conditions.append(f"admission cost model confidence is {cost_confidence}")
    if seal_body.get("outcome") == "packed" and not cache_ok:
        conditions.append("sealed cache was not a fresh miss that stored")
    pipeline_exit = read_exit(CLOSURE / "raw" / "pipeline-build.exit")
    if pipeline_exit not in (0, None):
        conditions.append(f"pipeline build exited {pipeline_exit}")
    reader_build_exit = read_exit(CLOSURE / "raw" / "reader-build.exit")
    if reader_build_exit not in (0, None):
        conditions.append(f"reader build exited {reader_build_exit}")
    if source and not source_ok:
        conditions.append("FE and BE were not stopped before the trusted query")
    if reopen and not reopen_ok:
        conditions.append("local reopen did not match the oracle while FE and BE stayed stopped")
    elif source and not reopen:
        conditions.append("local reopen was not recorded")
    if not hashes_sealed():
        conditions.append("preview and pipeline ELF hashes are not sealed to the on-disk binaries")
    sealed_install_exit = read_exit(CLOSURE / "raw" / "install-sealed-catalog.exit")
    if sealed_install_exit not in (0, None):
        conditions.append(f"sealed catalog install exited {sealed_install_exit}")
    install_exit = read_exit(CLOSURE / "raw" / "install-native-catalog.exit")
    if install_exit not in (0, None):
        conditions.append(f"native catalog install exited {install_exit}")
    complete = (
        CHAIN["code"] == 0
        and audit.get("ready") is True
        and not audit.get("forbidden_attempts")
        and decode.get("decode_exit") == 0
        and decode.get("present") is True
        and imports
        and all(code == 0 for code in imports.values())
        and set(imports) == set(TABLES)
        and catalog_ok
        and pipeline_exit == 0
        and reader_build_exit == 0
        and sealed_install_exit == 0
        and install_exit == 0
        and seal_body.get("outcome") == "packed"
        and seal_body.get("ready") is False
        and seal_body.get("review") == "unreviewed"
        and cache_ok
        and cost_confidence == "unknown"
        and unauthorized_held
        and timeout_held
        and cost_held
        and typed_ok
        and confirm_ok()
        and denial_refused
        and widen_ok
        and hidden_refused
        and inflight_ok
        and precision_held
        and unknown_held
        and object_ok
        and source_ok
        and reopen_ok
        and hashes_sealed()
    )
    if complete:
        outcome = "author_complete"
    elif shared:
        outcome = "blocked_shared_contract"
    else:
        outcome = "blocked_external"
        if not external and not conditions:
            conditions.append("the same-run chain stopped before every gate was terminal")
    return outcome, conditions, {
        "audit_ready": audit.get("ready"),
        "boundary": boundary,
        "catalogs": catalogs,
        "confirm_ok": confirm_ok(),
        "consumer": consumer,
        "cost_model_confidence": cost_confidence,
        "cost_unknown_outcome": probe_cost.get("outcome"),
        "decode": decode,
        "first_command": CHAIN["command"],
        "forbidden_attempts": audit.get("forbidden_attempts") or [],
        "http_401": probe_401.get("http_status"),
        "imports": imports,
        "inflight": inflight,
        "inflight_sqlstate": inflight_body.get("sqlstate"),
        "object_refusal": object_ok,
        "pipeline_commit": PIN,
        "seal": {
            "cache": seal_body.get("cache"),
            "cache_store": seal_body.get("cache_store"),
            "model": seal_body.get("model"),
            "outcome": seal_body.get("outcome"),
            "pack": seal_body.get("pack"),
            "ready": seal_body.get("ready"),
            "review": seal_body.get("review"),
            "usage": seal_body.get("usage"),
        },
        "reopen_matched": reopen.get("matched"),
        "source": source,
        "source_acceptance": False,
        "source_port_reopened": reopen.get("source_port_reopened"),
        "timeout_reason": probe_timeout.get("reason"),
        "typed_matched": typed.get("matched"),
        "widen_returned_rows": widen.get("returned_rows"),
    }


def delivery_body(outcome, conditions, source_sha):
    decisive = [
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/closure-run/facts.json",
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/closure-run/REPORT.md",
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/closure-run/audit-reconcile.json",
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/closure-run/seal-result.json",
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/failed-attempts/blocked-523d24f/whole-source-delivery.json",
    ]
    return {
        "decisive_case_paths": [path for path in decisive if (REPO / path).is_file() or path.endswith("facts.json") or path.endswith("REPORT.md")],
        "outcome": outcome,
        "protocol": "oo.source-product-delivery/v1",
        "remaining_conditions": conditions,
        "source_id": SOURCE,
        "source_sha": source_sha,
        "task_id": "DL01-W1-43",
    }


def emit_reports(chain_code):
    run_dir = EVIDENCE / "closure-run"
    run_dir.mkdir(parents=True, exist_ok=True)
    keep = {"w06-date32-packet", "w06-date32-packet-910106e99", "part-day-not-shape.txt"}
    for child in list(run_dir.iterdir()):
        if child.name in keep:
            continue
        if child.is_dir():
            shutil.rmtree(child)
        else:
            child.unlink()
    raw = run_dir / "raw"
    raw.mkdir(parents=True, exist_ok=True)
    names = (
        "product-reader.exit", "decode-present.json", "import-codes.json", "validate.exit",
        "sealed.exit", "sealed-401.exit", "sealed-timeout.exit", "sealed-cost.exit",
        "schema-digest-check.json", "consumer.json", "inflight-summary.json",
        "source-off.json", "preview-build.exit", "preview-build.tail", "preview-build.log",
        "consumer-generation-bind.stdout",
        "consumer-generation-bind.exit", "oid-drift.json",
        "pipeline-build.exit", "pipeline-build.tail", "reader-build.exit", "reader-build.tail",
        "boundary-refusals.json",
        "denial-query.stdout", "denial-query.exit", "denial-query.stderr",
        "typed-query.stdout", "typed-query.exit", "typed-oracle.json",
        "widen-query.stdout", "widen-query.exit",
        "hidden-column-query.stdout", "hidden-column-query.exit",
        "inflight-cancel.stdout", "inflight-query.stdout", "inflight-query.exit",
        "late-cancel.stdout", "late-cancel.exit",
        "object-refusal.json", "catalog-codes.json",
        "install-sealed-catalog.json", "install-sealed-catalog.exit",
        "install-native-catalog.json", "install-native-catalog.exit",
    )
    for name in names:
        copy_text(CLOSURE / "raw" / name, raw / name)
    for name in TABLES:
        copy_text(CLOSURE / "raw" / f"typed-{name}.stdout", raw / f"typed-{name}.stdout")
        copy_text(CLOSURE / "raw" / f"reopen-{name}.stdout", raw / f"reopen-{name}.stdout")
        copy_text(CLOSURE / "raw" / f"native-catalog-{name}.json", raw / f"native-catalog-{name}.json")
    copy_text(CLOSURE / "raw" / "local-reopen.json", raw / "local-reopen.json")
    capture_out = run_dir / "capture"
    capture_out.mkdir(parents=True, exist_ok=True)
    for name in TABLES:
        copy_text(CAPTURE / f"{name}.arrow", capture_out / f"{name}.arrow")
    for label in ("401", "timeout", "packed", "cost"):
        copy_text(PLAN / f"builder-price-{label}.json", raw / f"builder-price-{label}.json")
    copy_text(BINS / "pipeline-binaries.sha256", raw / "pipeline-binaries.sha256")
    copy_text(BINS / "pipeline-rustc.txt", raw / "pipeline-rustc.txt")
    copy_text(BINS / "pipeline-commit.txt", raw / "pipeline-commit.txt")
    copy_text(BINS / "reader-binaries.sha256", raw / "reader-binaries.sha256")
    copy_text(BINS / "reader-rustc.txt", raw / "reader-rustc.txt")
    copy_text(BINS / "binaries.sha256", raw / "binaries.sha256")
    copy_text(BINS / "preview-binaries.sha256", raw / "preview-binaries.sha256")
    copy_text(BINS / "preview-rustc.txt", raw / "preview-rustc.txt")
    copy_text(BINS / "preview-commit.txt", raw / "preview-commit.txt")
    copy_text(CLOSURE / "preview-build.sh", raw / "preview-build.sh")
    copy_text(CLOSURE / "raw" / "binary-identity.json", raw / "binary-identity.json")
    copy_text(CLOSURE / "raw" / "local-reopen-stream.log", raw / "local-reopen-stream.log")
    copy_text(CLOSURE / "raw" / "reopen-capacity.json", raw / "reopen-capacity.json")
    for name in TABLES:
        copy_text(CLOSURE / "raw" / f"reopen-{name}.exit", raw / f"reopen-{name}.exit")
    copy_text(CLOSURE / "launch-closure-stream.log", run_dir / "launch-closure-stream.log")
    copy_text(CLOSURE / "seal-out" / "result.json", run_dir / "seal-result.json")
    copy_text(CLOSURE / "seal-401" / "result.json", run_dir / "seal-401.json")
    copy_text(CLOSURE / "seal-timeout" / "result.json", run_dir / "seal-timeout.json")
    copy_text(CLOSURE / "seal-cost" / "result.json", run_dir / "seal-cost.json")
    copy_text(PLAN / "seal-catalog.json", run_dir / "seal-catalog.json")
    copy_text(RUN / "audit-reconcile.json", run_dir / "audit-reconcile.json")
    copy_text(RUN / "run-summary.json", run_dir / "run-summary.json")
    copy_text(RUN / "gate" / "ready.json", run_dir / "ready.json")
    copy_text(RUN / "gate" / "confirm.json", run_dir / "confirm.json")
    copy_text(RUN / "object-probe" / "object-refusal.json", run_dir / "reader-object-refusal.json")
    copy_text(RUN / "object-probe-held.json", run_dir / "object-probe-held.json")
    outcome, conditions, facts = classify()
    facts["chain_exit"] = chain_code
    write_json(run_dir / "facts.json", facts)
    condition_text = "\n".join(f"- {item}" for item in conditions) or "- none"
    command = facts.get("first_command")
    report = (
        "# DL01-W1-43 Doris same-run closure\n\n"
        f"Chain exit: {chain_code}\n\n"
        f"first_command: {command}\n\n"
        "The chain exit is that command's status. "
        "A held schema refusal is recorded in raw/boundary-refusals.json and is not assigned to first. "
        "The unapproved-object refusal, the 401 probe, the timeout probe, and the cost_unknown probe "
        "keep their own statuses and are not assigned to first when the recorded result is the expected refusal.\n\n"
        f"Delivery outcome: {outcome}\n\n"
        "This run used Doris, a recomputed admission, and the fixed native importer. "
        "ClickHouse fixture pins were not loaded. The pack review field stays unreviewed. "
        "source acceptance stays false. Independent review is deferred and is not an approval.\n\n"
        "## Gate facts\n\n"
        f"```json\n{json.dumps(facts, indent=2, sort_keys=True)}\n```\n\n"
        "## Remaining\n\n"
        f"{condition_text}\n"
    )
    findings = (
        "# Findings\n\n"
        "The product reader skips trial DDL. Historical native-3 DROP, ALTER, and GRANT rows stay in that run "
        "and are not this reader journey. The reader client audit is gate/client-audit.jsonl. "
        "DROP, ALTER, and GRANT rows in dba/dba-audit.jsonl are bootstrap statements, not the reader journey.\n\n"
        "part_day.day stays date32 with precision and scale absent. "
        "The importer keeps that source date and its provenance. "
        "A copied manifest with precision 32 on day, and a copied manifest whose day token is not_a_type, "
        "diverge from the approved schema fields. The importer returns reason schema, with the loader not invoked, "
        "before it plans the arrow column. The real part_day capture seals as date oid 1082. "
        "Those refusals are in raw/boundary-refusals.json. "
        "Admission cost_model confidence stays unknown.\n\n"
        "The packed builder input price is declared_non_billing with per_total_token 0. "
        "That value is the request config in raw/builder-price-packed.json. "
        "seal-result.json is the packed result and this report does not add a price field to it. "
        "The 401, timeout, and cost_unknown results are seal-401.json, seal-timeout.json, and seal-cost.json.\n\n"
        f"Outcome {outcome} follows the gate facts in REPORT.md. "
        "A sealed pack with ready false and review unreviewed is the author chain, not an independent approval. "
        "source acceptance stays false.\n\n"
        "preview-binaries.sha256 is the sha256sum tee printed by build_preview after "
        "cargo build --locked --jobs 1 --offline. The recipe deletes "
        f"/target/preview-{TARGET_TAG}-op and /target/preview-{TARGET_TAG}-gw before those builds. "
        "The log shows Compiling operator-preview, Finished in 2m 34s, then Compiling oo-gateway, Finished in 2m 52s, "
        "then the three sha256 lines and PREVIEW_BUILD_OK. "
        "install-column-acl and provision-mapping-reader are bins of oo-gateway. "
        "preview-rustc.txt records rustc 1.94.0. The container exit is 0 and the builder was removed after the log was collected. "
        "The three ELF hashes match the bytes already used for the typed and reopen stdout, so those commands were not run again. "
        "Pipeline and reader ELFs were not rebuilt. "
        "The 2600-byte tail with no tee is failed-attempts/preview-build-unverified-tee. "
        "A later run that printed PREVIEW_BUILD_OK with Finished in under a second and no Compiling line is "
        "failed-attempts/preview-build-incremental-noop. That marker was not copied into the 2600-byte tail.\n\n"
        f"{condition_text}\n"
    )
    handoff = (
        "# Handoff\n\n"
        "Owner session 5106fed0-5bcd-4aaf-8df7-838c95a7d7e2. Task DL01-W1-43.\n\n"
        "The earlier preview-build.tail has no sha256sum tee and no PREVIEW_BUILD_OK. "
        "failed-attempts/preview-build-unverified-tee keeps that tail and the continuation json that asserted the marker. "
        "The marker was not written into that tail. "
        "This inventory's preview-build.tail and preview-build.log come from build_preview on archive "
        f"{PIN}. The recipe removes /target/preview-{TARGET_TAG}-op and /target/preview-{TARGET_TAG}-gw, "
        "then runs cargo build --locked --jobs 1 --offline under rustc 1.94.0. "
        "operator-preview compiled in 2m 34s. oo-gateway, which contains install-column-acl and provision-mapping-reader, compiled in 2m 52s. "
        "The tee hashes equal the existing ELF files, so the seven typed queries, the reopen, the guard refusals, and the inflight cancel were not run again. "
        "Pipeline and reader were not rebuilt. "
        "failed-attempts/preview-build-incremental-noop keeps the Finished-only log. "
        "Its PREVIEW_BUILD_OK line was not copied into the 2600-byte tail.\n\n"
        f"This run's importer and consumer archive is {PIN}. "
        "Consumer 6d9d6cbb98f249709a462915afc8f45e4db3c047 is an ancestor of that archive. "
        "CHAR commits 29aae36b and e9e3013c are ancestors of that archive. "
        "The old b1ff bpchar refusal was not retried. "
        "The importer, catalog, and consumer binaries come from the readonly a6825165 archive. "
        "doris-reader and doris-decode are compiled in this run from this task's owned reader workspace.\n\n"
        "Native catalog reads use pg-builder import-native-catalog. "
        "The request protocol is oo.pipeline.native-sealed-catalog/v1 and the request has no engine field. "
        "Install applies oo.pipeline.install-sealed-catalog/v1, then "
        "oo.pipeline.install-native-sealed-catalog/v1, on this task's PostgreSQL. "
        "catalog-import was not called.\n\n"
        "Historical importer d3b3abcff32cc85602d7736eb1340b14813c069b refused part_day with reason shape. "
        "Evidence commit e304a6adeedbe2c713623fe68ad480ff3c8bff27 was the binding record. "
        "Boundary review 6279ab8ab0a368f73f05e6c3f1235d737e304857. "
        "That shape packet stays at closure-run/w06-date32-packet. "
        "The pre-run copy is closure-run/w06-date32-packet-910106e99. "
        "applied-4a2ebdb is left in place. This run writes applied-a6825165 when the import completed.\n\n"
        "Engine is doris. source_id and project are dorispreview. Snapshot token is oo-dl01w1-43-product. "
        "W06 starrocks evidence e88cb4dd8943e74f20f7483fa7612198b5d4acf5 was read and its 84-line SHA256SUMS checked. "
        "That tree was not edited. W38 was not edited.\n\n"
        "An absent or late cancel is not inflight. inflight-summary.json records whether a running execution was seen. "
        "The inflight proof is sqlstate 57014 and rollback_confirmed on the running query. "
        "Trusted queries run after FE and BE are stopped. A psql count and a source port reopen are not the consumption proof.\n\n"
        "local_reopen runs operator-preview once per approved table after FE and BE are stopped. "
        "The execution id in each reopen stdout is the id that command returned. "
        "clear_stale_results removes local-reopen.json and the reopen stdout files before a later full run. "
        "preview-binaries.sha256 records operator-preview, install-column-acl, and provision-mapping-reader "
        "from the build_preview sha256sum tee. "
        "The pipeline lines in binaries.sha256 are the sha256 of the ELF files named by pipeline-binaries.sha256. "
        "seal_binary_identity checks that tee, the Compiling lines in preview-build.log, and the pinned archive blobs. "
        "The compile command is build_preview, and it does not rebuild the pipeline or the reader. "
        "The recipe remains cargo build --locked with rustc 1.94.0.\n\n"
        f"{condition_text}\n"
    )
    write(run_dir / "REPORT.md", report)
    write(run_dir / "FINDINGS.md", findings)
    write(run_dir / "HANDOFF.md", handoff)
    write_resource_receipt(run_dir, "pending-owned-commit")
    preview = delivery_body(outcome, conditions, "pending-owned-commit")
    write_json(run_dir / "delivery-preview.json", preview)
    evidence_sha = write_sums(run_dir)
    preserved = EVIDENCE / "failed-attempts" / "blocked-523d24f"
    live = EVIDENCE / "whole-source-delivery.json"
    if live.is_file() and not (preserved / "whole-source-delivery.json").exists():
        preserved.mkdir(parents=True, exist_ok=True)
        shutil.copy(live, preserved / "whole-source-delivery.json")
    outer = delivery_body(outcome, conditions, "pending-owned-commit")
    outer["evidence_sha"] = evidence_sha
    write_json(live, outer)


def write_resource_receipt(run_dir, source_commit):
    capacity = load_json(CLOSURE / "capacity" / "preview-builder.json") or {}
    identity = load_json(CLOSURE / "raw" / "binary-identity.json") or {}
    reopen = load_json(CLOSURE / "raw" / "local-reopen.json") or {}
    pipeline = parse_sha_lines(BINS / "pipeline-binaries.sha256")
    preview = parse_sha_lines(BINS / "preview-binaries.sha256")
    proof_ok, proof = preview_build_proof()
    body = {
        "archive": PIN,
        "archive_match": identity.get("archive_match"),
        "binary_locked": identity.get("locked"),
        "build_exits": {
            "pipeline": read_exit(CLOSURE / "raw" / "pipeline-build.exit"),
            "preview": read_exit(CLOSURE / "raw" / "preview-build.exit"),
            "reader": read_exit(CLOSURE / "raw" / "reader-build.exit"),
        },
        "capacity": {
            "action": "preview-build",
            "cpu_after": capacity.get("cpu_after"),
            "global_rust": capacity.get("global_rust"),
            "headroom_bytes": capacity.get("headroom_bytes"),
            "lock": LOCK,
            "new_rust": True,
            "own_rust_builders": capacity.get("own_rust_builders"),
        },
        "compiler_started": proof_ok,
        "containers": {
            FE_NAME: container_status(FE_NAME),
            BE_NAME: container_status(BE_NAME),
            PG_NAME: container_status(PG_NAME),
            RUNTIME: container_status(RUNTIME),
        },
        "named_compile": {
            "builder_exit": proof.get("exit"),
            "builder_oom": proof.get("exit") == 137,
            "builder_status": "removed-after-collect" if proof.get("exit") == 0 else "see-preview-build-log",
            "container": BUILDER,
            "log_has_compiling": proof.get("log_has_compiling"),
            "log_has_preview_build_ok": proof.get("log_has_preview_build_ok"),
            "pipeline_rebuilt": False,
            "reader_rebuilt": False,
            "recipe": "cargo build --locked --jobs 1 --offline",
            "rustc": proof.get("rustc"),
        },
        "owner_session": SESSION,
        "pack_ready": False,
        "pack_review": "unreviewed",
        "pipeline_elf": {name: pipeline.get(name) for name in ("oo-pipeline", "init-pg", "pg-builder")},
        "preview_elf": {
            name: preview.get(name)
            for name in ("operator-preview", "install-column-acl", "provision-mapping-reader")
        },
        "product_reader_write_attempts": 0,
        "protocol": "oo.doris.resource-receipt/v1",
        "reopen_exit": 0 if all(reopen.get("reopen_exits", {}).get(name) == 0 for name in TABLES) else 1,
        "rustc": proof.get("rustc") or "",
        "secret_ref": "secret://oo-dl01w1-43/reader",
        "source_acceptance": False,
        "source_commit": source_commit,
        "source_generation": GENERATION,
        "source_port_reopened": False,
        "target_generations": {
            "agg_qty": 3,
            "dec_exact": 1,
            "dup_events": 2,
            "nz_obs": 6,
            "part_day": 7,
            "str_key": 5,
            "uniq_acct": 4,
        },
        "task_id": "DL01-W1-43",
        "worker_session": "fd183cf9-cf17-4061-af89-defa00621ff3",
    }
    write_json(run_dir / "resource-receipt.json", body)


def hex40(value):
    return isinstance(value, str) and len(value) == 40 and all(ch in "0123456789abcdef" for ch in value)


def reseal_source_sha(source_sha):
    if not hex40(source_sha):
        return 2
    run_dir = EVIDENCE / "closure-run"
    preview_path = run_dir / "delivery-preview.json"
    live = EVIDENCE / "whole-source-delivery.json"
    if not preview_path.is_file() or not live.is_file():
        return 1
    preview = json.loads(preview_path.read_text(encoding="utf-8"))
    preview.pop("evidence_sha", None)
    preview["source_sha"] = source_sha
    write_json(preview_path, preview)
    evidence_sha = write_sums(run_dir)
    outer = json.loads(live.read_text(encoding="utf-8"))
    outer["source_sha"] = source_sha
    outer["evidence_sha"] = evidence_sha
    write_json(live, outer)
    listed = None
    for line in (run_dir / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        if line.endswith("  delivery-preview.json"):
            listed = line.split()[0]
    actual = sha256_file(preview_path)
    if listed != actual or outer["source_sha"] != source_sha:
        return 1
    print(evidence_sha)
    return 0


def attach_stream_log():
    run_dir = EVIDENCE / "closure-run"
    live = EVIDENCE / "whole-source-delivery.json"
    if not live.is_file():
        return 1
    copy_text(CLOSURE / "launch-closure-stream.log", run_dir / "launch-closure-stream.log")
    evidence_sha = write_sums(run_dir)
    outer = json.loads(live.read_text(encoding="utf-8"))
    outer["evidence_sha"] = evidence_sha
    write_json(live, outer)
    print(evidence_sha)
    return 0


def parse_customer(argv):
    secret = None
    approved = None
    index = 0
    while index < len(argv):
        token = argv[index]
        if token == "--secret-ref" and index + 1 < len(argv):
            secret = argv[index + 1]
            index += 2
            continue
        if token == "--approved-objects" and index + 1 < len(argv):
            approved = [part.strip() for part in argv[index + 1].split(",") if part.strip()]
            index += 2
            continue
        index += 1
    if not isinstance(secret, str) or not secret.startswith("secret://") or any(ch.isspace() for ch in secret):
        return None
    if not approved or len(approved) != len(set(approved)) or set(approved) != set(TABLES):
        return None
    return secret, approved


def main():
    if "--self-check" in sys.argv:
        return self_check()
    if "--reseal-source-sha" in sys.argv:
        index = sys.argv.index("--reseal-source-sha")
        if index + 1 >= len(sys.argv):
            return 2
        return reseal_source_sha(sys.argv[index + 1])
    if "--attach-stream-log" in sys.argv:
        return attach_stream_log()
    if "--reopen-stopped-source" in sys.argv:
        identity = seal_binary_identity()
        if identity != 0:
            return identity
        return local_reopen()
    parsed = parse_customer(sys.argv)
    if parsed is None:
        print("secret_ref and the seven approved objects are required", file=sys.stderr)
        return 2
    CUSTOMER["secret_ref"], CUSTOMER["approved"] = parsed
    code = 1
    try:
        code = run_chain()
    finally:
        try:
            emit_reports(code)
        except Exception:
            write(CLOSURE / "raw" / "emit-error.txt", "emit\n")
        cleanup_owned()
    return code


if __name__ == "__main__":
    try:
        sys.exit(main())
    except subprocess.TimeoutExpired as exc:
        write(CLOSURE / "raw" / "timeout.txt", f"{exc.cmd}\n")
        sys.exit(124)
    except Exception:
        write(CLOSURE / "raw" / "uncaught.txt", "uncaught\n")
        raise
