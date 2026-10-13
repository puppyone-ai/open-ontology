#!/bin/bash
# Real builder and trusted consumer against the retained oo-dl01w1-41 seal.
# Does not drop Postgres, does not reimport, and does not rebuild pg-builder.
# A fake upstream test is recorded and is not the product proof.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
SEAL="$EVIDENCE/raw/seal-retry"
OUT="${CLOSURE_OUT:-$EVIDENCE/raw/framing-consume}"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
DECODED="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/probe/decoded.json"
TOKEN_FILE="/tmp/oo-customer-demo-20261007/llm-token"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
OWNER="23df6145-4592-4dc0-82df-fd226dd020fd"
CURRENT="c0017148-4b75-465a-a7d1-efb1d03dfb4c"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-consumer"
PG="${PREFIX}-pg"
MONGO="${PREFIX}-mongodb"
NET="${PREFIX}-net"
OPBIN="/target/w21-native/debug/operator-preview"
PGBIN="/target/pg/debug/pg-builder"

phase() {
  mkdir -p "$OUT"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$OUT/phase.txt"
}

cleanup() {
  local ec=$?
  phase "cleanup $ec"
  docker logs "$RUST" > "$OUT/docker-rust.log" 2>&1 || true
  docker logs "$PG" > "$OUT/docker-pg.log" 2>&1 || true
  docker rm -f "$RUST" > "$OUT/docker-rm-rust.txt" 2>&1 || true
  python3 "$ADMIT" release rust > "$OUT/release-rust.json" 2>&1 || true
  docker stop "$MONGO" > "$OUT/docker-stop-mongo.txt" 2>&1 || true
  if [[ -f "$OUT/product-outcome.txt" ]] && grep -q '^source_author_complete$' "$OUT/product-outcome.txt"; then
    {
      echo "sealed_target_retained"
      echo "postgres_container_kept"
      echo "postgres_volume_kept"
      echo "mongo_container_kept_stopped_after_reopen"
      echo "mongo_volume_kept"
      echo "network_kept"
      echo "secrets_kept_gitexcluded"
      echo "cargo_and_target_volumes_kept"
      echo "rust_container_removed"
      echo "not_reimported"
    } > "$OUT/resource-retained.txt"
  else
    {
      echo "sealed_target_retained_consumer_unfinished"
      echo "postgres_container_kept"
      echo "not_reimported"
    } > "$OUT/resource-retained.txt"
  fi
  docker ps -a --filter "name=${PREFIX}" --format '{{.Names}} {{.Status}}' >> "$OUT/resource-retained.txt" || true
  docker volume ls --filter "name=${PREFIX}" --format '{{.Name}}' >> "$OUT/resource-retained.txt" || true
  python3 - "$OUT" "$SECRETS" "$TOKEN_FILE" << 'PY'
import sys
from pathlib import Path
out = Path(sys.argv[1])
secrets = []
secret_dir = Path(sys.argv[2])
if secret_dir.is_dir():
    for path in secret_dir.iterdir():
        if path.is_file():
            secrets.append(path.read_bytes().strip())
token = Path(sys.argv[3])
if token.exists():
    secrets.append(token.read_bytes().strip())
secrets = [item for item in secrets if item]
for path in out.rglob("*"):
    if not path.is_file() or path.stat().st_size > 20_000_000:
        continue
    data = path.read_bytes()
    changed = False
    for secret in secrets:
        if secret in data:
            data = data.replace(secret, b"REDACTED")
            changed = True
    if changed:
        path.write_bytes(data)
PY
  printf '%s\n' "$ec" > "$OUT/status.txt"
}
trap cleanup EXIT

if [[ -e "$OUT" ]]; then
  printf '%s FAIL %s already exists\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$OUT" >&2
  exit 1
fi
phase "prepare"
mkdir -p "$OUT/builder" "$OUT/operator" "$OUT/unit" "$OUT/plan"
test -f "$TOKEN_FILE"
test -f "$DECODED"
test -f "$SEAL/sealed-catalog.json"
test -f "$SEAL/physical-mappings.json"
test -d "$SECRETS"
docker inspect -f '{{.State.Status}}' "$PG" | grep -qx running || { phase "FAIL sealed postgres is not running"; exit 1; }
cp "$SEAL/sealed-catalog.json" "$OUT/sealed-catalog.json"
cp "$SEAL/physical-mappings.json" "$OUT/physical-mappings.json"
cp "$SEAL/customer-input.json" "$OUT/customer-input.json"
cp "$SEAL/builder/config-401.json" "$OUT/builder/config-401.json"
cp "$SEAL/builder/config-pack.json" "$OUT/builder/config-pack.json"
cp "$SEAL/builder/config-unknown.json" "$OUT/builder/config-unknown.json"
cp "$SEAL/builder/settings.json" "$OUT/builder/settings.json"
cp "$SEAL/builder/visibility.json" "$OUT/builder/visibility.json"
cp "$SEAL/registry.json" "$OUT/registry.json"
cp "$SEAL/registry.sha256" "$OUT/registry.sha256"
python3 - "$OUT" "$SEAL" "$CURRENT" "$OWNER" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
current = sys.argv[3]
owner = sys.argv[4]
catalog = json.loads((out / "sealed-catalog.json").read_text())
mappings = json.loads((out / "physical-mappings.json").read_text())
table = next(row["physical_name"] for row in mappings if row["level"] == "t")
(out / "plan/physical-table.txt").write_text(table + "\n")
(out / "identity.json").write_text(json.dumps({
    "protocol": "oo.source-product-identity/v1",
    "current_cli_uuid": current,
    "historical_capturer_uuid": owner,
    "historical_evidence_uuid_is_not_this_cli": True,
    "sealed_run_id": catalog.get("run_id"),
    "source_id": catalog.get("source_id"),
    "input_digest": catalog.get("input_digest"),
    "schema_digest": catalog.get("schema_digest"),
    "physical_table": table,
    "seal_evidence": "raw/seal-retry",
    "preserved_http0_timeout": "raw/consumer-closure/builder/pack/result.json",
    "preserved_upstream_fin_499": "raw/seal-retry/builder/pack/result.json",
    "reimport": False,
}, indent=2) + "\n")
PY
table="$(tr -d '[:space:]' < "$OUT/plan/physical-table.txt")"
rows="$(docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"")"
printf '%s\n' "$rows" > "$OUT/plan/sealed-row-count.txt"
[[ "$rows" == "16" ]] || { phase "FAIL sealed row count $rows"; exit 1; }
phase "sealed rows 16 retained"

phase "admit rust"
admitted=0
for attempt in $(seq 1 30); do
  if python3 "$ADMIT" acquire rust "$OUT/admission-rust.json"; then
    admitted=1
    break
  fi
  phase "rust slot busy attempt $attempt"
  sleep 30
done
[[ "$admitted" -eq 1 ]] || { phase "FAIL rust slot"; exit 1; }

ids="$(docker ps -q)"
cpus=0
if [[ -n "$ids" ]]; then
  cpus="$(docker inspect -f '{{.HostConfig.NanoCpus}}' $ids | awk '{s+=$1} END {print s+0}')"
fi
if [[ "$cpus" -gt 15000000000 ]]; then
  phase "FAIL cpu cap before rust"
  exit 1
fi

phase "container"
docker rm -f "$RUST" >/dev/null 2>&1 || true
docker run -d --name "$RUST" \
  --platform linux/arm64 --pull never \
  --memory 6g --cpus 1 --pids-limit 1024 \
  --network "$NET" \
  --add-host host.docker.internal:host-gateway \
  --label owner=dl01-w1-41 \
  --label session="$OWNER" \
  --label project=open-ontology \
  --label task=DL01-W1-41 \
  --label kind=framing-consumer \
  -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_BUILD_JOBS=1 \
  -v "${PREFIX}-target:/target" \
  -v "$ROOT/tools/mongodb-preview:/work/tools/mongodb-preview:ro" \
  -v "$OUT:/evidence" \
  -v "$SECRETS:/secrets:ro" \
  -v "$TOKEN_FILE:/secrets/llm-token:ro" \
  "$RUST_IMAGE" \
  sleep infinity > "$OUT/docker-run.txt" || { phase "FAIL docker run"; exit 1; }
docker exec "$RUST" rustc --version > "$OUT/unit/rustc.txt"
grep -q '^rustc 1.94.0 ' "$OUT/unit/rustc.txt" || { phase "FAIL rustc"; exit 1; }
phase "unit framing"
docker exec "$RUST" rustc --test /work/tools/mongodb-preview/scripts/loopback-proxy.rs -O -o /tmp/loopback-proxy-test > "$OUT/unit/rustc-test.log" 2>&1
echo $? > "$OUT/unit/rustc-test.exit"
[[ "$(cat "$OUT/unit/rustc-test.exit")" == "0" ]] || { phase "FAIL unit compile"; exit 1; }
docker exec "$RUST" /tmp/loopback-proxy-test --test-threads=1 > "$OUT/unit/framing.stdout" 2> "$OUT/unit/framing.stderr"
echo $? > "$OUT/unit/framing.exit"
[[ "$(cat "$OUT/unit/framing.exit")" == "0" ]] || { phase "FAIL unit"; exit 1; }
phase "proxy compile"
docker exec "$RUST" rustc /work/tools/mongodb-preview/scripts/loopback-proxy.rs -O -o /target/proxy/loopback-proxy > "$OUT/unit/proxy-compile.log" 2>&1
echo $? > "$OUT/unit/proxy-compile.exit"
[[ "$(cat "$OUT/unit/proxy-compile.exit")" == "0" ]] || { phase "FAIL proxy compile"; exit 1; }
docker exec "$RUST" test -x "$PGBIN" || { phase "FAIL pg-builder binary"; exit 1; }
docker exec "$RUST" test -x "$OPBIN" || { phase "FAIL operator binary"; exit 1; }
docker exec "$RUST" sha256sum "$PGBIN" "$OPBIN" /target/proxy/loopback-proxy > "$OUT/unit/bins.sha256"
phase "binaries ready"

start_proxy() {
  docker exec "$RUST" bash -c 'if [ -f /tmp/proxy.pid ]; then kill "$(cat /tmp/proxy.pid)" 2>/dev/null || true; rm -f /tmp/proxy.pid; fi; sleep 0.2'
  docker exec -d -e OO_PROXY_MODE="$1" -e OO_UPSTREAM=host.docker.internal:8317 "$RUST" \
    bash -c 'echo $$ > /tmp/proxy.pid; exec /target/proxy/loopback-proxy >> /evidence/builder/proxy.log 2>&1'
  sleep 0.5
}

run_sealed() {
  local name="$1" config="$2" cache="$3"
  mkdir -p "$OUT/builder/$name" "$OUT/builder/$cache"
  docker exec "$RUST" "$PGBIN" sealed \
    --config "/evidence/builder/$config" \
    --catalog /evidence/sealed-catalog.json \
    --settings /evidence/builder/settings.json \
    --visibility /evidence/builder/visibility.json \
    --cache "/evidence/builder/$cache" \
    --out "/evidence/builder/$name" \
    --reviewer dl01-w1-41-operator \
    --reviewed-at 2026-10-11 \
    --dataset oo_w141.shipments \
    --target-host oo-dl01w1-41-pg \
    --target-port 5432 \
    --target-database oo_w141 \
    --target-user oo_w141_admin \
    --login oo_w141_admin \
    --target-ca /secrets/ca.crt \
    --target-password /secrets/admin-password \
    > "$OUT/builder/$name/sealed.invoke" 2>&1
  echo $? > "$OUT/builder/$name/sealed.exit"
}

cat > "$OUT/builder/preserved-failures.txt" << 'EOF'
raw/consumer-closure/builder/pack is the preserved 180000 deadline timeout. model called, socket opened, http_status 0, pack not_produced. That deadline was not raised and that run was not repeated.
raw/seal-retry/builder/pack is the preserved upstream FIN. The forwarder shut the upstream write side, the product proxy returned HTTP 499 context canceled in about 6ms, and no pack was stored. declared_non_billing USD 0.000001 remains an estimate, not a bill.
This run keeps that estimate and the 180000 deadline. The unit test is not this proof.
EOF

phase "builder 401"
start_proxy 401
run_sealed probe-401 config-401.json cache-401
phase "builder pack"
start_proxy forward
run_sealed pack config-pack.json cache-pack
python3 - "$OUT/builder/pack/result.json" > "$OUT/builder/pack-outcome.txt" << 'PY'
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
if not path.exists():
    print("missing")
    raise SystemExit
doc = json.loads(path.read_text())
print("|".join([str(doc.get("outcome")), str(doc.get("reason")), str(doc.get("pack")), str(doc.get("model")), str(doc.get("http_status"))]))
PY
if [[ -f "$OUT/builder/pack/authority.json" ]]; then
  phase "builder cache"
  run_sealed cache-hit config-pack.json cache-pack
fi
pack_outcome="$(cat "$OUT/builder/pack-outcome.txt")"
case "$pack_outcome" in
  *timeout*)
    printf '%s\n' "cost_unknown was not called. The pack attempt timed out." > "$OUT/builder/cost-unknown-skipped.txt"
    ;;
  missing|*not_produced*)
    if [[ "$pack_outcome" == packed\|*\|produced\|* ]]; then
      phase "builder cost_unknown"
      run_sealed cost-unknown config-unknown.json cache-unknown
    else
      printf '%s\n' "cost_unknown was not called. The pack attempt did not return a model completion that produced a pack." > "$OUT/builder/cost-unknown-skipped.txt"
    fi
    ;;
  *)
    phase "builder cost_unknown"
    run_sealed cost-unknown config-unknown.json cache-unknown
    ;;
esac

phase "operator"
docker exec "$RUST" "$OPBIN" --help > "$OUT/operator/help.stdout" 2> "$OUT/operator/help.stderr" || true
echo $? > "$OUT/operator/help.exit"
if [[ -f "$OUT/builder/pack/authority.json" ]]; then
  python3 - "$OUT" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
mappings = json.loads((out / "physical-mappings.json").read_text())
allowed = ["_id","code","flag","units","price","created","uid","blob","blob_presence","weight","warehouse","lane","memo","memo_presence","marks","marks_presence","pack","pack_presence"]
columns = [row for row in mappings if row["level"] == "c" and row["logical_column"] in allowed]
table = next(row["physical_name"] for row in mappings if row["level"] == "t")
if len(columns) != len(allowed):
    raise SystemExit(f"mapped {len(columns)}")
quoted = ", ".join(f'"{row["physical_name"]}"' for row in columns)
sql = (
    "DO $$\nBEGIN\n"
    "  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_r_w141_reader') THEN\n"
    "    CREATE ROLE oo_r_w141_reader NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOLOGIN NOBYPASSRLS NOREPLICATION;\n"
    "  END IF;\nEND\n$$;\n"
    "GRANT USAGE ON SCHEMA \"_oo\" TO oo_r_w141_reader;\n"
    f"GRANT SELECT ({quoted}) ON TABLE \"_oo\".\"{table}\" TO oo_r_w141_reader;\n"
)
(out / "operator/grants.sql").write_text(sql)
(out / "operator/physical-table.txt").write_text(table + "\n")
query = {
    "protocol": "oo.operator-preview.query/v1",
    "sql": "SELECT _id, code, flag, units, price, created, uid, blob, blob_presence, weight, warehouse, lane, memo, memo_presence, marks, marks_presence, pack, pack_presence FROM oo_w141.shipments ORDER BY code",
    "max_rows": 16,
    "max_result_bytes": 200000,
    "wall_clock_ms": 20000,
}
(out / "operator/query.json").write_text(json.dumps(query, indent=2) + "\n")
deny = dict(query)
deny["sql"] = "UPDATE oo_w141.shipments SET code = code"
(out / "operator/deny-query.json").write_text(json.dumps(deny, indent=2) + "\n")
hidden = dict(query)
hidden["sql"] = "SELECT whCode FROM oo_w141.shipments"
(out / "operator/hidden-query.json").write_text(json.dumps(hidden, indent=2) + "\n")
widen = dict(query)
widen["sql"] = "SELECT code, whCode FROM oo_w141.shipments"
(out / "operator/widen-query.json").write_text(json.dumps(widen, indent=2) + "\n")
(out / "operator/cancel-query.json").write_text(json.dumps(query, indent=2) + "\n")
PY
  docker exec -i "$PG" psql -U oo_w141_admin -d oo_w141 -v ON_ERROR_STOP=1 -f - < "$OUT/operator/grants.sql" > "$OUT/operator/grants.out" 2> "$OUT/operator/grants.err"
  echo $? > "$OUT/operator/grants.exit"
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"" > "$OUT/operator/rows-before.txt"
  docker exec "$RUST" "$OPBIN" describe --authority /evidence/builder/pack/authority.json > "$OUT/operator/describe.stdout" 2> "$OUT/operator/describe.stderr"
  echo $? > "$OUT/operator/describe.exit"
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/query.json > "$OUT/operator/query.stdout" 2> "$OUT/operator/query.stderr"
  echo $? > "$OUT/operator/query.exit"
  if [[ "$(cat "$OUT/operator/query.exit")" == "0" ]]; then
    python3 "$ROOT/tools/mongodb-preview/scripts/consumer-oracle.py" "$OUT/operator/query.stdout" "$DECODED" > "$OUT/operator/query-oracle.out"
    echo $? > "$OUT/operator/query-oracle.exit"
  else
    printf '%s\n' "query did not exit 0" > "$OUT/operator/query-oracle.out"
    echo 1 > "$OUT/operator/query-oracle.exit"
  fi
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/deny-query.json > "$OUT/operator/deny.stdout" 2> "$OUT/operator/deny.stderr"
  echo $? > "$OUT/operator/deny.exit"
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/hidden-query.json > "$OUT/operator/hidden.stdout" 2> "$OUT/operator/hidden.stderr"
  echo $? > "$OUT/operator/hidden.exit"
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/widen-query.json > "$OUT/operator/widen.stdout" 2> "$OUT/operator/widen.stderr"
  echo $? > "$OUT/operator/widen.exit"
  openssl rand -hex 24 > "$SECRETS/nobody-password"
  chmod 600 "$SECRETS/nobody-password"
  python3 - "$SECRETS/nobody-password" "$SECRETS/create-nobody.sql" << 'PY'
import sys
from pathlib import Path
password = Path(sys.argv[1]).read_text().strip().replace("'", "''")
Path(sys.argv[2]).write_text(
    "DO $$\nBEGIN\n"
    "  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_w141_nobody') THEN\n"
    "    CREATE ROLE oo_w141_nobody LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '"
    + password
    + "';\n"
    "  END IF;\nEND\n$$;\n"
)
PY
  docker cp "$SECRETS/create-nobody.sql" "$PG:/tmp/create-nobody.sql"
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -v ON_ERROR_STOP=1 -f /tmp/create-nobody.sql > "$OUT/operator/nobody-create.out" 2> "$OUT/operator/nobody-create.err"
  echo $? > "$OUT/operator/nobody-create.exit"
  docker exec "$PG" rm -f /tmp/create-nobody.sql
  docker cp "$SECRETS/nobody-password" "$PG:/tmp/nobody-password"
  docker exec "$PG" bash -c "export PGPASSWORD=\$(cat /tmp/nobody-password); rm -f /tmp/nobody-password; psql -U oo_w141_nobody -d oo_w141 -c 'SELECT count(*) FROM \"_oo\".\"${table}\"'" > "$OUT/operator/nobody-select.out" 2> "$OUT/operator/nobody-select.err"
  echo $? > "$OUT/operator/nobody-select.exit"
  rm -f "$SECRETS/create-nobody.sql"
  phase "operator cancel"
  docker exec -d "$PG" psql -U oo_w141_admin -d oo_w141 -c "BEGIN; LOCK TABLE \"_oo\".\"${table}\" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(50);"
  sleep 1
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/cancel-query.json > "$OUT/operator/cancel-query.stdout" 2> "$OUT/operator/cancel-query.stderr" &
  query_pid=$!
  running=""
  active=0
  for _ in $(seq 1 60); do
    running="$(python3 - "$OUT/builder/pack/state/executions" << 'PY'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
if not root.is_dir():
    raise SystemExit
for path in root.glob("*.json"):
    try:
        doc = json.loads(path.read_text())
    except Exception:
        continue
    if doc.get("status") == "running" and doc.get("execution_id"):
        print(doc["execution_id"])
        raise SystemExit
PY
)"
    if [[ -n "$running" ]]; then
      waiting="$(docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM pg_stat_activity WHERE datname='oo_w141' AND state='active' AND wait_event_type='Lock' AND query NOT LIKE 'BEGIN; LOCK TABLE%'")"
      if [[ "$waiting" != "0" && -n "$waiting" ]]; then
        active=1
        break
      fi
    fi
    sleep 0.25
  done
  printf '%s\n' "$running" > "$OUT/operator/cancel-id.txt"
  printf '%s\n' "$active" > "$OUT/operator/cancel-backend-active.txt"
  if [[ -n "$running" && "$active" == "1" ]]; then
    docker exec "$RUST" "$OPBIN" cancel --authority /evidence/builder/pack/authority.json --execution "$running" > "$OUT/operator/cancel.stdout" 2> "$OUT/operator/cancel.stderr"
    echo $? > "$OUT/operator/cancel.exit"
  else
    printf '%s\n' "running execution was not observed" > "$OUT/operator/cancel.stdout"
    echo 10 > "$OUT/operator/cancel.exit"
  fi
  wait "$query_pid" || true
  echo $? > "$OUT/operator/cancel-query.exit"
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE 'BEGIN; LOCK TABLE%' AND pid <> pg_backend_pid();" > "$OUT/operator/cancel-lock-stop.out" 2>&1 || true
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"" > "$OUT/operator/rows-after-cancel.txt"
  phase "source off query"
  docker inspect -f '{{.State.Status}}' "$MONGO" > "$OUT/operator/source-off-status.txt" 2> "$OUT/operator/source-off-inspect.err" || true
  docker exec "$MONGO" mongosh --quiet --eval 'quit(0)' > "$OUT/operator/source-off-ping.out" 2> "$OUT/operator/source-off-ping.err"
  echo $? > "$OUT/operator/source-off-ping.exit"
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/query.json > "$OUT/operator/source-off-query.stdout" 2> "$OUT/operator/source-off-query.stderr"
  echo $? > "$OUT/operator/source-off-query.exit"
  if [[ "$(cat "$OUT/operator/source-off-query.exit")" == "0" ]]; then
    python3 "$ROOT/tools/mongodb-preview/scripts/consumer-oracle.py" "$OUT/operator/source-off-query.stdout" "$DECODED" > "$OUT/operator/source-off-oracle.out"
    echo $? > "$OUT/operator/source-off-oracle.exit"
  fi
  phase "source reopen"
  docker start "$MONGO" > "$OUT/operator/source-reopen.out" 2> "$OUT/operator/source-reopen.err"
  echo $? > "$OUT/operator/source-reopen.exit"
  if [[ "$(cat "$OUT/operator/source-reopen.exit")" == "0" ]]; then
    reopen_ready=0
    for _ in $(seq 1 20); do
      sleep 2
      if docker exec -u 0 -e HOME=/tmp "$MONGO" \
        mongosh --quiet --norc 'mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-reopen' \
        --eval 'const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/dba","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_dba", pw)) quit(2); const n=db.getSiblingDB("oo_w141").shipments.find({}, {code:1}).limit(16).toArray().length; fs.writeFileSync("/tmp/oo-reopen-count.txt", "rows="+n+"\n"); if (n!==16) quit(3);' \
        > "$OUT/operator/source-reopen-count.out" 2> "$OUT/operator/source-reopen-count.err"; then
        reopen_ready=1
        break
      fi
    done
    docker cp "$MONGO:/tmp/oo-reopen-count.txt" "$OUT/operator/source-reopen-count.txt" >/dev/null 2>&1 || true
    docker exec -u 0 "$MONGO" rm -f /tmp/oo-reopen-count.txt >/dev/null 2>&1 || true
    printf '%s\n' "$reopen_ready" > "$OUT/operator/source-reopen-ready.txt"
  fi
else
  printf '%s\n' "operator query was not started because the pack authority is absent" > "$OUT/operator/not-started.txt"
fi

python3 - "$OUT" << 'PY'
import json
from pathlib import Path
out = Path(__import__("sys").argv[1])

def load(rel):
    path = out / rel
    if not path.exists():
        return None
    text = path.read_text().strip()
    if rel.endswith(".json"):
        return json.loads(text)
    return text

def exit_of(rel):
    text = load(rel)
    if text is None or text == "":
        return None
    try:
        return int(text)
    except ValueError:
        return text

pack = load("builder/pack/result.json") or {}
cache = load("builder/cache-hit/result.json") or {}
unknown = load("builder/cost-unknown/result.json") or {}
denied = load("builder/probe-401/result.json") or {}
cancel = {}
raw = load("operator/cancel-query.stdout")
if isinstance(raw, str) and raw.startswith("{"):
    try:
        cancel = json.loads(raw)
    except json.JSONDecodeError:
        cancel = {}
facts = {
    "unit_exit": exit_of("unit/framing.exit"),
    "probe_401_http_status": denied.get("http_status"),
    "probe_401_pack": denied.get("pack"),
    "probe_401_cache_store": denied.get("cache_store"),
    "pack_outcome": pack.get("outcome"),
    "pack_http_status": pack.get("http_status"),
    "pack_model": pack.get("model"),
    "pack_cache": pack.get("cache"),
    "pack_value": pack.get("pack"),
    "cache_model": cache.get("model"),
    "cache_socket": cache.get("socket"),
    "cache_pack": cache.get("pack"),
    "unknown_outcome": unknown.get("outcome"),
    "unknown_pack": unknown.get("pack"),
    "unknown_reason": unknown.get("reason"),
    "query_exit": exit_of("operator/query.exit"),
    "oracle_exit": exit_of("operator/query-oracle.exit"),
    "deny_exit": exit_of("operator/deny.exit"),
    "hidden_exit": exit_of("operator/hidden.exit"),
    "widen_exit": exit_of("operator/widen.exit"),
    "nobody_exit": exit_of("operator/nobody-select.exit"),
    "cancel_exit": exit_of("operator/cancel.exit"),
    "cancel_outcome": cancel.get("outcome"),
    "cancel_sqlstate": cancel.get("sqlstate"),
    "cancel_rollback": cancel.get("rollback_confirmed", cancel.get("rollback")),
    "rows_before": load("operator/rows-before.txt"),
    "rows_after_cancel": load("operator/rows-after-cancel.txt"),
    "source_off_ping_exit": exit_of("operator/source-off-ping.exit"),
    "source_off_query_exit": exit_of("operator/source-off-query.exit"),
    "source_off_oracle_exit": exit_of("operator/source-off-oracle.exit"),
    "reopen_ready": load("operator/source-reopen-ready.txt"),
}
(out / "collected-facts.json").write_text(json.dumps(facts, indent=2) + "\n")
gates = {
    "unit": facts["unit_exit"] == 0,
    "auth_401": facts["probe_401_http_status"] == 401 and facts["probe_401_pack"] == "not_produced",
    "fresh_pack": facts["pack_outcome"] == "packed" and facts["pack_value"] == "produced" and facts["pack_model"] == "called" and facts["pack_http_status"] == 200,
    "cache_hit": facts["cache_model"] == "cache_hit" and facts["cache_socket"] == "not_opened" and facts["cache_pack"] == "produced",
    "cost_unknown": facts["unknown_outcome"] == "cost_unknown" and facts["unknown_pack"] == "not_produced",
    "typed_query": facts["query_exit"] == 0 and facts["oracle_exit"] == 0,
    "deny_update": facts["deny_exit"] not in (None, 0),
    "hidden": facts["hidden_exit"] not in (None, 0),
    "widen": facts["widen_exit"] not in (None, 0),
    "nobody": facts["nobody_exit"] not in (None, 0),
    "cancel_57014": facts["cancel_outcome"] == "canceled" and facts["cancel_sqlstate"] == "57014" and facts["cancel_rollback"] is True and facts["rows_before"] == "16" and facts["rows_after_cancel"] == "16",
    "source_off": facts["source_off_ping_exit"] not in (None, 0) and facts["source_off_query_exit"] == 0 and facts["source_off_oracle_exit"] == 0,
    "reopen": facts["reopen_ready"] == "1",
}
(out / "gates.json").write_text(json.dumps(gates, indent=2) + "\n")
if all(gates.values()):
    (out / "product-outcome.txt").write_text("source_author_complete\n")
else:
    missing = [name for name, ok in gates.items() if not ok]
    (out / "product-outcome.txt").write_text("blocked\n" + "\n".join(missing) + "\n")
PY
phase "recorded"
exit 0
