#!/usr/bin/env python3
"""Read fixed 05 and PG C1 archives into the existing control ELF.

The catalog, profile, and source-semantics files are copied byte for byte.
This process does not rebuild the ELF, does not invent column types, and does
not open a model bearer or a source password.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess

from closed_loop import (
    COMMIT,
    IDENTITY_ORIGIN,
    ORIGIN,
    Client,
    Fail,
    Identity,
    Raw,
    admit,
    card_ready,
    code_of,
    endpoint,
    require,
    sha256_bytes,
    stage_map,
    wait_ready,
    write_json,
)

PG_SUMS = "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/SHA256SUMS"
PG_ROOT = "evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product"
CATALOG_REL = "linux/runner/out/capture/phases/catalog-capture-check.json"
PROFILE_REL = "linux/runner/out/capture/phases/compare-capture-compare.json"
SEM_SUMS = "evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/oracle-source-bound/SHA256SUMS"
SEM_REL = (
    "evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/"
    "oracle-source-bound/post-edit-6/cli/batch-positive/profile.json"
)
ELF_SHA = "575e33a739c8b265b74d5d4baa065709a4962dfde03d6f5da882f7a5422df8da"
CLOSED_TYPES = (
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
)


def file_sha(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_sums(sums_path: str, root: str) -> tuple[int, list[str]]:
    matched = 0
    bad: list[str] = []
    with open(sums_path, encoding="utf-8") as handle:
        for line in handle:
            text = line.rstrip("\n")
            if not text or text.startswith("#"):
                continue
            digest, separator, rel = text.partition("  ")
            if separator != "  " or len(digest) != 64:
                bad.append(f"malformed sums line: {text[:80]}")
                continue
            path = os.path.join(root, rel)
            if not os.path.isfile(path):
                bad.append(f"missing {rel}")
                continue
            actual = file_sha(path)
            if actual != digest:
                bad.append(f"mismatch {rel}")
            else:
                matched += 1
    return matched, bad


def copy_exact(source: str, dest: str) -> str:
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.copyfile(source, dest)
    os.chmod(dest, 0o600)
    return file_sha(dest)


def write_bytes(path: str, payload: dict) -> str:
    raw = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(raw)
    os.chmod(path, 0o600)
    return sha256_bytes(raw)


def typed_comparison(catalog: dict, profile: dict, semantics: dict) -> dict:
    columns = []
    for table in catalog.get("tables", []):
        schema = table.get("schema")
        name = table.get("table")
        for column in table.get("columns", []):
            columns.append(
                {
                    "dataset": f"{schema}.{name}",
                    "column": column if isinstance(column, str) else None,
                    "data_type": None,
                    "type_status": "not_in_source_catalog",
                    "auto_approved": False,
                }
            )
    counts = [
        {
            "table": item.get("table"),
            "source_rows": item.get("source_rows"),
            "target_rows": item.get("target_rows"),
            "cell_mismatches": item.get("cell_mismatches"),
        }
        for item in profile.get("tables", [])
    ]
    semantic_columns = []
    for item in semantics.get("columns", []):
        semantic_columns.append(
            {
                "dataset": item.get("dataset"),
                "column": item.get("column"),
                "data_type": None,
                "consistency_grade": semantics.get("consistency_grade"),
                "delete_detection": semantics.get("delete_detection"),
                "grade_status": "unknown" if semantics.get("consistency_grade") is None else "present",
                "delete_status": "unknown" if semantics.get("delete_detection") is None else "present",
                "auto_approved": False,
            }
        )
    return {
        "catalog_protocol": catalog.get("protocol"),
        "profile_protocol": profile.get("protocol"),
        "semantics_protocol": semantics.get("protocol"),
        "catalog_contract_key": "contract" in catalog,
        "profile_contract_key": "contract" in profile,
        "semantics_contract_key": "contract" in semantics,
        "rows_compared": profile.get("rows_compared"),
        "sampled": profile.get("sampled"),
        "full_scan": profile.get("full_scan"),
        "table_counts": counts,
        "c1_columns": columns,
        "semantics_columns": semantic_columns,
        "control_api_closed_types": list(CLOSED_TYPES),
        "types_assigned": 0,
        "unknown_auto_approved": False,
        "ready": False,
        "published": False,
    }


def elf_sha(image: str) -> str:
    ran = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--pull",
            "never",
            "-v",
            "oo-dl01w1-71-target:/target:ro",
            image,
            "sha256sum",
            "/target/release/selfbuild-control-api",
        ],
        capture_output=True,
        text=True,
    )
    require(ran.returncode == 0, "elf hash probe failed")
    return ran.stdout.split()[0]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--cache", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--network", required=True)
    parser.add_argument("--api-name", required=True)
    parser.add_argument("--pg-name", required=True)
    parser.add_argument("--lock", required=True)
    args = parser.parse_args()
    os.chdir(args.root)
    summary: dict = {
        "elf_rebuilt": False,
        "source_artifact": "not_attached",
        "model_bearer": "path_absent",
        "published": False,
        "business_owner_signed": False,
        "historical_cancel_get": "not_queried",
        "generation_injection": "not_run",
        "live_rs_counted_as_postgres": False,
        "workflow_100k_counted_as_run": False,
    }
    pg_matched, pg_bad = check_sums(PG_SUMS, PG_ROOT)
    sem_matched, sem_bad = check_sums(SEM_SUMS, args.root)
    summary["pg_sums_matched"] = pg_matched
    summary["pg_sums_bad"] = pg_bad[:8]
    summary["semantics_sums_matched"] = sem_matched
    summary["semantics_sums_bad"] = sem_bad[:8]
    require(pg_matched == 1773 and not pg_bad, "pg fixed hashes did not match")
    require(sem_matched > 0 and not sem_bad, "source-semantics fixed hashes did not match")
    catalog_path = os.path.join(PG_ROOT, CATALOG_REL)
    profile_path = os.path.join(PG_ROOT, PROFILE_REL)
    semantics_path = os.path.join(args.root, SEM_REL)
    catalog = json.loads(open(catalog_path, encoding="utf-8").read())
    profile = json.loads(open(profile_path, encoding="utf-8").read())
    semantics = json.loads(open(semantics_path, encoding="utf-8").read())
    comparison = typed_comparison(catalog, profile, semantics)
    require(comparison["rows_compared"] == 100050, "profile row count drifted")
    require(comparison["sampled"] is False, "profile was sampled")
    require(semantics.get("consistency_grade") is None, "grade was present")
    require(semantics.get("delete_detection") is None, "delete detection was present")
    require(all(item["data_type"] is None for item in comparison["c1_columns"]), "a C1 type was invented")
    write_json(os.path.join(args.evidence, "typed-comparison.json"), comparison)
    bundle = os.path.join(args.cache, "bundles", "c1fixed")
    os.makedirs(bundle, exist_ok=True)
    copied = {
        "catalog.json": copy_exact(catalog_path, os.path.join(bundle, "catalog.json")),
        "profile.json": copy_exact(profile_path, os.path.join(bundle, "profile.json")),
        "source_semantics.json": copy_exact(semantics_path, os.path.join(bundle, "source_semantics.json")),
    }
    require(copied["catalog.json"] == file_sha(catalog_path), "catalog copy drifted")
    envelopes = {
        "read_logic.json": {
            "contract": "oo.selfbuild-control-api/read-logic/v1",
            "executed": False,
            "source": "control_envelope",
        },
        "rules.json": {"columns": [], "relationships": []},
        "suggestions.json": {
            "certified": False,
            "contract": "oo.selfbuild-control-api/suggestions/v1",
            "merged": False,
            "ready": False,
        },
        "generation.json": {
            "llm_calls": 0,
            "outcome": "structural_draft",
            "proxy_call": False,
        },
        "roles.json": {
            "contract": "oo.selfbuild-control-api/sealed-roles/v1",
            "roles": {"oo_r_reader": ["public.hubs.hub_id"]},
        },
    }
    for name, value in envelopes.items():
        copied[name] = write_bytes(os.path.join(bundle, name), value)
    manifest = {"contract": "oo.selfbuild-control-api/sealed-bundle/v1", "files": copied}
    write_json(os.path.join(bundle, "manifest.json"), manifest)
    tables = [{"schema": item["schema"], "table": item["table"]} for item in catalog["tables"]]
    config = {
        "binaries": {
            "catalog": os.path.join(args.cache, "bin", "missing-catalog"),
            "pg_builder": os.path.join(args.cache, "bin", "missing-pg-builder"),
            "review_ledger": os.path.join(args.cache, "bin", "missing-review-ledger"),
            "rules": os.path.join(args.cache, "bin", "missing-rules"),
            "suggestions": os.path.join(args.cache, "bin", "missing-suggestions"),
        },
        "contract": "oo.selfbuild-control-api.config/v1",
        "deployment_origins": [ORIGIN],
        "identity": {"ca_file": os.path.join(args.cache, "tls", "ca", "ca.crt"), "origin": IDENTITY_ORIGIN},
        "ledger": endpoint(args.cache, args.pg_name),
        "listen": {
            "bind": "0.0.0.0:8443",
            "cert_file": os.path.join(args.cache, "tls", "api", "server.crt"),
            "key_file": os.path.join(args.cache, "tls", "api", "server.key"),
        },
        "model_cache_dir": "",
        "postgres": endpoint(args.cache, args.pg_name),
        "principals": [],
        "projects": [
            {
                "allowance_id": "c1_allow",
                "config_commit": COMMIT,
                "hidden_columns": [],
                "llm": {
                    "bearer_file": "/Users/redux/.cache/oo-dl01w1-71/missing-bearer",
                    "model_id": "grok-4.7",
                    "origin": "http://127.0.0.1:8317",
                },
                "llm_secret_ref": "secret://llm/sealed",
                "project_id": "c1fixed",
                "role_policy_file": os.path.join(args.cache, "role-policy.json"),
                "rules_auto_accept": "rule_high_confidence",
                "sample_policy": "none",
                "sealed_bundle": bundle,
                "source": {
                    "ca_file": "/Users/redux/.cache/oo-dl01w1-71/missing-source-ca",
                    "database": "unused",
                    "host": "unused.invalid",
                    "password_file": "/Users/redux/.cache/oo-dl01w1-71/missing-source-password",
                    "port": 5432,
                    "tables": tables,
                    "username": "unused",
                },
                "source_ref": "c1_src",
                "source_secret_ref": "secret://source/unused",
                "visible_columns": [],
            }
        ],
        "work_dir": os.path.join(args.cache, "work"),
    }
    write_json(os.path.join(args.cache, "role-policy.json"), {"active": False, "published": False})
    write_json(os.path.join(args.cache, "config.json"), config)
    summary["input_sha256"] = copied
    summary["elf_sha256"] = elf_sha(args.image)
    require(summary["elf_sha256"] == ELF_SHA, "target ELF is not the recorded binary")
    raw = Raw(os.path.join(args.evidence, "raw"))
    identity = Identity(
        os.path.join(args.cache, "tls", "identity", "server.crt"),
        os.path.join(args.cache, "tls", "identity", "server.key"),
    )
    try:
        identity.start()
        identity.ready.wait(5)
        install = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "--network",
                args.network,
                "--pull",
                "never",
                "-v",
                f"{args.cache}:{args.cache}",
                "-v",
                "oo-dl01w1-71-target:/target:ro",
                args.image,
                "/target/release/selfbuild-control-api",
                "install",
                "--config",
                os.path.join(args.cache, "config.json"),
            ],
            capture_output=True,
        )
        with open(os.path.join(args.evidence, "phases", "install-command.log"), "wb") as handle:
            handle.write(install.stdout + install.stderr)
        require(install.returncode == 0, "install failed")
        serve = [
            "docker",
            "run",
            "-d",
            "--name",
            args.api_name,
            "--pull",
            "never",
            "--network",
            args.network,
            "--add-host",
            "host.docker.internal:host-gateway",
            "--memory",
            "1g",
            "--label",
            "oo.project=open-ontology",
            "--label",
            "oo.owner=dl01w1-71",
            "--label",
            "oo.task=DL01-W1-71",
            "--label",
            "oo.kind=control-api",
            "-p",
            "127.0.0.1:18443:8443",
            "-v",
            f"{args.cache}:{args.cache}",
            "-v",
            "oo-dl01w1-71-target:/target:ro",
            args.image,
            "/target/release/selfbuild-control-api",
            "serve",
            "--config",
            os.path.join(args.cache, "config.json"),
        ]
        admit(args.lock, 0, "oo-dl01w1-71", args.evidence, "api", serve)
        wait_ready(args.api_name, 0)
        api_cpu = subprocess.check_output(
            ["docker", "inspect", args.api_name, "--format", "{{.HostConfig.NanoCpus}}"], text=True
        ).strip()
        pg_cpu = subprocess.check_output(
            ["docker", "inspect", args.pg_name, "--format", "{{.HostConfig.NanoCpus}}"], text=True
        ).strip()
        summary["api_nanocpus"] = api_cpu
        summary["pg_nanocpus"] = pg_cpu
        require(api_cpu == "0" and pg_cpu == "0", "cpu quota is still set")
        client = Client(os.path.join(args.cache, "tls", "ca", "ca.crt"), raw)
        status, anon = client.exchange("GET", "/control/v1/builds/" + "b_" + "0" * 32, None, None, "anon")
        require(status == 401 and code_of(anon) == "unauthenticated", "anon was not 401")
        created_status, created = client.exchange(
            "POST",
            "/control/v1/builds",
            {
                "allowance_id": "c1_allow",
                "config_commit": COMMIT,
                "identity_revision": 1,
                "idempotency_key": client.idem(),
                "llm_secret_ref": "secret://llm/sealed",
                "project_id": "c1fixed",
                "source_ref": "c1_src",
            },
            "admin",
            "create-c1",
        )
        require(created_status == 202 and isinstance(created.get("stages"), list), "create did not return stages")
        build_id = created["build_id"]
        summary["build_id"] = build_id

        def failed_s0(document: dict) -> bool:
            stages = stage_map(document)
            return stages.get("S0", {}).get("status") == "failed"

        from closed_loop import wait_row

        row = wait_row(client, build_id, failed_s0, "wait-s0")
        stages = stage_map(row)
        summary["s0_detail"] = stages["S0"].get("detail_code")
        summary["job_status"] = row.get("status")
        require(summary["s0_detail"] == "sealed.malformed", "S0 did not refuse the fixed bytes")
        for name in [f"S{index}" for index in range(1, 11)]:
            require(stages[name]["status"] != "succeeded", f"{name} succeeded after a malformed seal")
        cards_status, cards = client.exchange("GET", f"/control/v1/builds/{build_id}/cards", None, "admin", "cards")
        require(cards_status == 200 and card_ready(cards) is False, "cards became ready")
        pack_status, pack = client.exchange(
            "POST",
            f"/control/v1/builds/{build_id}/role-pack",
            {
                "identity_revision": 1,
                "idempotency_key": client.idem(),
                "policy_version": "oos/v1",
                "role": "oo_r_reader",
            },
            "admin",
            "pack",
        )
        summary["pack_status"] = pack_status
        require(pack_status != 200, "role pack was compiled from the refused input")
        require(pack.get("published") is False and pack.get("compiled_pack") == "not_published", "pack claimed publication")
        summary["ok"] = True
    except Exception as exc:
        summary["ok"] = False
        summary["error"] = str(exc)
        raise
    finally:
        identity.close()
        write_json(os.path.join(args.evidence, "consume-summary.json"), summary)


if __name__ == "__main__":
    main()
