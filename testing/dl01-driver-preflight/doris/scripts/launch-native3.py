#!/usr/bin/env python3
"""Fresh native FE/BE/reader gate from private copies. No live repo mount."""

import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
REPO = os.path.abspath(os.path.join(ROOT, "..", "..", ".."))
STAGING = "/tmp/oo-dl01w1-43-native3"
EVIDENCE = os.path.join(
    REPO,
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/native-3",
)
PRODUCT = os.path.join(
    REPO,
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/current",
)
COMPILE7 = os.path.join(
    REPO,
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/compile-7/bin",
)
OWN = ("oo-dl01w1-43-fe", "oo-dl01w1-43-be", "oo-dl01w1-43-reader")
STAGED = {"ok": False}


def run(cmd):
    return subprocess.run(cmd, text=True, capture_output=True)


def say(text):
    print(text, flush=True)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def remove_own():
    for name in OWN:
        inspected = run([
            "docker", "inspect", "-f", '{{index .Config.Labels "oo.owner"}}', name,
        ])
        if inspected.returncode != 0:
            continue
        owner = inspected.stdout.strip()
        if owner != "DL01-W1-43":
            say(f"leave foreign container {name} owner={owner}")
            return 2
        removed = run(["docker", "rm", "-f", name])
        say(f"removed {name} exit={removed.returncode}")
        if removed.returncode != 0:
            return removed.returncode or 1
    return 0


def stage():
    if os.path.isdir(STAGING):
        shutil.rmtree(STAGING)
    boot = os.path.join(STAGING, "boot")
    sql = os.path.join(STAGING, "sql")
    oracle = os.path.join(STAGING, "oracle")
    bin_dir = os.path.join(STAGING, "bin")
    os.makedirs(boot)
    os.makedirs(oracle)
    os.makedirs(os.path.join(STAGING, "admit"))
    os.makedirs(os.path.join(STAGING, "run"))
    os.makedirs(bin_dir)
    for name in ("fe-boot.sh", "be-boot.sh", "judge.py"):
        shutil.copy2(os.path.join(ROOT, "scripts", name), os.path.join(boot, name))
    os.chmod(os.path.join(boot, "fe-boot.sh"), 0o755)
    os.chmod(os.path.join(boot, "be-boot.sh"), 0o755)
    shutil.copytree(os.path.join(ROOT, "sql"), sql)
    shutil.copy2(os.path.join(ROOT, "fixtures", "oracle.json"), os.path.join(oracle, "oracle.json"))
    lines = []
    for name in ("doris-reader", "doris-decode"):
        source = os.path.join(COMPILE7, name)
        target = os.path.join(bin_dir, name)
        shutil.copy2(source, target)
        os.chmod(target, 0o755)
        lines.append(f"{sha256(target)}  {name}\n")
    with open(os.path.join(STAGING, "run", "binaries.sha256"), "w", encoding="utf-8") as handle:
        handle.writelines(lines)
    text = open(os.path.join(boot, "fe-boot.sh"), encoding="utf-8").read()
    if "sys_log_mode = NORMAL" not in text:
        raise SystemExit("staged fe-boot.sh is missing the sync audit setting")
    STAGED["ok"] = True


def wait_backend():
    deadline = time.time() + 600
    last = ""
    while time.time() < deadline:
        status = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}}", "oo-dl01w1-43-fe"])
        be = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}}", "oo-dl01w1-43-be"])
        fe_state = status.stdout.strip()
        be_state = be.stdout.strip()
        if not fe_state.startswith("running"):
            say(f"fe not running: {fe_state}")
            return 1
        if be_state.startswith("exited"):
            say(f"be exited: {be_state}")
            return 1
        probed = run([
            "docker", "exec", "oo-dl01w1-43-fe",
            "mysql", "-uroot", "-P9030", "-h127.0.0.1", "-N", "-B", "-e", "SHOW BACKENDS",
        ])
        last = probed.stdout
        for line in probed.stdout.splitlines():
            fields = line.split("\t")
            if "172.16.43.20" in fields and any(field == "true" for field in fields):
                path = os.path.join(STAGING, "admit", "backends.txt")
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(probed.stdout)
                say("backend alive")
                return 0
        say(f"waiting backend fe={fe_state} be={be_state}")
        time.sleep(10)
    with open(os.path.join(STAGING, "admit", "backends-timeout.txt"), "w", encoding="utf-8") as handle:
        handle.write(last)
    say("backend wait timed out")
    return 1


def prepare_manifest(run_dir):
    gate = os.path.join(run_dir, "gate")
    document = {
        "protocol": "oo.doris.import-manifest-prep/v1",
        "sealed_catalog": False,
        "compiler_invoked": False,
        "importer_invoked": False,
        "engine": "doris",
        "product_engine": "apache-doris",
        "storage_label": "not_loaded",
        "not_a_sealed_catalog": True,
        "sealed_protocol_not_emitted": "oo.pipeline.sealed-import-catalog/v1",
        "caveat": "cast_decimal_10_2_saturates_to_99999999.99_lossless_false",
        "awaiting": "W06 physical loader",
    }
    sat_path = os.path.join(gate, "saturation.json")
    confirm_path = os.path.join(gate, "confirm.json")
    catalog_path = os.path.join(gate, "catalog.json")
    if os.path.isfile(sat_path):
        document["saturation"] = json.load(open(sat_path, encoding="utf-8"))
    if os.path.isfile(confirm_path):
        document["confirm"] = json.load(open(confirm_path, encoding="utf-8"))
    if os.path.isfile(catalog_path):
        catalog = json.load(open(catalog_path, encoding="utf-8"))
        columns = []
        for row in catalog.get("columns", []):
            if len(row) < 8:
                continue
            columns.append({
                "logical_schema": row[0],
                "logical_table": row[1],
                "logical_name": row[2],
                "data_type": row[3],
                "source_type": row[4],
                "nullable": row[5],
                "key": row[6],
                "ordinal": row[7],
            })
        document["columns"] = columns
        document["tables"] = sorted({column["logical_table"] for column in columns})
    arrows = []
    if os.path.isdir(gate):
        for name in sorted(os.listdir(gate)):
            if not name.endswith(".arrow"):
                continue
            path = os.path.join(gate, name)
            arrows.append({
                "file": name,
                "sha256": sha256(path),
                "bytes": os.path.getsize(path),
            })
    document["arrow"] = arrows
    target = os.path.join(run_dir, "manifest-prep.json")
    with open(target, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=2)
        handle.write("\n")
    return document


def publish():
    os.makedirs(EVIDENCE, exist_ok=True)
    for part in ("admit", "run"):
        source = os.path.join(STAGING, part)
        dest = os.path.join(EVIDENCE, part)
        if not os.path.isdir(source):
            continue
        if os.path.isdir(dest):
            shutil.rmtree(dest)
        shutil.copytree(source, dest)
    prep = os.path.join(EVIDENCE, "run", "manifest-prep.json")
    if os.path.isfile(prep):
        os.makedirs(PRODUCT, exist_ok=True)
        shutil.copy2(prep, os.path.join(PRODUCT, "manifest-prep.json"))


def capture_failure_logs():
    dest = os.path.join(STAGING, "admit")
    os.makedirs(dest, exist_ok=True)
    for name in ("oo-dl01w1-43-fe", "oo-dl01w1-43-be"):
        logs = run(["docker", "logs", "--tail", "80", name])
        with open(os.path.join(dest, f"{name}.log-tail.txt"), "w", encoding="utf-8") as handle:
            handle.write(logs.stdout)
            handle.write(logs.stderr)


def main():
    code = remove_own()
    if code != 0:
        return code
    stage()
    env = os.environ.copy()
    env["OO_DORIS_ADMIT_EVIDENCE"] = os.path.join(STAGING, "admit")
    env["OO_DORIS_BOOT_DIR"] = os.path.join(STAGING, "boot")
    env["OO_DORIS_BIN"] = os.path.join(STAGING, "bin")
    env["OO_DORIS_SQL"] = os.path.join(STAGING, "sql")
    env["OO_DORIS_ORACLE"] = os.path.join(STAGING, "oracle")
    env["OO_DORIS_RUN"] = os.path.join(STAGING, "run")
    env["OO_DORIS_JUDGE"] = os.path.join(STAGING, "boot", "judge.py")
    say("admit fe+be")
    admitted = subprocess.run(
        [sys.executable, os.path.join(ROOT, "scripts", "admit-native.py")],
        env=env,
    )
    say(f"admit exit={admitted.returncode}")
    if admitted.returncode != 0:
        capture_failure_logs()
        return admitted.returncode
    ready = wait_backend()
    if ready != 0:
        capture_failure_logs()
        return ready
    say("reader gate")
    reader = subprocess.run(
        [sys.executable, os.path.join(ROOT, "scripts", "run-reader.py")],
        env=env,
    )
    say(f"reader exit={reader.returncode}")
    prepare_manifest(os.path.join(STAGING, "run"))
    return reader.returncode


if __name__ == "__main__":
    exit_code = 1
    try:
        exit_code = main()
    finally:
        if STAGED["ok"]:
            try:
                prepare_manifest(os.path.join(STAGING, "run"))
            except Exception as exc:
                say(f"manifest prep failed: {exc}")
            try:
                publish()
                say(f"published {EVIDENCE}")
            except Exception as exc:
                say(f"publish failed: {exc}")
                exit_code = exit_code or 1
        removed = remove_own()
        if removed != 0 and exit_code == 0:
            exit_code = removed
        say(f"native3 exit={exit_code}")
    sys.exit(exit_code)
