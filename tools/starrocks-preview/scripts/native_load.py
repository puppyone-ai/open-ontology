#!/usr/bin/env python3
"""Run the fixed-archive oo-pipeline against producer receipts.

Reads manifest.json, calls install-native-update and import-native-update,
then compares sealed physical cells with the captured truth. Does not print
the database password.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CHAIN = (
    ROOT
    / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-chain"
)
ELF = Path("/tmp/oo-w45-native-out/oo-pipeline")
IMAGE = "rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG = "oo-dl01w1-45-native-pg"
PASSWORD = Path("/tmp/oo-w45-native-pg.pw")
REPO = str(ROOT)


def run(cmd: list[str], stdin: bytes | None = None, timeout: int = 180) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, input=stdin, capture_output=True, timeout=timeout)


def save(directory: Path, result: subprocess.CompletedProcess) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "stdout").write_bytes(result.stdout)
    (directory / "stderr").write_bytes(result.stderr)
    (directory / "exit").write_text(str(result.returncode) + "\n")


def mount(source: str, target: str) -> list[str]:
    # `-v source:target:ro` on this Docker mounts the tree at a path with a
    # trailing "o" and the files are missing. `--mount` keeps the absolute path.
    return ["--mount", f"type=bind,source={source},target={target},readonly"]


def cli(command: str, request: bytes) -> subprocess.CompletedProcess:
    return run(
        [
            "docker",
            "run",
            "--rm",
            "-i",
            "--network",
            f"container:{PG}",
            *mount(REPO, REPO),
            *mount(str(ELF), "/opt/oo-pipeline"),
            *mount(str(PASSWORD), "/run/secrets/pg.pw"),
            "--entrypoint",
            "/opt/oo-pipeline",
            IMAGE,
            command,
        ],
        stdin=request,
    )


def psql(sql: str) -> subprocess.CompletedProcess:
    return run(
        ["docker", "exec", "-i", PG, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "oo_w45_native", "-At", "-F", "\t"],
        stdin=sql.encode(),
    )


def ident(name: str) -> str:
    name = name.strip()
    if len(name) == 63 and name[:3] in {"pt_", "pc_", "pn_"} and all(ch in "0123456789abcdef" for ch in name[3:]):
        return name
    raise ValueError(f"unexpected physical name {name!r} len={len(name)}")


def bag(rows: list) -> list:
    return sorted(json.dumps(row, ensure_ascii=False) for row in rows)


def compare_one(request: dict) -> dict:
    generation = int(request["source_generation"])
    table = request["logical_table"]
    schema = request["logical_schema"]
    source = request["source_id"]
    lookup = psql(
        "SELECT physical_name FROM _oo.physical_mappings "
        f"WHERE source = '{source}' AND level = 't' AND logical_namespace = '{schema}' "
        f"AND logical_table = '{table}' AND logical_column = '' AND run_generation = {generation};\n"
    )
    if lookup.returncode != 0:
        return {"ok": False, "reason": "mapping_query", "stderr": lookup.stderr.decode()[:400]}
    names = [line for line in lookup.stdout.decode().splitlines() if line]
    if len(names) != 1:
        return {"ok": False, "reason": "table_mapping", "names": names}
    physical = ident(names[0])
    columns = []
    for column in request["columns"]:
        found = psql(
            "SELECT physical_name FROM _oo.physical_mappings "
            f"WHERE source = '{source}' AND level = 'c' AND logical_namespace = '{schema}' "
            f"AND logical_table = '{table}' AND logical_column = '{column['logical_name']}' "
            f"AND run_generation = {generation};\n"
        )
        got = [line for line in found.stdout.decode().splitlines() if line]
        if found.returncode != 0 or len(got) != 1:
            return {"ok": False, "reason": "column_mapping", "column": column["logical_name"], "names": got}
        columns.append(ident(got[0]))
    select = ", ".join(f'"{name}"::text' for name in columns)
    read = psql(f'SELECT {select} FROM _oo."{physical}" ORDER BY "_oo_pos";\n')
    if read.returncode != 0:
        return {"ok": False, "reason": "read", "stderr": read.stderr.decode()[:400]}
    width = len(columns)
    got_rows = []
    text = read.stdout.decode()
    if text.strip():
        for line in text.splitlines():
            parts = line.split("\t")
            if len(parts) != width:
                return {"ok": False, "reason": "width", "line": line[:200]}
            got_rows.append([None if part == "" else part for part in parts])
    # psql -At prints SQL NULL as an empty field. Distinguish empty string with a typed query.
    typed = psql(
        "SELECT COALESCE(jsonb_agg(row_values ORDER BY row_values::text), '[]'::jsonb)::text FROM ("
        f"SELECT jsonb_build_array({', '.join(f'\"{name}\"::text' for name in columns)}) AS row_values "
        f'FROM _oo."{physical}") s;\n'
    )
    if typed.returncode != 0:
        return {"ok": False, "reason": "typed_read", "stderr": typed.stderr.decode()[:400]}
    typed_rows = json.loads(typed.stdout.decode().strip() or "[]")
    truth = request["truth"]
    match = bag(typed_rows) == bag(truth)
    absent_302 = True
    if table == "pk_accounts":
        absent_302 = not any(row[0] == "302" for row in typed_rows)
    return {
        "ok": match and (absent_302 or generation == 1),
        "cells_equal": match,
        "got_rows": len(typed_rows),
        "truth_rows": len(truth),
        "account_302_absent": absent_302 if table == "pk_accounts" else None,
        "physical": physical,
        "untyped_rows": len(got_rows),
    }


def main() -> int:
    resume = "--resume" in sys.argv
    if not ELF.is_file():
        print("elf missing", file=sys.stderr)
        return 2
    phases = CHAIN / "phases"
    phases.mkdir(parents=True, exist_ok=True)
    file_out = run(["file", str(ELF)])
    (phases / "elf.file").write_bytes(file_out.stdout)
    sha = run(["shasum", "-a", "256", str(ELF)])
    (phases / "elf.sha256").write_bytes(sha.stdout)
    install = {
        "protocol": "oo.pipeline.install-native-update/v1",
        "target": {
            "host": "127.0.0.1",
            "port": 5432,
            "database": "oo_w45_native",
            "runtime_principal": "postgres",
            "password_file": "/run/secrets/pg.pw",
            "ssl": "disable",
            "shard": "sr_w45",
            "owner": json.loads((CHAIN / "manifest.json").read_text())["owner"],
        },
    }
    install_path = phases / "install-request.json"
    install_path.write_text(json.dumps(install, separators=(",", ":")) + "\n")
    prior_install = phases / "install" / "exit"
    if resume and prior_install.is_file() and prior_install.read_text().strip() == "0":
        print("install skipped")
    else:
        installed = cli("install-native-update", install_path.read_bytes())
        save(phases / "install", installed)
        print("install", installed.returncode, installed.stdout.decode()[:300])
        if installed.returncode != 0:
            return 1
    manifest = json.loads((CHAIN / "manifest.json").read_text())
    results = []
    for item in manifest["loads"]:
        slot = phases / f"g{item['generation']}-{item['table']}"
        prior = slot / "stdout"
        if resume and prior.is_file():
            try:
                old = json.loads(prior.read_text())
            except json.JSONDecodeError:
                old = {}
            if old.get("load_state") == "awaiting_publish":
                results.append(
                    {
                        "phase": item["phase"],
                        "table": item["table"],
                        "generation": item["generation"],
                        "exit": 0,
                        "load_state": "awaiting_publish",
                        "source_bound": old.get("source_bound"),
                        "reason": None,
                        "run_id": old.get("run_id"),
                        "resumed": True,
                    }
                )
                print(f"g{item['generation']} {item['table']} resumed")
                continue
        request = Path(item["request"]).read_bytes()
        (slot / "request.json").write_bytes(request)
        result = cli("import-native-update", request)
        save(slot, result)
        stdout = result.stdout.decode()
        parsed = None
        try:
            parsed = json.loads(stdout) if stdout.strip() else None
        except json.JSONDecodeError:
            parsed = None
        results.append(
            {
                "phase": item["phase"],
                "table": item["table"],
                "generation": item["generation"],
                "exit": result.returncode,
                "load_state": None if parsed is None else parsed.get("load_state"),
                "source_bound": None if parsed is None else parsed.get("source_bound"),
                "reason": None if parsed is None else parsed.get("reason"),
                "run_id": None if parsed is None else parsed.get("run_id"),
            }
        )
        print(
            f"g{item['generation']} {item['table']} exit={result.returncode} "
            f"state={results[-1]['load_state']} bound={results[-1]['source_bound']} reason={results[-1]['reason']}"
        )
    compares = []
    for item in manifest["loads"]:
        if not any(row["generation"] == item["generation"] and row["table"] == item["table"] and row["load_state"] == "awaiting_publish" for row in results):
            continue
        request = json.loads(Path(item["request"]).read_text())
        compared = compare_one(request)
        compared["phase"] = item["phase"]
        compared["table"] = item["table"]
        compared["generation"] = item["generation"]
        compares.append(compared)
        print("compare", item["phase"], item["table"], compared.get("ok"), compared.get("reason"))
    summary = {"imports": results, "compares": compares}
    (phases / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    failed = [row for row in results if row["exit"] != 0 or row["load_state"] != "awaiting_publish"]
    bad = [row for row in compares if not row.get("ok")]
    print(json.dumps({"imports": len(results), "failed": len(failed), "compare_bad": len(bad)}))
    return 0 if not failed and not bad else 1


if __name__ == "__main__":
    raise SystemExit(main())
