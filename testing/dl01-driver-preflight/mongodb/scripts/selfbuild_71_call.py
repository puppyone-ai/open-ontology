#!/usr/bin/env python3
"""Call the pinned DL01-W1-71 ELF with the real oo_w141.shipments catalog.

The sealed bundle keeps the approved key `_id`. ident_ok rejects a leading
underscore, so S0 is expected to fail closed as sealed.malformed. This script
does not rename the key, does not call the native loader, and does not start
the retained mongod. Secrets stay under /tmp and are not written to evidence.
"""

from __future__ import annotations

import hashlib
import json
import os
import socket
import ssl
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
EVIDENCE = ROOT / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT = EVIDENCE / "raw/selfbuild-71"
BUNDLE = OUT / "bundle"
CACHE = Path("/tmp/oo-dl01w1-41-selfbuild")
ELF = Path("/Users/redux/.cache/oo-dl01w1-71/selfbuild-control-api")
ELF_SHA = "dce12ddd7243e8433f54dd945c24051545411dda0e72079a39a316bc592893e9"
RUST_IMAGE = "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE = "sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
PG_ENTRY = ROOT / "tools/selfbuild-control-api/linux/pg-entry.sh"
COMMIT = "71" * 32
API_HOST = "127.0.0.1"
API_PORT = 18553
IDENTITY_PORT = 18551
ORIGIN = f"https://{API_HOST}:{API_PORT}"
IDENTITY_ORIGIN = f"https://host.docker.internal:{IDENTITY_PORT}"
NET = "oo-dl01w1-41-sb-net"
PG_NAME = "oo-dl01w1-41-sb-pg"
API_NAME = "oo-dl01w1-41-sb-api"
VOLUME = "oo-dl01w1-41-sb-pgdata"
COOKIE = "a" * 43
CSRF = "e" * 43
PROJECT_ID = "ledger"
SOURCE_REF = "oo_w141"
ALLOWANCE_ID = "read_budget_16"
LLM_SECRET_REF = "secret://llm/not-present"
IDEMPOTENCY_KEY = "w141sb71create0001"
APPROVED_FIELDS = [
    "_id",
    "code",
    "flag",
    "units",
    "price",
    "created",
    "uid",
    "blob",
    "blob_presence",
    "weight",
    "warehouse",
    "whCode",
    "lane",
    "memo",
    "memo_presence",
    "marks",
    "marks_presence",
    "pack",
    "pack_presence",
]
OOS_DATA_TYPES = {
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


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(value, indent=2, sort_keys=True) + "\n"
    path.write_text(text, encoding="utf-8")


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def ident_ok(value: str) -> bool:
    if not value or len(value) > 63 or not ("a" <= value[0] <= "z"):
        return False
    return all(("a" <= char <= "z") or char.isdigit() or char == "_" for char in value)


def run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, cwd=cwd, capture_output=True)


def require_name_free(name: str) -> None:
    found = subprocess.check_output(["docker", "ps", "-aq", "--filter", f"name=^{name}$"], text=True).strip()
    if found:
        raise SystemExit(2)


def volume_exists(name: str) -> bool:
    found = subprocess.check_output(["docker", "volume", "ls", "-q", "--filter", f"name=^{name}$"], text=True).strip()
    return bool(found)


def docker_text(args: list[str]) -> str:
    result = run(["docker", *args])
    return (result.stdout or b"").decode("utf-8", "replace").strip()


def scrub(text: str, secrets: list[bytes]) -> str:
    out = text
    for secret in secrets:
        if secret:
            out = out.replace(secret.decode("utf-8", "replace"), "[redacted]")
    return out


def load_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def cite(native: Path) -> dict:
    gen1 = native / "gen1"
    gen2 = native / "gen2"
    audit = EVIDENCE / "raw/c5-cycle-79/audit-profiled-named.json"
    profile = native / "profile/profile.json"
    files = {
        "gen1_catalog": gen1 / "catalog.json",
        "gen1_registry": gen1 / "registry.json",
        "gen1_receipt": gen1 / "receipt.json",
        "gen1_arrow": gen1 / "rows.arrow",
        "gen2_catalog": gen2 / "catalog.json",
        "gen2_registry": gen2 / "registry.json",
        "gen2_receipt": gen2 / "receipt.json",
        "gen2_arrow": gen2 / "rows.arrow",
        "audit": audit,
        "profile": profile,
    }
    hashes = {name: sha256_file(path) for name, path in files.items()}
    receipt = load_json(files["gen1_receipt"])
    registry_text = files["gen1_registry"].read_text(encoding="utf-8")
    receipt_text = files["gen1_receipt"].read_text(encoding="utf-8")
    gen2_receipt = load_json(files["gen2_receipt"])
    gen2_registry_text = files["gen2_registry"].read_text(encoding="utf-8")
    audit_doc = load_json(audit)
    profile_doc = load_json(profile)
    return {
        "hashes": hashes,
        "gen1_receipt_run": receipt.get("run_id"),
        "gen1_registry_sha_inside_receipt": receipt.get("registry_sha256"),
        "gen1_registry_file_sha": hashes["gen1_registry"],
        "gen1_receipt_contains_registry_sha": receipt.get("registry_sha256") == hashes["gen1_registry"],
        "gen1_registry_contains_receipt_sha": hashes["gen1_receipt"] in registry_text,
        "gen2_receipt_contains_registry_sha": gen2_receipt.get("registry_sha256") == hashes["gen2_registry"],
        "gen2_registry_contains_receipt_sha": hashes["gen2_receipt"] in gen2_registry_text,
        "audit_reader_dml": audit_doc.get("reader_dml"),
        "profile_ready": profile_doc.get("ready"),
        "profile_model": profile_doc.get("model"),
        "profile_outcome": profile_doc.get("outcome"),
        "catalog_identity_hash": profile_doc.get("catalog_identity_hash"),
        "document_identity_hash": profile_doc.get("document_identity_hash"),
        "driver_digest": receipt.get("driver_digest"),
    }


def counts() -> dict:
    queries = {
        "platform_gen2": (
            "oo-dl01w1-41-pg",
            "oo_w141_admin",
            "select count(*) from _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898",
        ),
        "platform_gen1": (
            "oo-dl01w1-41-pg",
            "oo_w141_admin",
            "select count(*) from _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254",
        ),
        "isolated_gen1": (
            "oo-dl01w1-41-pg-native",
            "postgres",
            "select count(*) from _oo.pt_44103bb2e2cc9a13bbfe3077807b25900ca663bcd39106c1b7d511b31f01",
        ),
        "isolated_gen2": (
            "oo-dl01w1-41-pg-native",
            "postgres",
            "select count(*) from _oo.pt_d1eed4b241abd54a6a1749cb83000d6d0423cc3584f318db35cdb192774c",
        ),
    }
    out = {}
    for name, (container, user, sql) in queries.items():
        result = run(["docker", "exec", container, "psql", "-U", user, "-d", "oo_w141", "-At", "-c", sql])
        out[name] = {
            "exit": result.returncode,
            "stdout": (result.stdout or b"").decode("utf-8", "replace").strip(),
            "stderr": (result.stderr or b"").decode("utf-8", "replace").strip(),
        }
    return out


def loader_blob() -> dict:
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    blob = subprocess.check_output(
        ["git", "rev-parse", "HEAD:crates/oo-pipeline/src/native_update.rs"], cwd=ROOT, text=True
    ).strip()
    text = (ROOT / "crates/oo-pipeline/src/native_update.rs").read_text(encoding="utf-8")
    return {
        "head": head,
        "blob": blob,
        "trusted_capture_present": "pub fn trusted_capture(" in text,
        "capture_matched_sets_trusted_true": '"trusted": true' in text and '"entry": "capture_receipt_file"' in text,
        "applied_pos_discarded": "let _ = plan.applied_pos_request;" in text,
        "jobs_lease_null_inserted": 'object.insert("jobs_lease_receipt".to_string(), Value::Null);' in text,
        "verify_inputs_called_from_trusted_capture": "verify_inputs" in text[text.find("pub fn trusted_capture(") : text.find("fn historical_admission(")],
    }


def write_bundle(cited: dict) -> dict:
    catalog = {
        "tables": [
            {
                "columns": [
                    {"data_type": "varchar", "document": False, "name": "_id", "nullable": False, "oid": 1043, "typmod": -1},
                    {"data_type": "jsonb", "document": True, "name": "doc", "oid": 3802, "typmod": -1},
                ],
                "schema": "oo_w141",
                "table": "shipments",
            }
        ]
    }
    profile = {
        "catalog_identity_hash": cited["catalog_identity_hash"],
        "cited_profile_path": "raw/native-2e0bd/profile/profile.json",
        "cited_profile_sha256": cited["hashes"]["profile"],
        "column": "doc",
        "compiler": "compile_jsonb_document_profile",
        "contract": "oo.selfbuild-control-api/sealed-profile/v1",
        "dataset": "oo_w141.shipments",
        "document_identity_hash": cited["document_identity_hash"],
        "document_oid": 3802,
        "document_typmod": -1,
        "executed": False,
        "model": "not_called",
        "outcome": cited["profile_outcome"],
        "pack": None,
        "ready": False,
        "weight": {"$numberDouble": "41.25"},
    }
    read_logic = {
        "audit_path": "raw/c5-cycle-79/audit-profiled-named.json",
        "audit_sha256": cited["hashes"]["audit"],
        "budget_rows": 16,
        "contract": "oo.selfbuild-control-api/sealed-read-logic/v1",
        "driver_digest": cited["driver_digest"],
        "executed": False,
        "reader_dml": cited["audit_reader_dml"],
        "role": "read",
    }
    source_semantics = {
        "approved_field_count": 19,
        "approved_fields": APPROVED_FIELDS,
        "consistency_grade": None,
        "contract": "oo.selfbuild-control-api/sealed-source-semantics/v1",
        "dataset": "oo_w141.shipments",
        "delete_detection_label": "c5_diff reconcile capture config",
        "executed": False,
        "fulldoc_complete": False,
        "generations": [
            {
                "arrow_sha256": cited["hashes"]["gen1_arrow"],
                "catalog_sha256": cited["hashes"]["gen1_catalog"],
                "cell_diff": 0,
                "receipt_sha256": cited["hashes"]["gen1_receipt"],
                "registry_sha256": cited["hashes"]["gen1_registry"],
                "sealed_rows": 16,
                "source_bound_from_loader": True,
                "source_generation": 1,
                "trusted": False,
            },
            {
                "arrow_sha256": cited["hashes"]["gen2_arrow"],
                "catalog_sha256": cited["hashes"]["gen2_catalog"],
                "cell_diff": 0,
                "receipt_sha256": cited["hashes"]["gen2_receipt"],
                "registry_sha256": cited["hashes"]["gen2_registry"],
                "sealed_rows": 14,
                "source_bound_from_loader": True,
                "source_generation": 2,
                "trusted": False,
            },
        ],
        "gen1_receipt_contains_registry_sha256": cited["gen1_receipt_contains_registry_sha"],
        "gen1_registry_contains_receipt_sha256": cited["gen1_registry_contains_receipt_sha"],
        "gen2_receipt_contains_registry_sha256": cited["gen2_receipt_contains_registry_sha"],
        "gen2_registry_contains_receipt_sha256": cited["gen2_registry_contains_receipt_sha"],
        "hash_direction": "receipt_cites_registry_sha256",
        "hidden_note": "unclassified",
        "read_budget": 16,
        "source_bound_trusted": False,
        "source_id": "oo_w141",
        "trust_review": "65abb6c7f9ee8af20ecd50de5a6a2e66f15750b0",
        "trust_review_result": "REQUEST_CHANGES",
    }
    rules = {
        "approved_fields": APPROVED_FIELDS,
        "columns": [
            {
                "confidence": "unknown",
                "id": "oo_w141.shipments.doc",
                "pii": "unknown",
                "semantic_type": "json",
            },
            {
                "confidence": "unknown",
                "id": "oo_w141.shipments._id",
                "pii": "unknown",
                "semantic_type": "identifier",
            },
        ],
        "contract": "oo.selfbuild-control-api/sealed-rules/v1",
        "hidden_note": "unclassified",
        "rules_auto_accept": "none",
    }
    suggestions = {
        "certified": False,
        "contract": "oo.selfbuild-control-api/sealed-suggestions/v1",
        "merged": False,
        "ready": False,
    }
    generation = {
        "bearer": "absent",
        "contract": "oo.selfbuild-control-api/sealed-generation/v1",
        "llm_calls": 0,
        "model_called": False,
        "outcome": "model_unauthorized",
    }
    roles = {
        "contract": "oo.selfbuild-control-api/sealed-roles/v1",
        "roles": {"oo_w141_reader": ["oo_w141.shipments._id", "oo_w141.shipments.doc"]},
    }
    bodies = {
        "catalog.json": catalog,
        "profile.json": profile,
        "read_logic.json": read_logic,
        "source_semantics.json": source_semantics,
        "rules.json": rules,
        "suggestions.json": suggestions,
        "generation.json": generation,
        "roles.json": roles,
    }
    BUNDLE.mkdir(parents=True, exist_ok=True)
    encoded = {}
    for name, value in bodies.items():
        data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
        write_bytes(BUNDLE / name, data)
        encoded[name] = sha256_bytes(data)
    manifest = {
        "contract": "oo.selfbuild-control-api/sealed-bundle/v1",
        "files": {name: encoded[name] for name in BUNDLE_FILES},
    }
    write_json(BUNDLE / "manifest.json", manifest)
    column_names = [column["name"] for column in catalog["tables"][0]["columns"]]
    preflight = {
        "protocol": "oo.mongodb-preview.selfbuild-ident-preflight/v1",
        "ident_rule": "lowercase ASCII start, then lowercase, digit, or underscore, length 1-63",
        "schema_ident_ok": ident_ok("oo_w141"),
        "table_ident_ok": ident_ok("shipments"),
        "column_ident": {name: ident_ok(name) for name in column_names},
        "failing_column": "_id",
        "failing_reason": "leading underscore is not a lowercase letter",
        "varchar_in_oos_data_type_enum": "varchar" in OOS_DATA_TYPES,
        "jsonb_in_oos_data_type_enum": "jsonb" in OOS_DATA_TYPES,
        "oos_data_types": sorted(OOS_DATA_TYPES),
        "renamed": False,
        "source_protocol_set": False,
        "source_protocol_reason": "server source_protocol accepts only the PostgreSQL capture protocols",
        "expected_api_detail": "sealed.malformed",
    }
    write_json(OUT / "ident-preflight.json", preflight)
    identity = {
        "protocol": "oo.mongodb-preview.runtime-identity/v1",
        "source_acceptance": False,
        "source_bound_trusted": False,
        "approved_fields": APPROVED_FIELDS,
        "hidden_note": "unclassified",
        "citations": cited,
        "control_plane": {
            "allowance_id": ALLOWANCE_ID,
            "config_commit": COMMIT,
            "config_commit_note": "control-plane test pin from the 71 recipe COMMIT constant, not a Mongo git commit",
            "database": "oo_w141_sb",
            "ledger_user": "oo_selfbuild_api_admin",
            "project_id": PROJECT_ID,
            "project_id_note": "the stock identity envelope grants project.admin on ledger; source tables stay oo_w141.shipments",
            "source_ref": SOURCE_REF,
        },
        "isolated_target": {
            "container": "oo-dl01w1-41-pg-native",
            "database": "oo_w141",
            "host": "127.0.0.1",
            "owner": "41a14141-1414-4414-8414-000000000041",
            "password_in_evidence": False,
            "port": 5432,
            "principal": "postgres",
            "shard": "shard_w141",
            "ssl": "disable",
        },
        "platform_generation_2_run": "ad8b3022-7730-4d5c-9218-ca68bd26b561",
        "mongo_started": False,
    }
    write_json(OUT / "identity.json", identity)
    return {"file_sha256": encoded, "preflight": preflight}


def make_certs(secrets: list[bytes], pg_san: str) -> None:
    ca = CACHE / "tls/ca"
    ca.mkdir(parents=True, exist_ok=True)
    subprocess.check_call(
        [
            "openssl",
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(ca / "ca.key"),
            "-out",
            str(ca / "ca.crt"),
            "-days",
            "30",
            "-subj",
            "/CN=oo-dl01w1-41-sb-ca",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-addext",
            "subjectKeyIdentifier=hash",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    def sign(name: str, ext: Path) -> None:
        folder = CACHE / "tls" / name
        folder.mkdir(parents=True, exist_ok=True)
        subprocess.check_call(
            [
                "openssl",
                "req",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                str(folder / "server.key"),
                "-out",
                str(folder / "server.csr"),
                "-subj",
                f"/CN={name}",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        subprocess.check_call(
            [
                "openssl",
                "x509",
                "-req",
                "-in",
                str(folder / "server.csr"),
                "-CA",
                str(ca / "ca.crt"),
                "-CAkey",
                str(ca / "ca.key"),
                "-CAcreateserial",
                "-out",
                str(folder / "server.crt"),
                "-days",
                "30",
                "-extfile",
                str(ext),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        chain = (folder / "server.crt").read_bytes() + b"\n" + (ca / "ca.crt").read_bytes()
        (folder / "server.crt").write_bytes(chain)
        (folder / "ca.crt").write_bytes((ca / "ca.crt").read_bytes())
        (folder / "server.csr").unlink(missing_ok=True)

    (CACHE / "tls/pg.ext").write_text(
        "basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\nauthorityKeyIdentifier=keyid,issuer\n"
        f"subjectAltName={pg_san}\n",
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
    password = subprocess.check_output(["openssl", "rand", "-base64", "24"])
    source_password = subprocess.check_output(["openssl", "rand", "-base64", "24"])
    (secret_dir / "pg.password").write_bytes(password)
    (secret_dir / "source.password").write_bytes(source_password)
    os.chmod(secret_dir / "pg.password", 0o600)
    os.chmod(secret_dir / "source.password", 0o600)
    os.chmod(ca / "ca.key", 0o600)
    secrets.extend([password.strip(), source_password.strip()])
    modes = {
        "pg_password_mode": oct(os.stat(secret_dir / "pg.password").st_mode & 0o777),
        "source_password_mode": oct(os.stat(secret_dir / "source.password").st_mode & 0o777),
        "pg_password_path": str(secret_dir / "pg.password"),
        "source_password_path": str(secret_dir / "source.password"),
        "values_in_evidence": False,
    }
    write_json(OUT / "phases" / "secret-paths.json", modes)


def write_config(pg_host: str) -> None:
    (CACHE / "work").mkdir(parents=True, exist_ok=True)
    (CACHE / "bin").mkdir(parents=True, exist_ok=True)
    write_json(CACHE / "role-policy.json", {"active": False, "published": False})
    endpoint = {
        "ca_file": str(CACHE / "tls/ca/ca.crt"),
        "database": "oo_w141_sb",
        "host": pg_host,
        "password_file": str(CACHE / "secrets/pg.password"),
        "port": 5432,
        "server_name": pg_host,
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
        "listen": {
            "bind": "0.0.0.0:8443",
            "cert_file": str(CACHE / "tls/api/server.crt"),
            "key_file": str(CACHE / "tls/api/server.key"),
        },
        "model_cache_dir": "",
        "postgres": endpoint,
        "principals": [],
        "projects": [
            {
                "allowance_id": ALLOWANCE_ID,
                "config_commit": COMMIT,
                "hidden_columns": [],
                "llm": {
                    "bearer_file": str(CACHE / "llm-bearer-absent"),
                    "model_id": "grok-4.7",
                    "origin": "http://127.0.0.1:8317",
                },
                "llm_secret_ref": LLM_SECRET_REF,
                "project_id": PROJECT_ID,
                "role_policy_file": str(CACHE / "role-policy.json"),
                "rules_auto_accept": "none",
                "sample_policy": "none",
                "sealed_bundle": str(BUNDLE),
                "source": {
                    "ca_file": str(CACHE / "tls/ca/ca.crt"),
                    "database": "oo_w141",
                    "host": "unused.invalid",
                    "password_file": str(CACHE / "secrets/source.password"),
                    "port": 27017,
                    "tables": [{"schema": "oo_w141", "table": "shipments"}],
                    "username": "oo_w141_reader",
                },
                "source_ref": SOURCE_REF,
                "source_secret_ref": "secret://source/not-opened",
                "visible_columns": [],
            }
        ],
        "work_dir": str(CACHE / "work"),
    }
    write_json(CACHE / "config.json", config)
    public = json.loads(json.dumps(config))
    public["postgres"]["password_file"] = "secret-path-not-copied"
    public["ledger"]["password_file"] = "secret-path-not-copied"
    public["projects"][0]["source"]["password_file"] = "secret-path-not-copied"
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
                    body = b'{"error":{"code":"unauthenticated"}}'
                    tls.sendall(http_response(401, "Unauthorized", body))
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
        "issuer": "https://oo-dl01w1-41.example",
        "operation": "getIdentityCurrent",
        "owner_certification": "not_issued",
        "projects": [{"capabilities": ["project.admin"], "project_id": PROJECT_ID, "role": "admin"}],
        "revision": 1,
        "session": {
            "absolute_expires_at": "2027-01-02T00:00:00Z",
            "idle_expires_at": "2027-01-01T00:00:00Z",
            "revoked": False,
        },
        "session_class": "management_cookie",
        "subject": "user-admin",
    }


def http_response(status: int, reason: str, body: bytes) -> bytes:
    head = (
        f"HTTP/1.1 {status} {reason}\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(body)}\r\n"
        "Connection: close\r\n\r\n"
    )
    return head.encode("ascii") + body


def parse_http(data: bytes) -> tuple[int, bytes]:
    header, _, body = data.partition(b"\r\n\r\n")
    text = header.decode("iso-8859-1", "replace")
    status = int(text.split("\r\n", 1)[0].split(" ")[1])
    if "transfer-encoding: chunked" in text.lower():
        out = b""
        rest = body
        while rest:
            line, _, rest = rest.partition(b"\r\n")
            size = int(line.split(b";", 1)[0] or b"0", 16)
            if size == 0:
                break
            out += rest[:size]
            rest = rest[size + 2 :]
        body = out
    else:
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
            lowered = header.lower()
            if b"content-length:" in lowered:
                length = 0
                for line in header.split(b"\r\n"):
                    if line.lower().startswith(b"content-length:"):
                        length = int(line.split(b":", 1)[1].strip())
                if len(rest) >= length:
                    break
            elif b"transfer-encoding: chunked" in lowered and b"\r\n0\r\n" in rest:
                break
        if not chunk:
            break
    sock.close()
    status, response = parse_http(data)
    record = {
        "request_sha256": sha256_bytes(request),
        "response": response.decode("utf-8", "replace"),
        "response_sha256": sha256_bytes(response),
        "status": status,
    }
    write_json(OUT / "http" / f"{name}.json", record)
    if not response:
        return status, {}
    try:
        return status, json.loads(response)
    except json.JSONDecodeError:
        return status, {"raw": response.decode("utf-8", "replace")}


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
    for path in ("/sys/fs/cgroup/cpu.max", "/sys/fs/cgroup/cpu/cpu.cfs_quota_us"):
        result = run(["docker", "exec", name, "cat", path])
        inspect[path] = {
            "exit": result.returncode,
            "stdout": (result.stdout or b"").decode("utf-8", "replace").strip(),
        }
    return inspect


def start_pg(started: list[str], network_mode: str) -> None:
    cmd = [
        "docker",
        "run",
        "-d",
        "--name",
        PG_NAME,
        "--pull",
        "never",
        "--platform",
        "linux/arm64",
        "--hostname",
        PG_NAME,
    ]
    if network_mode == "custom":
        cmd.extend(["--network", NET])
    else:
        cmd.extend(
            [
                "--add-host",
                "host.docker.internal:host-gateway",
                "-p",
                f"127.0.0.1:{API_PORT}:8443",
            ]
        )
    cmd.extend([
        "--memory",
        "1g",
        "--memory-swap",
        "1g",
        "--label",
        "oo.project=open-ontology",
        "--label",
        "oo.owner=dl01w1-41",
        "--label",
        "oo.task=DL01-W1-41",
        "--label",
        "oo.kind=postgres",
        "-e",
        "POSTGRES_USER=oo_selfbuild_api_admin",
        "-e",
        "POSTGRES_DB=oo_w141_sb",
        "-e",
        "POSTGRES_PASSWORD_FILE=/secrets/pg.password",
        "-e",
        "POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256 --auth-local=scram-sha-256",
        "-v",
        f"{CACHE}/secrets:/secrets:ro",
        "-v",
        f"{CACHE}/tls/pg:/tls-in:ro",
        "-v",
        f"{VOLUME}:/var/lib/postgresql/data",
        "-v",
        f"{PG_ENTRY}:/pg-entry.sh:ro",
        "--entrypoint",
        "/bin/sh",
        PG_IMAGE,
        "/pg-entry.sh",
    ])
    result = run(cmd)
    write_bytes(OUT / "phases" / "pg-start.stdout", result.stdout or b"")
    write_bytes(OUT / "phases" / "pg-start.stderr", result.stderr or b"")
    (OUT / "phases" / "pg-start.exit").write_text(f"{result.returncode}\n", encoding="utf-8")
    if result.returncode != 0:
        raise CallError("pg start failed")
    started.append(PG_NAME)
    for i in range(1, 61):
        ready = run(["docker", "exec", PG_NAME, "pg_isready", "-U", "oo_selfbuild_api_admin", "-d", "oo_w141_sb"])
        if ready.returncode == 0:
            (OUT / "phases" / "pg-ready-tries.txt").write_text(f"{i}\n", encoding="utf-8")
            return
        time.sleep(2)
    raise CallError("pg did not become ready")


def api_network(network_mode: str) -> list[str]:
    if network_mode == "custom":
        return ["--network", NET, "--add-host", "host.docker.internal:host-gateway", "-p", f"127.0.0.1:{API_PORT}:8443"]
    return ["--network", f"container:{PG_NAME}"]


def start_api(started: list[str], network_mode: str) -> None:
    install = [
        "docker",
        "run",
        "--rm",
        "--pull",
        "never",
        "--platform",
        "linux/arm64",
        "--network",
        NET if network_mode == "custom" else f"container:{PG_NAME}",
        "-v",
        f"{CACHE}:{CACHE}",
        "-v",
        f"{BUNDLE}:{BUNDLE}:ro",
        "-v",
        f"{ELF}:/opt/selfbuild-control-api:ro",
        RUST_IMAGE,
        "/opt/selfbuild-control-api",
        "install",
        "--config",
        str(CACHE / "config.json"),
    ]
    installed = run(install)
    write_bytes(OUT / "phases" / "install.stdout", installed.stdout or b"")
    write_bytes(OUT / "phases" / "install.stderr", installed.stderr or b"")
    (OUT / "phases" / "install.exit").write_text(f"{installed.returncode}\n", encoding="utf-8")
    if installed.returncode != 0:
        raise CallError("install failed")
    serve = [
        "docker",
        "run",
        "-d",
        "--name",
        API_NAME,
        "--pull",
        "never",
        "--platform",
        "linux/arm64",
        *api_network(network_mode),
        "--memory",
        "1g",
        "--memory-swap",
        "1g",
        "--label",
        "oo.project=open-ontology",
        "--label",
        "oo.owner=dl01w1-41",
        "--label",
        "oo.task=DL01-W1-41",
        "--label",
        "oo.kind=control-api",
        "-v",
        f"{CACHE}:{CACHE}",
        "-v",
        f"{BUNDLE}:{BUNDLE}:ro",
        "-v",
        f"{ELF}:/opt/selfbuild-control-api:ro",
        RUST_IMAGE,
        "/opt/selfbuild-control-api",
        "serve",
        "--config",
        str(CACHE / "config.json"),
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
        "llm_secret_ref": LLM_SECRET_REF,
        "project_id": PROJECT_ID,
        "source_ref": SOURCE_REF,
    }
    write_json(OUT / "create.request.json", body)
    created_status, created = exchange("POST", "/control/v1/builds", body, True, "create")
    summary = {
        "anon_status": status,
        "anon_code": code_of(anon),
        "anon_published": anon.get("published") if isinstance(anon, dict) else None,
        "anon_compiled_pack": anon.get("compiled_pack") if isinstance(anon, dict) else None,
        "create_status": created_status,
        "create_code": code_of(created),
    }
    if created_status != 202 or not isinstance(created, dict) or "build_id" not in created:
        summary["terminal"] = "create_refused"
        return summary
    build_id = created["build_id"]
    summary["build_id"] = build_id
    last: object = created
    for attempt in range(1, 46):
        got, document = exchange("GET", f"/control/v1/builds/{build_id}", None, True, f"get-{attempt:02d}")
        last = document
        if isinstance(document, dict) and document.get("status") in {"failed", "canceled", "succeeded"}:
            summary["get_status"] = got
            summary["job_status"] = document.get("status")
            summary["job_detail"] = document.get("detail_code")
            summary["failure_code"] = document.get("failure_code")
            summary["stage"] = document.get("stage")
            summary["stages"] = document.get("stages")
            summary["polls"] = attempt
            break
        time.sleep(1)
    else:
        summary["get_status"] = None
        summary["job_status"] = "timeout"
        summary["last"] = last
    cards_status, cards = exchange("GET", f"/control/v1/builds/{build_id}/cards", None, True, "cards")
    summary["cards_status"] = cards_status
    summary["cards_code"] = code_of(cards)
    ready = None
    if isinstance(cards, dict):
        nested = cards.get("cards")
        if isinstance(nested, dict) and "ready" in nested:
            ready = nested["ready"]
        elif "ready" in cards:
            ready = cards["ready"]
    summary["cards_ready"] = ready
    summary["published"] = created.get("published")
    summary["compiled_pack"] = created.get("compiled_pack")
    return summary


def cleanup(names: list[str], secrets: list[bytes]) -> None:
    for name in names:
        run(["docker", "stop", name])
        run(["docker", "rm", name])
    remaining = subprocess.check_output(["docker", "ps", "-aq", "--filter", "name=oo-dl01w1-41-sb"], text=True).strip()
    volume = subprocess.check_output(["docker", "volume", "ls", "-q", "--filter", f"name=^{VOLUME}$"], text=True).strip()
    network = subprocess.check_output(["docker", "network", "ls", "-q", "--filter", f"name=^{NET}$"], text=True).strip()
    write_json(
        OUT / "cleanup.json",
        {
            "containers_removed": names,
            "containers_remaining": remaining.split() if remaining else [],
            "network_left": NET if network else "",
            "volume_left": VOLUME if volume else "",
            "volume_removed": False,
            "network_pruned": False,
            "mongo_started": False,
            "platform_ddl": 0,
        },
    )


def save_logs(secrets: list[bytes]) -> None:
    for name, label in ((PG_NAME, "pg"), (API_NAME, "api")):
        logs = run(["docker", "logs", name])
        text = scrub(((logs.stdout or b"") + (logs.stderr or b"")).decode("utf-8", "replace"), secrets)
        (OUT / "phases" / f"{label}.log").write_text(text if text.endswith("\n") or not text else text + "\n", encoding="utf-8")


def main() -> int:
    if (OUT / "http" / "create.json").exists() or volume_exists(VOLUME):
        return 2
    for name in (PG_NAME, API_NAME):
        require_name_free(name)
    if sha256_file(ELF) != ELF_SHA:
        raise SystemExit("pinned ELF hash drifted")
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "phases").mkdir(parents=True, exist_ok=True)
    (OUT / "http").mkdir(parents=True, exist_ok=True)
    cited = cite(EVIDENCE / "raw/native-2e0bd")
    bundle = write_bundle(cited)
    observed = {
        "protocol": "oo.mongodb-preview.selfbuild-resource-before/v1",
        "token_files": len(list(Path("/tmp/oo-customer-demo-20261007/llm-token").glob("*"))),
        "counts": counts(),
        "loader": loader_blob(),
        "docker": subprocess.check_output(
            ["docker", "ps", "-a", "--filter", "name=oo-dl01w1-41", "--format", "{{.Names}} {{.Status}}"],
            text=True,
        ),
        "elf_sha256": sha256_file(ELF),
        "mongo_started": False,
        "model_called": False,
    }
    write_json(OUT / "resource-before.json", observed)
    secrets: list[bytes] = []
    identity: Identity | None = None
    started: list[str] = []
    summary: dict = {
        "protocol": "oo.mongodb-preview.selfbuild-71/v1",
        "source_acceptance": False,
        "model_called": False,
        "ready": False,
        "fulldoc_complete": False,
        "published": False,
        "compiled_pack": "not_published",
        "elf_sha256": ELF_SHA,
        "bundle_files": bundle["file_sha256"],
        "ident_preflight": bundle["preflight"],
    }
    exit_code = 1
    try:
        created = run(["docker", "network", "create", NET])
        write_bytes(OUT / "phases" / "network-create.stdout", created.stdout or b"")
        write_bytes(OUT / "phases" / "network-create.stderr", created.stderr or b"")
        (OUT / "phases" / "network-create.exit").write_text(f"{created.returncode}\n", encoding="utf-8")
        network_mode = "custom" if created.returncode == 0 else "bridge"
        summary["network_mode"] = network_mode
        summary["network_create_exit"] = created.returncode
        if created.returncode != 0:
            summary["network_create_error"] = (created.stderr or b"").decode("utf-8", "replace").strip()
        pg_host = PG_NAME if network_mode == "custom" else "127.0.0.1"
        pg_san = f"DNS:{PG_NAME}" if network_mode == "custom" else "IP:127.0.0.1"
        make_certs(secrets, pg_san)
        write_config(pg_host)
        start_pg(started, network_mode)
        write_json(OUT / "phases" / "pg-cgroup.json", cgroup(PG_NAME))
        identity = Identity()
        identity.start()
        identity.ready.wait(5)
        start_api(started, network_mode)
        write_json(OUT / "phases" / "api-cgroup.json", cgroup(API_NAME))
        http_summary = call_http()
        summary["http"] = http_summary
        stages = http_summary.get("stages") or []
        s0 = next((item for item in stages if isinstance(item, dict) and item.get("stage") == "S0"), {})
        summary["s0_status"] = s0.get("status")
        summary["s0_detail"] = s0.get("detail_code")
        summary["job_status"] = http_summary.get("job_status")
        summary["job_detail"] = http_summary.get("job_detail")
        matched = (
            http_summary.get("create_status") == 202
            and http_summary.get("job_status") == "failed"
            and s0.get("detail_code") == "sealed.malformed"
        )
        summary["outcome"] = "sealed.malformed" if matched else "unexpected"
        summary["ok"] = matched
        exit_code = 0 if matched else 1
    except Exception as exc:
        summary["ok"] = False
        summary["outcome"] = "failed"
        summary["error"] = str(exc)
        exit_code = 1
    finally:
        if identity is not None:
            identity.close()
        if started:
            save_logs(secrets)
        cleanup(started, secrets)
        write_json(OUT / "protocol-summary.json", summary)
        write_json(
            OUT / "resource.json",
            {
                "protocol": "oo.mongodb-preview.selfbuild-resource/v1",
                "mongo_container": "oo-dl01w1-41-mongodb",
                "mongo_started": False,
                "platform_pg": "oo-dl01w1-41-pg",
                "platform_ddl": 0,
                "isolated_pg_left_running": "oo-dl01w1-41-pg-native",
                "control_pg_removed": PG_NAME,
                "control_api_removed": API_NAME,
                "volume_kept": VOLUME if volume_exists(VOLUME) else "",
                "network_kept": NET
                if subprocess.check_output(["docker", "network", "ls", "-q", "--filter", f"name=^{NET}$"], text=True).strip()
                else "",
                "model_called": False,
                "source_acceptance": False,
                "token_files": len(list(Path("/tmp/oo-customer-demo-20261007/llm-token").glob("*"))),
            },
        )
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
