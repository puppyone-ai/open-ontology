#!/usr/bin/env python3
"""Dock the committed ClickHouse capture to the current DL01-W1-71 control API.

The bundle is the approved five-column scope. note stays bytea, which is outside
the OOS data_type enum, so S2 is expected to fail closed. This script does not
call catalog-import-native, does not set source_protocol, and does not write a
Pack. Secrets stay under /tmp.
"""

from __future__ import annotations

import hashlib
import json
import os
import socket
import ssl
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
PRODUCT = ROOT / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product"
NATIVE = PRODUCT / "raw/run/product-run-16-native"
OUT = PRODUCT / "raw/run/product-run-18-builder-input"
BUNDLE = OUT / "bundle"
CACHE = Path("/tmp/oo-dl01w1-40-selfbuild")
ELF = Path("/Users/redux/.cache/oo-dl01w1-71/selfbuild-control-api")
ELF_SHA = "dce12ddd7243e8433f54dd945c24051545411dda0e72079a39a316bc592893e9"
PG_BUILDER = Path("/tmp/oo-u06-194/debug/pg-builder")
PG_BUILDER_SHA = "af5e79560d50a1c29cee465a95361b7e3a1632e5a51156c46e154e2363172c54"
PIPELINE = Path("/tmp/oo-u06-194/debug/oo-pipeline")
PIPELINE_SHA = "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
RUST_IMAGE = "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE = "sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
PG_ENTRY = ROOT / "tools/selfbuild-control-api/linux/pg-entry.sh"
BINDING = ROOT / "crates/oo-pipeline/src/native_binding.rs"
CONTRACT = ROOT / "tools/source-admission/CONTRACT.md"
API_HOST = "127.0.0.1"
API_PORT = 18640
IDENTITY_PORT = 18641
ORIGIN = f"https://{API_HOST}:{API_PORT}"
IDENTITY_ORIGIN = f"https://host.docker.internal:{IDENTITY_PORT}"
NET = "oo-dl01w1-40-sb-net"
PG_NAME = "oo-dl01w1-40-sb-pg"
API_NAME = "oo-dl01w1-40-sb-api"
SEAL_NAME = "oo-dl01w1-40-sb-seal"
VOLUME = "oo-dl01w1-40-sb-pgdata"
COOKIE = "c" * 43
CSRF = "k" * 43
PROJECT_ID = "clickhouse_268"
SOURCE_REF = "clickhouse_268"
ALLOWANCE_ID = "rows_100000"
PROTOCOL_TOKEN = "secret://" + "llm/not-present"
SOURCE_TOKEN = "secret://" + "source/not-opened"
IDEMPOTENCY_KEY = "ch268sb71create0001"
PIN_TEXT = "oo.clickhouse.selfbuild-control-pin/v1\nclickhouse_268\n"
COMMIT = hashlib.sha256(PIN_TEXT.encode("utf-8")).hexdigest()
ROLE = "oo_r_reader"
OOS_TYPES = {
    "bigint",
    "integer",
    "numeric",
    "text",
    "boolean",
    "date",
    "timestamp",
    "timestamptz",
    "jsonb",
    "uuid",
}
BUNDLE_FILES = [
    "catalog.json",
    "profile.json",
    "read_logic.json",
    "source_semantics.json",
    "rules.json",
    "suggestions.json",
    "generation.json",
    "roles.json",
]
APPROVED = ("reading_id", "ts", "amount", "note", "qty")


class CallError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def numeric_typmod(precision: int, scale: int) -> int:
    return ((precision << 16) | scale) + 4


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def run(args: list[str]) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)


def docker_text(args: list[str]) -> str:
    result = run(["docker", *args])
    return (result.stdout or b"").decode("utf-8", "replace").strip()


def load_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def scrub(text: str, secrets: list[bytes]) -> str:
    for secret in secrets:
        if secret:
            text = text.replace(secret.decode("utf-8", "replace"), "[redacted]")
    text = text.replace(PROTOCOL_TOKEN, "[redacted]")
    text = text.replace(SOURCE_TOKEN, "[redacted]")
    text = text.replace("secret_ref", "ref_field")
    return text


def scrub_outputs(secrets: list[bytes]) -> None:
    for path in OUT.rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes()
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            continue
        cleaned = scrub(text, secrets)
        if cleaned != text:
            path.write_text(cleaned if cleaned.endswith("\n") or not cleaned else cleaned + "\n", encoding="utf-8")


def leak(secrets: list[bytes]) -> list[str]:
    needles = [b"secret://", b"secret_ref", *secrets]
    hits = []
    for path in OUT.rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes()
        for needle in needles:
            if needle and needle in data:
                hits.append(str(path.relative_to(OUT)))
                break
    return hits


def require_name_free(name: str) -> None:
    found = docker_text(["ps", "-aq", "--filter", f"name=^{name}$"])
    if found:
        raise CallError(f"{name} already exists")


def volume_exists(name: str) -> bool:
    return bool(docker_text(["volume", "ls", "-q", "--filter", f"name=^{name}$"]))


def git_blob(spec: str) -> str:
    return subprocess.check_output(["git", "rev-parse", spec], text=True).strip()


def binding_record() -> dict:
    text = BINDING.read_text(encoding="utf-8")
    return {
        "catalog_import_native": "not_rerun",
        "contract_head_blob": git_blob(f"HEAD:{CONTRACT.relative_to(ROOT).as_posix()}"),
        "contract_work_blob": subprocess.check_output(["git", "hash-object", str(CONTRACT)], text=True).strip(),
        "elf_rebuilt": False,
        "head_blob": git_blob(f"HEAD:{BINDING.relative_to(ROOT).as_posix()}"),
        "pipeline_elf_sha256": PIPELINE_SHA,
        "work_blob": subprocess.check_output(["git", "hash-object", str(BINDING)], text=True).strip(),
        "work_passes_full_registry_hash": "registry_sha256: registry_sha," in text,
        "work_passes_preimage": "registry_sha256: &preimage," in text,
    }


def projections(types_doc: dict) -> list[dict]:
    by_name = {item["name"]: item for item in types_doc["columns"]}
    if tuple(by_name) != APPROVED:
        raise SystemExit("native type file is not the five approved columns")
    specs = {
        "reading_id": {
            "arrow_type": "uint64",
            "data_type": "numeric",
            "pg_type": "numeric",
            "oid": 1700,
            "typmod": numeric_typmod(20, 0),
            "caveat": "bigint_unsigned_numeric",
            "nullable": False,
            "key": True,
            "precision_loss": False,
        },
        "ts": {
            "arrow_type": "timestamp_nanos_utc",
            "data_type": "numeric",
            "pg_type": "numeric",
            "oid": 1700,
            "typmod": numeric_typmod(20, 0),
            "caveat": "",
            "nullable": False,
            "key": False,
            "precision_loss": False,
            "not_timestamptz": True,
        },
        "amount": {
            "arrow_type": "decimal256",
            "data_type": "numeric",
            "pg_type": "numeric",
            "oid": 1700,
            "typmod": numeric_typmod(76, 10),
            "precision": 76,
            "scale": 10,
            "caveat": "",
            "nullable": False,
            "key": False,
            "precision_loss": True,
        },
        "note": {
            "arrow_type": "binary",
            "data_type": "bytea",
            "pg_type": "bytea",
            "oid": 17,
            "typmod": -1,
            "caveat": "",
            "nullable": True,
            "key": False,
            "precision_loss": False,
        },
        "qty": {
            "arrow_type": "int64",
            "data_type": "bigint",
            "pg_type": "int8",
            "oid": 20,
            "typmod": -1,
            "caveat": "",
            "nullable": False,
            "key": False,
            "precision_loss": False,
        },
    }
    columns = []
    for name in APPROVED:
        got = by_name[name]
        spec = specs[name]
        if got.get("arrow_type") != spec["arrow_type"] or bool(got.get("nullable")) != spec["nullable"]:
            raise SystemExit(f"{name} arrow type drifted")
        if name == "amount" and (got.get("precision") != 76 or got.get("scale") != 10):
            raise SystemExit("amount precision drifted")
        column = {
            "name": name,
            "data_type": spec["data_type"],
            "arrow_type": spec["arrow_type"],
            "pg_type": spec["pg_type"],
            "oid": spec["oid"],
            "typmod": spec["typmod"],
            "caveat": spec["caveat"],
            "nullable": spec["nullable"],
            "key": spec["key"],
            "precision_loss": spec["precision_loss"],
            "oos_data_type": spec["data_type"] in OOS_TYPES,
        }
        if "precision" in spec:
            column["precision"] = spec["precision"]
            column["scale"] = spec["scale"]
        if spec.get("not_timestamptz"):
            column["not_timestamptz"] = True
        columns.append(column)
    return columns


def write_bundle() -> dict:
    types_doc = load_json(NATIVE / "identity/types.json")
    schema_doc = load_json(NATIVE / "identity/schema.json")
    config_doc = load_json(NATIVE / "identity/config.json")
    schedules = load_json(NATIVE / "schedules.json")
    refusals = load_json(NATIVE / "refusals.json")
    writes = load_json(NATIVE / "product-writes-final.json")
    truth = load_json(NATIVE / "loads/gen4/truth.json")
    if config_doc.get("fields") != list(APPROVED):
        raise SystemExit("capture config fields are not the approved five")
    if config_doc.get("budget") != {"hard_limit": 100000, "unit": "Rows"}:
        raise SystemExit("capture budget drifted")
    columns = projections(types_doc)
    physical = [{"name": item["name"], "type": item["type"], "approved": item["name"] in APPROVED} for item in schema_doc["columns"]]
    cites = {
        "capture_catalog_sha256": sha256_file(NATIVE / "identity/catalog.json"),
        "capture_schema_sha256": sha256_file(NATIVE / "identity/schema.json"),
        "capture_types_sha256": sha256_file(NATIVE / "identity/types.json"),
        "capture_config_sha256": sha256_file(NATIVE / "identity/config.json"),
        "schedules_sha256": sha256_file(NATIVE / "schedules.json"),
        "refusals_sha256": sha256_file(NATIVE / "refusals.json"),
        "product_writes_sha256": sha256_file(NATIVE / "product-writes-final.json"),
        "generation_4_truth_sha256": sha256_file(NATIVE / "loads/gen4/truth.json"),
        "generation_receipt_sha256": {
            name: sha256_file(NATIVE / "loads" / name / "receipt.json") for name in ("gen1", "gen2", "gen3", "gen4")
        },
    }
    refused = []
    for name, item in refusals.items():
        refused.append({"name": name, "admitted": item.get("admitted"), "reason": item.get("reason"), "source_read": item.get("source_read")})
    catalog = {
        "contract": "oo.clickhouse.approved-scope-catalog/v1",
        "source_id": SOURCE_REF,
        "tables": [{"columns": columns, "reltuples": 3, "schema": "ch", "table": "readings"}],
    }
    profile = {
        "budget": config_doc["budget"],
        "capture_cites": cites,
        "contract": "oo.clickhouse.capture-profile/v1",
        "executed": False,
        "generation_4_rows": truth,
        "locale": config_doc.get("locale"),
        "object": config_doc.get("object"),
        "physical_columns": physical,
        "row_count_generation_4": 3,
        "source_id": SOURCE_REF,
        "tombstone_generation_4": ["102"],
    }
    read_logic = {
        "capture_method": "c4_partition",
        "consistency_grade": "S0",
        "contract": "oo.clickhouse.read-logic/v1",
        "cross_table": "B",
        "delete_detection": config_doc.get("delete_detection"),
        "disclosure": config_doc.get("disclosure"),
        "executed": False,
        "not_log_cdc": True,
        "periods": schedules,
        "source_write_count": writes.get("write_count"),
        "sync": config_doc.get("sync"),
    }
    semantics = {
        "confidence": "unknown",
        "contract": "oo.clickhouse.source-semantics/v1",
        "executed": False,
        "model_called": False,
        "pii": "unknown",
        "refused_before_read": refused,
        "semantic_types_declared": False,
        "unapproved_physical_columns": [item["name"] for item in physical if not item["approved"]],
    }
    rules = {
        "columns": [
            {"column": f"ch.readings.{name}", "confidence": "unknown", "pii": "unknown"} for name in APPROVED
        ],
        "contract": "oo.clickhouse.sealed-rules/v1",
        "relationships": [],
    }
    suggestions = {
        "certified": False,
        "contract": "oo.selfbuild-control-api/sealed-suggestions/v1",
        "merged": False,
        "ready": False,
    }
    generation = {
        "contract": "oo.selfbuild-control-api/sealed-generation/v1",
        "llm_calls": 0,
        "outcome": "model_unauthorized",
    }
    roles = {
        "contract": "oo.selfbuild-control-api/sealed-roles/v1",
        "roles": {ROLE: [f"ch.readings.{name}" for name in APPROVED]},
    }
    files = {
        "catalog.json": catalog,
        "profile.json": profile,
        "read_logic.json": read_logic,
        "source_semantics.json": semantics,
        "rules.json": rules,
        "suggestions.json": suggestions,
        "generation.json": generation,
        "roles.json": roles,
    }
    BUNDLE.mkdir(parents=True, exist_ok=True)
    encoded = {}
    for name, value in files.items():
        text = json.dumps(value, indent=2, sort_keys=True) + "\n"
        if len(text.encode("utf-8")) > 65536:
            raise SystemExit(f"{name} exceeds the loader limit")
        (BUNDLE / name).write_text(text, encoding="utf-8")
        encoded[name] = sha256_bytes(text.encode("utf-8"))
    manifest = {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": encoded}
    (BUNDLE / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    outside = [item["name"] for item in columns if not item["oos_data_type"]]
    write_json(OUT / "inputs.json", {"bundle_sha256": encoded, "cites": cites, "oos_outside": outside, "pin_preimage": PIN_TEXT, "pin_sha256": COMMIT})
    return {"outside": outside, "sha256": encoded}


def observe(binding: dict) -> dict:
    token = Path("/tmp/oo-customer-demo-20261007/llm-token")
    entries = sorted(os.listdir(token)) if token.is_dir() else []
    return {
        "binding": binding,
        "catalog_import_native": "not_rerun",
        "clickhouse_containers": docker_text(["ps", "-a", "--filter", "name=oo-dl01w1-40", "--format", "{{.Names}} {{.Status}}"]),
        "elf71_sha256": sha256_file(ELF),
        "foreign_pg": docker_text(["ps", "-a", "--filter", "id=9e49238c6b319521cb3f6ef1781208bd954ec3415552b056940a4b5d6842e9b6", "--format", "{{.ID}} {{.Names}} {{.Status}}"]),
        "listener_18449": docker_text(["ps", "-a", "--filter", "name=oo-dl01w1-71", "--format", "{{.Names}} {{.Status}}"]),
        "oo_on_path": bool(subprocess.run(["bash", "-lc", "command -v oo"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False).stdout.strip()),
        "pg_builder_sha256": sha256_file(PG_BUILDER),
        "pipeline_sha256": sha256_file(PIPELINE),
        "token_entries": entries,
        "token_is_dir": token.is_dir(),
    }


def existing_seal() -> dict | None:
    path = OUT / "import-seal" / "summary.json"
    if not path.is_file():
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        document.get("exit") == 1
        and document.get("outcome") == "compiler_refused"
        and document.get("model") == "not_called"
        and document.get("pack") == "not_produced"
    ):
        kept = dict(document)
        kept["reused_first_refusal"] = True
        return kept
    return None


def import_seal() -> dict:
    out_dir = OUT / "import-seal"
    out_dir.mkdir(parents=True, exist_ok=True)
    catalog = NATIVE / "identity/catalog.json"
    cmd = [
        "docker",
        "run",
        "-d",
        "--name",
        SEAL_NAME,
        "--pull",
        "never",
        "--platform",
        "linux/arm64",
        "--label",
        "oo.owner=dl01w1-40",
        "--label",
        "oo.task=DL01-W1-40",
        "-v",
        f"{catalog.parent}:/in:ro",
        "-v",
        f"{out_dir}:/out",
        "-v",
        f"{PG_BUILDER}:/opt/pg-builder:ro",
        RUST_IMAGE,
        "/opt/pg-builder",
        "import-seal",
        "--catalog",
        "/in/catalog.json",
        "--out",
        "/out/import-seal.json",
    ]
    started = run(cmd)
    write_bytes(out_dir / "start.stdout", started.stdout or b"")
    write_bytes(out_dir / "start.stderr", started.stderr or b"")
    (out_dir / "start.exit").write_text(f"{started.returncode}\n", encoding="utf-8")
    if started.returncode != 0:
        return {"start_exit": started.returncode, "outcome": "start_failed"}
    time.sleep(0.3)
    host = {
        "nanocpus": docker_text(["inspect", SEAL_NAME, "--format", "{{.HostConfig.NanoCpus}}"]),
        "memory": docker_text(["inspect", SEAL_NAME, "--format", "{{.HostConfig.Memory}}"]),
        "cpu_quota": docker_text(["inspect", SEAL_NAME, "--format", "{{.HostConfig.CpuQuota}}"]),
    }
    cpu = run(["docker", "exec", SEAL_NAME, "cat", "/sys/fs/cgroup/cpu.max"])
    host["cpu_max_exit"] = cpu.returncode
    host["cpu_max"] = (cpu.stdout or b"").decode("utf-8", "replace").strip()
    waited = run(["docker", "wait", SEAL_NAME])
    logs = run(["docker", "logs", SEAL_NAME])
    write_bytes(out_dir / "stdout", logs.stdout or b"")
    write_bytes(out_dir / "stderr", logs.stderr or b"")
    exit_code = (waited.stdout or b"").decode("utf-8", "replace").strip()
    (out_dir / "exit").write_text(exit_code + "\n", encoding="utf-8")
    run(["docker", "rm", SEAL_NAME])
    document = {}
    body = out_dir / "import-seal.json"
    if body.is_file():
        document = json.loads(body.read_text(encoding="utf-8"))
    result = {
        "cpu": host,
        "exit": int(exit_code) if exit_code.lstrip("-").isdigit() else exit_code,
        "model": document.get("model"),
        "outcome": document.get("outcome"),
        "pack": document.get("pack"),
        "compiler": document.get("compiler"),
        "code": document.get("code"),
    }
    write_json(out_dir / "summary.json", result)
    return result


def make_certs(secrets: list[bytes]) -> None:
    ca = CACHE / "tls/ca"
    ca.mkdir(parents=True, exist_ok=True)
    subprocess.check_call(
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(ca / "ca.key"), "-out", str(ca / "ca.crt"),
            "-days", "30", "-subj", "/CN=oo-dl01w1-40-sb-ca",
            "-addext", "basicConstraints=critical,CA:TRUE",
            "-addext", "keyUsage=critical,keyCertSign,cRLSign",
            "-addext", "subjectKeyIdentifier=hash",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    def sign(name: str, ext: Path) -> None:
        folder = CACHE / "tls" / name
        folder.mkdir(parents=True, exist_ok=True)
        subprocess.check_call(
            ["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", str(folder / "server.key"), "-out", str(folder / "server.csr"), "-subj", f"/CN={name}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        subprocess.check_call(
            ["openssl", "x509", "-req", "-in", str(folder / "server.csr"), "-CA", str(ca / "ca.crt"), "-CAkey", str(ca / "ca.key"), "-CAcreateserial", "-out", str(folder / "server.crt"), "-days", "30", "-extfile", str(ext)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        (folder / "server.crt").write_bytes((folder / "server.crt").read_bytes() + b"\n" + (ca / "ca.crt").read_bytes())
        (folder / "ca.crt").write_bytes((ca / "ca.crt").read_bytes())
        (folder / "server.csr").unlink(missing_ok=True)

    (CACHE / "tls/pg.ext").write_text(
        "basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid,issuer\n"
        f"subjectAltName=DNS:{PG_NAME}\n",
        encoding="utf-8",
    )
    (CACHE / "tls/api.ext").write_text(
        "basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid,issuer\nsubjectAltName=IP:127.0.0.1\n",
        encoding="utf-8",
    )
    (CACHE / "tls/identity.ext").write_text(
        "basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid,issuer\n"
        "subjectAltName=DNS:host.docker.internal,IP:127.0.0.1\n",
        encoding="utf-8",
    )
    sign("pg", CACHE / "tls/pg.ext")
    sign("api", CACHE / "tls/api.ext")
    sign("identity", CACHE / "tls/identity.ext")
    secret_dir = CACHE / "secrets"
    secret_dir.mkdir(parents=True, exist_ok=True)
    password = subprocess.check_output(["openssl", "rand", "-base64", "24"]).strip()
    source_password = subprocess.check_output(["openssl", "rand", "-base64", "24"]).strip()
    (secret_dir / "pg.password").write_bytes(password + b"\n")
    (secret_dir / "source.password").write_bytes(source_password + b"\n")
    os.chmod(secret_dir / "pg.password", 0o600)
    os.chmod(secret_dir / "source.password", 0o600)
    os.chmod(ca / "ca.key", 0o600)
    secrets.extend([password, source_password])
    write_json(
        OUT / "phases" / "secret-paths.json",
        {
            "pg_password_mode": oct(os.stat(secret_dir / "pg.password").st_mode & 0o777),
            "source_password_mode": oct(os.stat(secret_dir / "source.password").st_mode & 0o777),
            "values_in_evidence": False,
        },
    )


def write_config() -> None:
    (CACHE / "work").mkdir(parents=True, exist_ok=True)
    (CACHE / "bin").mkdir(parents=True, exist_ok=True)
    write_json(CACHE / "role-policy.json", {"active": False, "published": False})
    endpoint = {
        "ca_file": str(CACHE / "tls/ca/ca.crt"),
        "database": "oo_ch40_sb",
        "host": PG_NAME,
        "password_file": str(CACHE / "secrets/pg.password"),
        "port": 5432,
        "server_name": PG_NAME,
        "user": "oo_selfbuild_api_admin",
    }
    config = {
        "binaries": {
            "catalog": str(CACHE / "bin/missing-catalog"),
            "pg_builder": str(CACHE / "bin/missing-pg-builder"),
            "review_ledger": str(CACHE / "bin/missing-review-ledger"),
            "rules": str(CACHE / "bin/missing-rules"),
            "suggestions": str(CACHE / "bin/missing-suggestions"),
        },
        "contract": "oo.selfbuild-control-api.config/v1",
        "deployment_origins": [ORIGIN],
        "identity": {"ca_file": str(CACHE / "tls/ca/ca.crt"), "origin": IDENTITY_ORIGIN},
        "ledger": endpoint,
        "listen": {"bind": "0.0.0.0:8443", "cert_file": str(CACHE / "tls/api/server.crt"), "key_file": str(CACHE / "tls/api/server.key")},
        "model_cache_dir": "",
        "postgres": endpoint,
        "principals": [],
        "projects": [
            {
                "allowance_id": ALLOWANCE_ID,
                "config_commit": COMMIT,
                "hidden_columns": [],
                "llm": {"bearer_file": str(CACHE / "llm-bearer-absent"), "model_id": "grok-4.7", "origin": "http://127.0.0.1:8317"},
                "llm_secret_ref": PROTOCOL_TOKEN,
                "project_id": PROJECT_ID,
                "role_policy_file": str(CACHE / "role-policy.json"),
                "rules_auto_accept": "none",
                "sample_policy": "none",
                "sealed_bundle": str(BUNDLE),
                "source": {
                    "ca_file": str(CACHE / "tls/ca/ca.crt"),
                    "database": "oo_dl01w40",
                    "host": "unused.invalid",
                    "password_file": str(CACHE / "secrets/source.password"),
                    "port": 8123,
                    "tables": [{"schema": "ch", "table": "readings"}],
                    "username": "oo_dl01w1_40_reader",
                },
                "source_ref": SOURCE_REF,
                "source_secret_ref": SOURCE_TOKEN,
                "visible_columns": [],
            }
        ],
        "work_dir": str(CACHE / "work"),
    }
    write_json(CACHE / "config.json", config)
    public = json.loads(json.dumps(config))
    public["postgres"].pop("password_file", None)
    public["ledger"].pop("password_file", None)
    project = public["projects"][0]
    project.pop("llm_secret_ref", None)
    project.pop("source_secret_ref", None)
    project["source"].pop("password_file", None)
    project["source_protocol_set"] = False
    write_json(OUT / "config.public.json", public)


class Identity(threading.Thread):
    def __init__(self) -> None:
        super().__init__(daemon=True)
        self.ready = threading.Event()
        self.stop = threading.Event()
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(CACHE / "tls/identity/server.crt", CACHE / "tls/identity/server.key")
        self.socket = socket.socket()
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.socket.bind(("0.0.0.0", IDENTITY_PORT))
        self.socket.listen(32)
        self.socket.settimeout(0.5)

    def run(self) -> None:
        self.ready.set()
        while not self.stop.is_set():
            try:
                client, _address = self.socket.accept()
            except socket.timeout:
                continue
            threading.Thread(target=self.serve, args=(client,), daemon=True).start()

    def serve(self, client: socket.socket) -> None:
        try:
            tls = self.context.wrap_socket(client, server_side=True)
            data = b""
            tls.settimeout(5)
            while b"\r\n\r\n" not in data and len(data) < 65536:
                chunk = tls.recv(4096)
                if not chunk:
                    break
                data += chunk
            head = data.split(b"\r\n\r\n", 1)[0].decode("iso-8859-1", "replace")
            line = head.split("\r\n", 1)[0]
            path = line.split(" ")[1] if " " in line else ""
            headers = {}
            for item in head.split("\r\n")[1:]:
                if ":" in item:
                    name, value = item.split(":", 1)
                    headers[name.strip().lower()] = value.strip()
            cookie = ""
            for part in headers.get("cookie", "").split(";"):
                part = part.strip()
                if part.startswith("__Host-oo-session="):
                    cookie = part.split("=", 1)[1]
            if path.startswith("/control/v1/identity/current"):
                if cookie != COOKIE:
                    tls.sendall(http_response(401, "Unauthorized", b'{"error":{"code":"unauthenticated"}}'))
                else:
                    tls.sendall(http_response(200, "OK", json.dumps(envelope()).encode("utf-8")))
            elif path.startswith("/control/v1/auth/bootstrap/consume"):
                if cookie != COOKIE:
                    tls.sendall(http_response(401, "Unauthorized", b'{"error":{"code":"unauthenticated"}}'))
                elif headers.get("x-oo-csrf") != CSRF:
                    tls.sendall(http_response(403, "Forbidden", b'{"error":{"code":"csrf.rejected"}}'))
                else:
                    tls.sendall(http_response(409, "Conflict", b'{"error":{"code":"bootstrap.consumed"}}'))
            else:
                tls.sendall(http_response(404, "Not Found", b'{"error":{"code":"not_found"}}'))
            tls.close()
        except Exception:
            try:
                client.close()
            except Exception:
                pass

    def close(self) -> None:
        self.stop.set()
        try:
            self.socket.close()
        except Exception:
            pass


def envelope() -> dict:
    return {
        "agent_session": "not_issued",
        "authority": "management_session",
        "config_commit": COMMIT,
        "contract": "oo.control-identity/v1",
        "csrf_token": CSRF,
        "customer_integration": "not_claimed",
        "data_grants": "none",
        "gateway_token": "not_issued",
        "harness_session": "not_issued",
        "install_state": "initialized",
        "issuer": "https://oo-dl01w1-40.example",
        "operation": "getIdentityCurrent",
        "owner_certification": "not_issued",
        "projects": [{"capabilities": ["project.admin"], "project_id": PROJECT_ID, "role": "admin"}],
        "revision": 1,
        "session": {"absolute_expires_at": "2027-01-02T00:00:00Z", "idle_expires_at": "2027-01-01T00:00:00Z", "revoked": False},
        "session_class": "management_cookie",
        "subject": "user-admin",
    }


def http_response(status: int, reason: str, body: bytes) -> bytes:
    head = f"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n"
    return head.encode("ascii") + body


def parse_http(data: bytes) -> tuple[int, bytes]:
    header, _, body = data.partition(b"\r\n\r\n")
    text = header.decode("iso-8859-1", "replace")
    status = int(text.split("\r\n", 1)[0].split(" ")[1])
    for line in text.split("\r\n"):
        if line.lower().startswith("content-length:"):
            body = body[: int(line.split(":", 1)[1].strip())]
            break
    return status, body


def exchange(method: str, path: str, body: dict | None, role: bool, name: str) -> tuple[int, object]:
    payload = b"" if body is None else json.dumps(body, sort_keys=True).encode("utf-8")
    head = [f"{method} {path} HTTP/1.1", f"Host: {API_HOST}", "Accept: application/json", "Connection: close"]
    if role:
        head.append(f"Cookie: __Host-oo-session={COOKIE}")
    if body is not None:
        head.append(f"Origin: {ORIGIN}")
        head.append("Content-Type: application/json")
        head.append(f"Content-Length: {len(payload)}")
        if role:
            head.append(f"X-OO-CSRF: {CSRF}")
    request = ("\r\n".join(head) + "\r\n\r\n").encode("ascii") + payload
    context = ssl.create_default_context(cafile=str(CACHE / "tls/ca/ca.crt"))
    sock = context.wrap_socket(socket.create_connection((API_HOST, API_PORT), 10), server_hostname=API_HOST)
    sock.sendall(request)
    data = b""
    while True:
        chunk = sock.recv(65536)
        if chunk:
            data += chunk
        if b"\r\n\r\n" in data:
            header, _, rest = data.partition(b"\r\n\r\n")
            if b"content-length:" in header.lower():
                length = 0
                for line in header.split(b"\r\n"):
                    if line.lower().startswith(b"content-length:"):
                        length = int(line.split(b":", 1)[1].strip())
                if len(rest) >= length:
                    break
        if not chunk:
            break
    sock.close()
    status, response = parse_http(data)
    record = {"request_sha256": sha256_bytes(request), "response_sha256": sha256_bytes(response), "status": status}
    parsed: object = {}
    if response:
        try:
            parsed = json.loads(response)
        except json.JSONDecodeError:
            parsed = {"raw_sha256": sha256_bytes(response)}
    if isinstance(parsed, dict):
        record["response"] = parsed
    write_json(OUT / "http" / f"{name}.json", record)
    return status, parsed


def code_of(document: object) -> str:
    if isinstance(document, dict):
        error = document.get("error")
        if isinstance(error, dict) and isinstance(error.get("code"), str):
            return error["code"]
    return ""


def cgroup(name: str) -> dict:
    inspect = {
        "nanocpus": docker_text(["inspect", name, "--format", "{{.HostConfig.NanoCpus}}"]),
        "memory": docker_text(["inspect", name, "--format", "{{.HostConfig.Memory}}"]),
        "cpu_quota": docker_text(["inspect", name, "--format", "{{.HostConfig.CpuQuota}}"]),
    }
    cpu = run(["docker", "exec", name, "cat", "/sys/fs/cgroup/cpu.max"])
    inspect["cpu_max_exit"] = cpu.returncode
    inspect["cpu_max"] = (cpu.stdout or b"").decode("utf-8", "replace").strip()
    return inspect


def start_pg(started: list[str]) -> None:
    cmd = [
        "docker", "run", "-d", "--name", PG_NAME, "--pull", "never", "--platform", "linux/arm64",
        "--hostname", PG_NAME, "--network", NET, "--label", "oo.owner=dl01w1-40", "--label", "oo.task=DL01-W1-40",
        "--label", "oo.kind=postgres",
        "-e", "POSTGRES_USER=oo_selfbuild_api_admin", "-e", "POSTGRES_DB=oo_ch40_sb",
        "-e", "POSTGRES_PASSWORD_FILE=/secrets/pg.password",
        "-e", "POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256 --auth-local=scram-sha-256",
        "-v", f"{CACHE}/secrets:/secrets:ro", "-v", f"{CACHE}/tls/pg:/tls-in:ro",
        "-v", f"{VOLUME}:/var/lib/postgresql/data", "-v", f"{PG_ENTRY}:/pg-entry.sh:ro",
        "--entrypoint", "/bin/sh", PG_IMAGE, "/pg-entry.sh",
    ]
    result = run(cmd)
    write_bytes(OUT / "phases" / "pg-start.stdout", result.stdout or b"")
    write_bytes(OUT / "phases" / "pg-start.stderr", result.stderr or b"")
    (OUT / "phases" / "pg-start.exit").write_text(f"{result.returncode}\n", encoding="utf-8")
    if result.returncode != 0:
        raise CallError("pg start failed")
    started.append(PG_NAME)
    for i in range(1, 61):
        ready = run(["docker", "exec", PG_NAME, "pg_isready", "-U", "oo_selfbuild_api_admin", "-d", "oo_ch40_sb"])
        if ready.returncode == 0:
            (OUT / "phases" / "pg-ready-tries.txt").write_text(f"{i}\n", encoding="utf-8")
            return
        time.sleep(2)
    raise CallError("pg did not become ready")


def start_api(started: list[str]) -> None:
    common = [
        "--pull", "never", "--platform", "linux/arm64", "--network", NET,
        "--add-host", "host.docker.internal:host-gateway",
        "-v", f"{CACHE}:{CACHE}", "-v", f"{BUNDLE}:{BUNDLE}:ro", "-v", f"{ELF}:/opt/selfbuild-control-api:ro",
    ]
    install = ["docker", "run", "--rm", *common, RUST_IMAGE, "/opt/selfbuild-control-api", "install", "--config", str(CACHE / "config.json")]
    installed = run(install)
    write_bytes(OUT / "phases" / "install.stdout", installed.stdout or b"")
    write_bytes(OUT / "phases" / "install.stderr", installed.stderr or b"")
    (OUT / "phases" / "install.exit").write_text(f"{installed.returncode}\n", encoding="utf-8")
    if installed.returncode != 0:
        raise CallError("install failed")
    serve = [
        "docker", "run", "-d", "--name", API_NAME, *common, "-p", f"127.0.0.1:{API_PORT}:8443",
        "--label", "oo.owner=dl01w1-40", "--label", "oo.task=DL01-W1-40", "--label", "oo.kind=control-api",
        RUST_IMAGE, "/opt/selfbuild-control-api", "serve", "--config", str(CACHE / "config.json"),
    ]
    served = run(serve)
    write_bytes(OUT / "phases" / "api-start.stdout", served.stdout or b"")
    write_bytes(OUT / "phases" / "api-start.stderr", served.stderr or b"")
    (OUT / "phases" / "api-start.exit").write_text(f"{served.returncode}\n", encoding="utf-8")
    if served.returncode != 0:
        raise CallError("api start failed")
    started.append(API_NAME)
    deadline = time.time() + 60
    while time.time() < deadline:
        logs = run(["docker", "logs", API_NAME])
        if b'"operation":"serve"' in (logs.stdout or b"") + (logs.stderr or b""):
            return
        time.sleep(1)
    raise CallError("serve did not become ready")


def call_http() -> dict:
    status, anon = exchange("GET", "/control/v1/builds/" + "b_" + ("0" * 32), None, False, "anon")
    body = {
        "allowance_id": ALLOWANCE_ID,
        "config_commit": COMMIT,
        "identity_revision": 1,
        "idempotency_key": IDEMPOTENCY_KEY,
        "llm_secret_ref": PROTOCOL_TOKEN,
        "project_id": PROJECT_ID,
        "source_ref": SOURCE_REF,
    }
    created_status, created = exchange("POST", "/control/v1/builds", body, True, "create")
    summary = {
        "anon_code": code_of(anon),
        "anon_status": status,
        "compiled_pack": created.get("compiled_pack") if isinstance(created, dict) else None,
        "create_code": code_of(created),
        "create_status": created_status,
        "published": created.get("published") if isinstance(created, dict) else None,
    }
    if created_status != 202 or not isinstance(created, dict) or "build_id" not in created:
        summary["terminal"] = "create_refused"
        return summary
    build_id = str(created["build_id"])
    summary["build_id"] = build_id
    for attempt in range(1, 46):
        got, document = exchange("GET", f"/control/v1/builds/{build_id}", None, True, f"get-{attempt:02d}")
        if isinstance(document, dict) and document.get("status") in {"failed", "canceled", "succeeded"}:
            summary["failure_code"] = document.get("failure_code")
            summary["get_status"] = got
            summary["job_status"] = document.get("status")
            summary["polls"] = attempt
            summary["stage"] = document.get("stage")
            summary["stages"] = document.get("stages")
            break
        time.sleep(1)
    else:
        summary["job_status"] = "timeout"
    pack_body = {"identity_revision": 1, "idempotency_key": "ch268sb71rolepack001", "policy_version": "oos/v1", "role": ROLE}
    pack_status, pack = exchange("POST", f"/control/v1/builds/{build_id}/role-pack", pack_body, True, "role-pack")
    summary["role_pack_code"] = code_of(pack)
    summary["role_pack_status"] = pack_status
    cards_status, cards = exchange("GET", f"/control/v1/builds/{build_id}/cards", None, True, "cards")
    summary["cards_code"] = code_of(cards)
    summary["cards_status"] = cards_status
    ready = None
    if isinstance(cards, dict):
        nested = cards.get("cards")
        if isinstance(nested, dict) and "ready" in nested:
            ready = nested["ready"]
        elif "ready" in cards:
            ready = cards["ready"]
    summary["cards_ready"] = ready
    return summary


def stage_of(stages: object, name: str) -> dict:
    if not isinstance(stages, list):
        return {}
    for item in stages:
        if isinstance(item, dict) and item.get("stage") == name:
            return item
    return {}


def cleanup(names: list[str]) -> None:
    for name in names:
        run(["docker", "stop", name])
        run(["docker", "rm", name])
    remaining = docker_text(["ps", "-aq", "--filter", "name=oo-dl01w1-40-sb"])
    write_json(
        OUT / "cleanup.json",
        {
            "containers_remaining": remaining.split() if remaining else [],
            "containers_removed": names,
            "foreign_pg": docker_text(["ps", "-a", "--filter", "id=9e49238c6b319521cb3f6ef1781208bd954ec3415552b056940a4b5d6842e9b6", "--format", "{{.ID}} {{.Status}}"]),
            "network_removed": False,
            "rust_container": docker_text(["ps", "-a", "--filter", "name=^oo-dl01w1-40-rust$", "--format", "{{.Names}} {{.Status}}"]),
            "volume_kept": VOLUME if volume_exists(VOLUME) else "",
            "volume_removed": False,
        },
    )


def save_logs(secrets: list[bytes]) -> None:
    for name, label in ((PG_NAME, "pg"), (API_NAME, "api")):
        logs = run(["docker", "logs", name])
        text = scrub(((logs.stdout or b"") + (logs.stderr or b"")).decode("utf-8", "replace"), secrets)
        path = OUT / "phases" / f"{label}.log"
        path.write_text(text if text.endswith("\n") or not text else text + "\n", encoding="utf-8")


def main() -> int:
    if (OUT / "http" / "create.json").exists() or volume_exists(VOLUME):
        return 2
    for name in (PG_NAME, API_NAME, SEAL_NAME):
        require_name_free(name)
    if docker_text(["network", "ls", "-q", "--filter", f"name=^{NET}$"]) and not volume_exists(VOLUME):
        run(["docker", "network", "rm", NET])
    if sha256_file(ELF) != ELF_SHA or sha256_file(PG_BUILDER) != PG_BUILDER_SHA or sha256_file(PIPELINE) != PIPELINE_SHA:
        raise SystemExit("pinned ELF hash drifted")
    record = binding_record()
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "phases").mkdir(parents=True, exist_ok=True)
    (OUT / "http").mkdir(parents=True, exist_ok=True)
    write_json(OUT / "resource-before.json", observe(record))
    bundle = write_bundle()
    seal = existing_seal()
    if seal is None:
        seal = import_seal()
    secrets: list[bytes] = []
    identity: Identity | None = None
    started: list[str] = []
    summary: dict = {
        "binding": record,
        "bundle_outside_oos": bundle["outside"],
        "catalog_import_native": "not_rerun",
        "compiled_pack": "not_published",
        "elf_sha256": ELF_SHA,
        "model_called": False,
        "pg_builder_sha256": PG_BUILDER_SHA,
        "pipeline_sha256": PIPELINE_SHA,
        "protocol": "oo.clickhouse.selfbuild-71/v1",
        "published": False,
        "ready": False,
        "seal": seal,
        "source_product_accepted": False,
    }
    exit_code = 1
    try:
        created = run([
            "docker", "network", "create",
            "--label", "oo.owner=dl01w1-40", "--label", "oo.task=DL01-W1-40",
            "--subnet", "10.40.18.0/24", "--gateway", "10.40.18.1",
            NET,
        ])
        write_bytes(OUT / "phases" / "network-create.stdout", created.stdout or b"")
        write_bytes(OUT / "phases" / "network-create.stderr", created.stderr or b"")
        (OUT / "phases" / "network-create.exit").write_text(f"{created.returncode}\n", encoding="utf-8")
        if created.returncode != 0:
            raise CallError("network create failed")
        inspected = run(["docker", "network", "inspect", NET])
        write_bytes(OUT / "phases" / "network.json", inspected.stdout or b"")
        make_certs(secrets)
        write_config()
        start_pg(started)
        write_json(OUT / "phases" / "pg-cgroup.json", cgroup(PG_NAME))
        identity = Identity()
        identity.start()
        identity.ready.wait(5)
        start_api(started)
        write_json(OUT / "phases" / "api-cgroup.json", cgroup(API_NAME))
        http_summary = call_http()
        summary["http"] = http_summary
        stages = http_summary.get("stages")
        s0 = stage_of(stages, "S0")
        s1 = stage_of(stages, "S1")
        s2 = stage_of(stages, "S2")
        summary["s0_detail"] = s0.get("detail_code")
        summary["s0_status"] = s0.get("status")
        summary["s1_detail"] = s1.get("detail_code")
        summary["s1_status"] = s1.get("status")
        summary["s2_detail"] = s2.get("detail_code")
        summary["s2_status"] = s2.get("status")
        matched = (
            seal.get("exit") == 1
            and seal.get("outcome") == "compiler_refused"
            and seal.get("model") == "not_called"
            and seal.get("pack") == "not_produced"
            and http_summary.get("create_status") == 202
            and s0.get("status") == "succeeded"
            and s1.get("status") == "succeeded"
            and s2.get("status") == "failed"
            and s2.get("detail_code") == "overlay.malformed"
            and http_summary.get("job_status") == "failed"
            and http_summary.get("compiled_pack") == "not_published"
            and http_summary.get("cards_ready") is not True
        )
        summary["outcome"] = "s2_bytea_closed" if matched else "unexpected"
        summary["ok"] = matched
        exit_code = 0 if matched else 1
    except Exception as exc:
        summary["error"] = str(exc)
        summary["ok"] = False
        summary["outcome"] = "failed"
        exit_code = 1
    finally:
        if identity is not None:
            identity.close()
        if started:
            save_logs(secrets)
        cleanup(started)
        scrub_outputs(secrets)
        hits = leak(secrets)
        summary["password_leaks"] = hits
        if hits:
            summary["ok"] = False
            summary["outcome"] = "leak"
            exit_code = 1
        write_json(OUT / "protocol-summary.json", summary)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
