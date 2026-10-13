#!/usr/bin/env python3
"""Call catalog-import-native against the generation-1 capture receipt.

The fixed loader checks the receipt and the binding registry before it opens
PostgreSQL. This probe records that refusal. It does not install a catalog,
does not write a pack, and does not copy a secret into the evidence tree.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology")
RUN = ROOT / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-16-native"
OUT = ROOT / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-17-sealed-bind"
RECEIPT = RUN / "loads/gen1/receipt.json"
CATALOG = RUN / "identity/catalog.json"
STUB = RUN / "identity/registry.json"
REPORT = Path("/tmp/oo-dl01w1-40-bind/inputs/report.json")
CONFIG = Path("/tmp/oo-dl01w1-40-bind/inputs/config.json")
PIPELINE = Path("/tmp/oo-u06-194/debug/oo-pipeline")
ADMISSION = Path("/tmp/oo-dl01w1-40-bind/admission/oo-source-admission")
IMAGE = "rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
OWNER = (RUN / "identity/owner.txt").read_text().strip()
PASSWORD = "/tmp/oo-dl01w1-40-native/pg.password"
CA = "/tmp/oo-dl01w1-40-native/missing-ca.pem"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def leak(data: bytes) -> bool:
    if b"secret://" in data or b"secret_ref" in data:
        return True
    secret = Path(PASSWORD).read_bytes().strip()
    return bool(secret) and secret in data


def write(path: Path, data: bytes) -> None:
    if leak(data):
        raise SystemExit(f"refusing to write a secret into {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def full_registry(receipt: dict, receipt_sha: str, decision_sha: str) -> dict:
    return {
        "bindings": [
            {
                "admission_receipt_path": str(RECEIPT),
                "admission_receipt_sha256": receipt_sha,
                "arrow_schema_sha256": receipt["arrow_schema_sha256"],
                "capture_receipt_path": str(RECEIPT),
                "capture_receipt_sha256": receipt_sha,
                "catalog_path": str(CATALOG),
                "catalog_sha256": receipt["catalog_sha256"],
                "config_digest": receipt["config_digest"],
                "config_path": str(CONFIG),
                "config_sha256": sha256(CONFIG),
                "decision_sha256": decision_sha,
                "driver_digest": receipt["driver_digest"],
                "driver_kind": "native-http",
                "driver_name": "clickhouse-server",
                "driver_version": "26.8.21.10",
                "fields": [
                    {"arrow_type": "uint64", "key": True, "name": "reading_id", "nullable": False, "precision": 64},
                    {"arrow_type": "timestamp_nanos_utc", "name": "ts", "nullable": False},
                    {"arrow_type": "decimal256", "name": "amount", "nullable": False, "precision": 76, "scale": 10},
                    {"arrow_type": "binary", "name": "note", "nullable": True},
                    {"arrow_type": "int64", "name": "qty", "nullable": False, "precision": 64},
                ],
                "input_digest": receipt["input_digest"],
                "objects": [
                    {
                        "arrow_sha256": receipt["arrow_sha256"][0],
                        "logical_schema": "ch",
                        "logical_table": "readings",
                        "row_count": 3,
                    }
                ],
                "protocol": "oo.source-admission.capture-binding/v1",
                "report_path": str(REPORT),
                "report_sha256": sha256(REPORT),
                "run_id": receipt["run_id"],
                "schema_digest": receipt["schema_digest"],
                "snapshot": receipt["snapshot"],
                "source_engine": "clickhouse",
                "source_generation": receipt["source_generation"],
                "source_id": receipt["source_id"],
                "type_digest": receipt["type_digest"],
            }
        ],
        "protocol": "oo.source-admission.binding-registry/v1",
    }


def request(receipt: dict) -> dict:
    return {
        "config_digest": receipt["config_digest"],
        "input_digest": receipt["input_digest"],
        "owner": OWNER,
        "protocol": "oo.pipeline.native-sealed-catalog/v1",
        "run_id": "f972c2ba-1548-429c-8718-12f4cc8aa78f",
        "schema_digest": receipt["schema_digest"],
        "snapshot": receipt["snapshot"],
        "source_generation": 1,
        "source_id": receipt["source_id"],
        "target": {
            "ca_file": CA,
            "database": "oo_ch40",
            "host": "127.0.0.1",
            "password_file": PASSWORD,
            "port": 5432,
            "runtime_principal": "oo_ch40_runtime",
        },
        "target_generation": 1,
        "type_digest": receipt["type_digest"],
    }


def run_catalog(name: str, body: bytes, env: dict) -> None:
    folder = OUT / name
    folder.mkdir(parents=True, exist_ok=True)
    write(folder / "request.json", body)
    cmd = [
        "docker", "run", "-i", "--name", f"oo-dl01w1-40-seal-{name}",
        "--platform", "linux/arm64", "--pull", "never", "--network", "none",
        "-v", f"{ROOT}:{ROOT}:ro",
        "-v", "/tmp/oo-u06-194/debug/oo-pipeline:/tmp/oo-u06-194/debug/oo-pipeline:ro",
        "-v", "/tmp/oo-dl01w1-40-bind:/tmp/oo-dl01w1-40-bind:ro",
        IMAGE,
        "/tmp/oo-u06-194/debug/oo-pipeline", "catalog-import-native",
    ]
    child_env = os.environ.copy()
    for key in ("OO_NATIVE_ADMISSION_BIN", "OO_NATIVE_BINDING_REGISTRY", "OO_NATIVE_BINDING_REGISTRY_SHA256"):
        child_env.pop(key, None)
    for key, value in env.items():
        cmd[cmd.index(IMAGE):cmd.index(IMAGE)] = ["-e", f"{key}={value}"]
    child_env.update(env)
    proc = subprocess.run(cmd, input=body, capture_output=True, env=child_env)
    inspect = subprocess.run(
        ["docker", "inspect", f"oo-dl01w1-40-seal-{name}"],
        capture_output=True,
    )
    subprocess.run(["docker", "rm", f"oo-dl01w1-40-seal-{name}"], capture_output=True)
    write(folder / "stdout.txt", proc.stdout)
    write(folder / "stderr.txt", proc.stderr)
    write(folder / "exit", f"{proc.returncode}\n".encode())
    if inspect.returncode == 0:
        meta = json.loads(inspect.stdout)[0]
        host = meta["HostConfig"]
        summary = {
            "CpuQuota": host.get("CpuQuota"),
            "Memory": host.get("Memory"),
            "NanoCpus": host.get("NanoCpus"),
            "exit": proc.returncode,
            "stderr": proc.stderr.decode("utf-8", "replace").strip(),
            "stdout_bytes": len(proc.stdout),
        }
        write(folder / "cgroup.json", (json.dumps(summary, indent=2) + "\n").encode())


def decision_summary() -> None:
    proc = subprocess.run(
        [
            "docker", "run", "--rm", "--platform", "linux/arm64", "--pull", "never", "--network", "none",
            "-v", f"{ADMISSION}:/elf/oo-source-admission:ro",
            "-v", f"{REPORT.parent}:/inputs:ro",
            IMAGE,
            "/elf/oo-source-admission", "validate", "--report", "/inputs/report.json", "--config", "/inputs/config.json",
        ],
        capture_output=True,
    )
    stdout = proc.stdout
    digest = hashlib.sha256(stdout).hexdigest()
    summary = {
        "admission_bin_sha256": sha256(ADMISSION),
        "exit": proc.returncode,
        "stderr": proc.stderr.decode("utf-8", "replace").strip(),
        "stdout_sha256": digest,
        "stdout_bytes": len(stdout),
    }
    if proc.returncode == 0 and stdout:
        doc = json.loads(stdout)
        identity = doc.get("identity") if isinstance(doc.get("identity"), dict) else {}
        model = doc.get("model") if isinstance(doc.get("model"), dict) else {}
        methods = model.get("capture_methods") if isinstance(model.get("capture_methods"), list) else []
        caps = model.get("capabilities") if isinstance(model.get("capabilities"), list) else []
        summary.update({
            "admitted": doc.get("admitted"),
            "blocks_len": len(doc.get("blocks") or []),
            "capability_write_class": [item.get("write_class") for item in caps if isinstance(item, dict)],
            "capture_method_names": [item.get("method") for item in methods if isinstance(item, dict)],
            "capture_method_write_class": [item.get("write_class") for item in methods if isinstance(item, dict)],
            "identity_embeds_ref_key": "secret_ref" in identity,
            "matrix_mutated": doc.get("matrix_mutated"),
            "protocol": doc.get("protocol"),
            "source_id": identity.get("source_id"),
        })
    write(OUT / "validator.json", (json.dumps(summary, indent=2) + "\n").encode())


def main() -> None:
    receipt = json.loads(RECEIPT.read_text())
    receipt_sha = sha256(RECEIPT)
    if receipt_sha != "e6fe7103cc5d64cafd9ac9ac274fcd37162eeeaad77041952c199c8e7206bd39":
        raise SystemExit(f"generation-1 receipt hash changed: {receipt_sha}")
    if sha256(CATALOG) != receipt["catalog_sha256"]:
        raise SystemExit("catalog file hash does not match the receipt")
    if sha256(STUB) != receipt["registry_sha256"]:
        raise SystemExit("stub registry hash does not match the receipt")
    if not PIPELINE.is_file() or sha256(PIPELINE) != "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe":
        raise SystemExit("pipeline ELF hash changed")
    body = (json.dumps(request(receipt), indent=2) + "\n").encode()
    decision_summary()
    decision_sha = json.loads((OUT / "validator.json").read_text())["stdout_sha256"]
    registry = full_registry(receipt, receipt_sha, decision_sha)
    rendered = (json.dumps(registry, indent=2) + "\n").encode()
    write(OUT / "full-registry.json", rendered)
    registry_sha = hashlib.sha256(rendered).hexdigest()
    write(OUT / "full-registry.sha256", (registry_sha + "\n").encode())
    write(OUT / "receipt-registry.sha256", (receipt["registry_sha256"] + "\n").encode())
    decision_summary()
    run_catalog("unset-env", body, {})
    run_catalog("stub-registry", body, {
        "OO_NATIVE_ADMISSION_BIN": str(ADMISSION),
        "OO_NATIVE_BINDING_REGISTRY": str(STUB),
        "OO_NATIVE_BINDING_REGISTRY_SHA256": receipt["registry_sha256"],
    })
    run_catalog("full-registry", body, {
        "OO_NATIVE_ADMISSION_BIN": str(ADMISSION),
        "OO_NATIVE_BINDING_REGISTRY": str(OUT / "full-registry.json"),
        "OO_NATIVE_BINDING_REGISTRY_SHA256": registry_sha,
    })


if __name__ == "__main__":
    main()
