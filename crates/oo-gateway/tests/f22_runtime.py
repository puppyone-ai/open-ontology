#!/usr/bin/env python3
"""Root-only owned fixture hook. No arbitrary SQL, endpoints or public access."""
import json
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

BIN = Path("/opt/homebrew/opt/postgresql@17/bin")

def require(condition, message="owned runtime validation failed"):
    if not condition:
        raise SystemExit(message)

def command(pid):
    return subprocess.run(["ps", "-p", str(pid), "-o", "command="],
                          text=True, capture_output=True).stdout.strip()

def load(path):
    runtime_path = Path(path).resolve()
    runtime = json.loads(runtime_path.read_text())
    require(runtime["protocol"] == "oo.f22.owned-runtime/v1")
    fixture = json.loads(Path(runtime["fixture"]).read_text())
    root = Path(fixture["work"]).resolve()
    require(root.name.startswith("f22-owned-") and runtime_path == root / "runtime.json")
    require(fixture["owned_fixture"] == "f22-owned/v1")
    require(fixture["host"] == "127.0.0.1" and fixture["database"] == "f22_owned")
    return runtime, fixture, root

def main():
    if len(sys.argv) < 3:
        raise SystemExit("start <evidence-dir> | cli <runtime.json> <oo args> | status|data-only|revoke|stop <runtime.json> [integer-amount]")
    action, path = sys.argv[1:3]
    if action == "start":
        return subprocess.call([sys.executable, str(Path(__file__).with_name("f22_owned_pg.py")),
                                str(Path(path).resolve()), "--handoff"])
    runtime, fixture, root = load(path)
    if action == "cli":
        require(len(sys.argv) > 3, "CLI command required")
        binary = Path(runtime["cli_binary"])
        require(hashlib.sha256(binary.read_bytes()).hexdigest() == runtime["cli_binary_sha256"],
                "frozen CLI binary changed")
        return subprocess.call([str(binary), *sys.argv[3:], *runtime["connection_args"]])
    pg = root / "pg"
    pg_alive = str(pg) in command(fixture["postmaster_pid"])
    gw_alive = str(root) in command(runtime["gateway_pid"]) and "oo-gateway" in command(runtime["gateway_pid"])
    if action == "status":
        print(json.dumps({"pg_running":pg_alive,"gateway_running":gw_alive,
                          "origin":runtime["origin"],"runtime":str(root / "runtime.json")}))
        return 0
    if action == "stop":
        if gw_alive:
            os.kill(runtime["gateway_pid"], signal.SIGTERM)
            for _ in range(50):
                if str(root) not in command(runtime["gateway_pid"]):
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError("owned Gateway did not stop; no unrelated process was killed")
        if pg_alive:
            subprocess.run([str(BIN / "pg_ctl"), "-D", str(pg), "-m", "fast", "-w", "stop"], check=True)
        (root / "root-cleanup.json").write_text(json.dumps({"pg_stopped":True,"gateway_stopped":True})+"\n")
        print(json.dumps({"stopped":True,"evidence_retained":str(root)}))
        return 0
    require(pg_alive and gw_alive, "owned runtime must be running")
    require(int((pg / "postmaster.pid").read_text().splitlines()[0]) == fixture["postmaster_pid"])
    table, column = "pt_" + "a"*60, "pc_" + "b"*60
    if action == "data-only":
        require(len(sys.argv) == 4, "integer amount required")
        amount = int(sys.argv[3])
        require(-(2**63) <= amount < 2**63, "int8 amount required")
        sql = f"""BEGIN;
            UPDATE _oo.{table} SET {column}={amount};
            UPDATE _oo.table_freshness SET target_generation=target_generation+1,applied_at=clock_timestamp();
            UPDATE oo_publish_target.fence SET opened_version='root-data-'||(SELECT target_generation::text FROM _oo.table_freshness);
            SELECT json_build_object('action','data-only','amount',{amount},
                'target_generation',target_generation,'knowledge_unchanged',true,'policy_unchanged',true)
                FROM _oo.table_freshness;
            COMMIT;"""
    elif action == "revoke":
        require(len(sys.argv) == 3)
        sql = f"REVOKE SELECT ({column}) ON _oo.{table} FROM oo_r_f22; SELECT '{{\"action\":\"revoke\"}}';"
    else:
        raise SystemExit("unsupported action; arbitrary SQL is not accepted")
    env = dict(os.environ, PGPASSWORD=Path(fixture["password_file"]).read_text())
    result = subprocess.run([str(BIN / "psql"), "-X", "-qAt", "-v", "ON_ERROR_STOP=1",
        "-h", "127.0.0.1", "-p", str(fixture["port"]), "-U", "postgres", "-d", "f22_owned"],
        input=sql, text=True, capture_output=True, env=env)
    if result.returncode:
        raise RuntimeError("owned mutation failed; no connection credentials are emitted")
    receipt = json.loads(result.stdout.strip())
    name = root / f"owner-mutation-{time.time_ns()}.json"
    name.write_text(json.dumps(receipt,indent=2)+"\n")
    print(json.dumps({"receipt":str(name),**receipt}))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
