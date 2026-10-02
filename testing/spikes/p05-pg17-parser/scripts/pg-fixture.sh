#!/bin/bash
# One PG 17.10 fixture for the P5 spike. No published port. No agent utility or DML.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
name=oo-a065-p5-pg1710
image=postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
worker=${P05_WORKER:-}
policy=$root/fixtures/policy.json
controls=$root/fixtures/pg17-controls.json
out=${1:-/tmp/p05-pg-fixture.jsonl}

if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
  echo "refusing: $name already exists" >&2
  exit 2
fi
running=$(docker ps --filter label=oo.role=rust-builder -q | wc -l | tr -d ' ')
echo "running_rust_builders=$running"
if [ "$running" -ge 2 ]; then
  echo "refusing: rust builder capacity is full" >&2
  exit 2
fi

docker run -d \
  --name "$name" \
  --platform linux/arm64 \
  --network none \
  --label oo.owner=a065 \
  --label oo.project=open-ontology \
  --label oo.run_id=eef831aa-dba4-4af2-a78b-541bc9e0a065 \
  --label oo.role=pg-fixture \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --security-opt no-new-privileges \
  -e POSTGRES_PASSWORD=fixture-only-not-a-secret \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  "$image" >/tmp/p05-pg-cid.txt

cid=$(cat /tmp/p05-pg-cid.txt)
echo "pg_cid=$cid"
ready=0
for _ in $(seq 1 60); do
  if docker exec "$name" pg_isready -U postgres -q >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$name" >&2 || true
  exit 3
fi

docker exec -i "$name" psql -U postgres -d postgres -v ON_ERROR_STOP=1 < "$root/fixtures/pg17-setup.sql"

: > "$out"
python3 - "$controls" "$worker" "$policy" "$name" "$out" << 'PY'
import json, os, subprocess, sys
controls_path, worker, policy, name, out_path = sys.argv[1:]
controls = json.loads(open(controls_path).read())
dump = os.path.join(os.path.dirname(controls_path), "..", "scripts", "admit-dump.py")

def psql(role, sql):
    proc = subprocess.run(
        ["docker", "exec", "-e", "PGOPTIONS=-c statement_timeout=5000", "-i", name,
         "psql", "-U", role, "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-At", "-F", "|"],
        input="\\set VERBOSITY verbose\n" + sql + "\n",
        text=True, capture_output=True)
    text = (proc.stdout or "") + (proc.stderr or "")
    return proc.returncode, text.strip()

def record(row):
    with open(out_path, "a") as fh:
        fh.write(json.dumps(row, ensure_ascii=False) + "\n")

for case in controls:
    rc, text = psql(case["role"], case["sql"])
    row = {"id": case["id"], "role": case["role"], "rc": rc, "sql": case["sql"], "out": text}
    if case.get("dump") and worker:
        proc = subprocess.run(
            ["python3", dump, worker, policy, case["sql"]],
            text=True, capture_output=True)
        if proc.returncode != 0:
            row["parser_error"] = (proc.stderr or proc.stdout).strip()
        else:
            decision = json.loads(proc.stdout)
            keep = {k: decision.get(k) for k in (
                "syntax_allow", "code", "parser_version", "original_projection_star",
                "executed_sql", "detail", "oid_acl_rls_proven")}
            row["parser"] = keep
            executed = decision.get("executed_sql") or ""
            if decision.get("syntax_allow") and executed:
                drc, dout = psql("agent_reader", executed)
                row["deparsed_rc"] = drc
                row["deparsed_out"] = dout
    record(row)
    print(f"{case['id']} rc={rc}", flush=True)
PY

docker inspect -f 'status={{.State.Status}} net={{.HostConfig.NetworkMode}} ports={{json .NetworkSettings.Ports}} priv={{.HostConfig.Privileged}} mem={{.HostConfig.Memory}} swap={{.HostConfig.MemorySwap}} cpus={{.HostConfig.NanoCpus}} pids={{.HostConfig.PidsLimit}} nnp={{json .HostConfig.SecurityOpt}}' "$name"
echo "fixture_rows=$(wc -l < "$out")"
