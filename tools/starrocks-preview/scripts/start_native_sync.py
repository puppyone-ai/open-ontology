#!/usr/bin/env python3
"""Start only a new owned synthetic StarRocks source for native scheduler proof."""
import importlib.util
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
SOURCE = "oo-dl01w1-45-native-sync"
TARGET = "oo-dl01w1-45-native-pg"
OUT = ROOT / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-sync"
PRIVATE = Path("/tmp/oo-w45-native-sync-private")
IMAGE = "starrocks/allin1-ubuntu@sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618"


def main():
    if OUT.exists() or subprocess.run(["docker", "ps", "-aq", "--filter", f"name=^{SOURCE}$"], capture_output=True, check=True).stdout.strip():
        raise SystemExit("fresh source/output required; stopped sources are never restarted")
    OUT.mkdir(parents=True)
    spec = importlib.util.spec_from_file_location("fixture", Path(__file__).with_name("unique_window.py"))
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    fixture.CONTAINER = SOURCE
    fixture.EVIDENCE = OUT
    fixture.SECRET_DIR = PRIVATE
    PRIVATE.mkdir(mode=0o700, exist_ok=True)
    os.chmod(PRIVATE, 0o700)
    phase = fixture.phase
    run = fixture.run
    mysql = fixture.mysql
    proc = run(["docker", "run", "-d", "--name", SOURCE, "--platform", "linux/arm64", "--network", f"container:{TARGET}", "--memory", "4g", "--memory-swap", "4g", "--restart", "no", "--pull", "never", "--ulimit", "nofile=65535:65535", IMAGE], timeout=60)
    if phase("source-start", proc):
        return proc.returncode
    deadline = time.monotonic() + 240
    while time.monotonic() < deadline:
        ready = mysql("SELECT 1;\nSHOW BACKENDS;", timeout=20)
        if ready.returncode == 0 and b"true" in ready.stdout.lower():
            phase("source-ready", ready)
            break
        time.sleep(5)
    else:
        phase("source-timeout", ready)
        return 1
    phase("version", mysql("SELECT current_version();\nSELECT @@version_comment;"))
    dba = os.urandom(18).hex()
    reader = os.urandom(18).hex()
    fixture.SECRETS.extend([dba, reader])
    statements = "CREATE DATABASE oo_w45_sr;\n" + f"CREATE USER 'oo_w45_dba'@'%' IDENTIFIED BY '{dba}';\nCREATE USER 'oo_w45_reader'@'%' IDENTIFIED BY '{reader}';\n" + "GRANT SELECT ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_reader'@'%';\nGRANT SELECT, INSERT, UPDATE, DELETE, ALTER, DROP ON ALL TABLES IN DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';\nGRANT CREATE TABLE ON DATABASE oo_w45_sr TO USER 'oo_w45_dba'@'%';"
    if phase("fixture-identities", mysql(statements)):
        return 1
    for role, password in (("dba", dba), ("reader", reader)):
        fixture.write_secret(f"{role}.cnf", f"[client]\nuser=oo_w45_{role}\npassword={password}\nhost=127.0.0.1\nport=9030\nprotocol=TCP\n")
        fixture.write_secret(f"{role}.password", password)
    run(["docker", "exec", SOURCE, "mkdir", "-p", "/tmp/oo-secrets"])
    for role in ("dba", "reader"):
        run(["docker", "cp", str(PRIVATE / f"{role}.cnf"), f"{SOURCE}:/tmp/oo-secrets/{role}.cnf"])
    if phase("fixture-load", mysql(fixture.LOAD_SQL.read_text(), "/tmp/oo-secrets/dba.cnf", timeout=180)):
        return 1
    phase("source-grants", mysql("SHOW GRANTS FOR 'oo_w45_reader'@'%';"))
    phase("cpu", run(["docker", "exec", SOURCE, "cat", "/sys/fs/cgroup/cpu.max"]))
    (OUT / "source-ready.txt").write_text(f"{SOURCE}\nSource fixture only, no product import/publish acceptance.\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
