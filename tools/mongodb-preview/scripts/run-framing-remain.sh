#!/bin/bash
# Continuation after the retained seal and the framing-consume pack.
# Does not reimport, does not drop Postgres, and does not repeat the fresh pack call.
# The packed result omits http_status. The proxy log is the HTTP 200 record.
# A filled audit segment admits one statement. Later statements need a new segment.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$EVIDENCE/raw/framing-consume"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
DECODED="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/probe/decoded.json"
TOKEN_FILE="/tmp/oo-customer-demo-20261007/llm-token"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
OWNER="23df6145-4592-4dc0-82df-fd226dd020fd"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-consumer"
PG="${PREFIX}-pg"
MONGO="${PREFIX}-mongodb"
NET="${PREFIX}-net"
OPBIN="/target/w21-native/debug/operator-preview"
PGBIN="/target/pg/debug/pg-builder"
REMAIN="$OUT/remain"

phase() {
  mkdir -p "$REMAIN"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$REMAIN/phase.txt"
}

cleanup() {
  local ec=$?
  phase "cleanup $ec"
  docker logs "$RUST" > "$REMAIN/docker-rust.log" 2>&1 || true
  docker rm -f "$RUST" > "$REMAIN/docker-rm-rust.txt" 2>&1 || true
  python3 "$ADMIT" release rust > "$REMAIN/release-rust.json" 2>&1 || true
  docker stop "$MONGO" > "$REMAIN/docker-stop-mongo.txt" 2>&1 || true
  docker ps -a --filter "name=${PREFIX}" --format '{{.Names}} {{.Status}}' > "$REMAIN/resource-retained.txt" || true
  docker volume ls --filter "name=${PREFIX}" --format '{{.Name}}' >> "$REMAIN/resource-retained.txt" || true
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
  printf '%s\n' "$ec" > "$REMAIN/status.txt"
}
trap cleanup EXIT

if [[ -e "$REMAIN" ]]; then
  printf '%s FAIL remain already exists\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >&2
  exit 1
fi
[[ -f "$OUT/builder/pack/authority.json" ]] || { phase "FAIL pack authority missing"; exit 1; }
[[ -f "$OUT/product-outcome-first.txt" ]] || cp "$OUT/product-outcome.txt" "$OUT/product-outcome-first.txt"
docker inspect -f '{{.State.Status}}' "$PG" | grep -qx running || { phase "FAIL sealed postgres is not running"; exit 1; }
docker inspect -f '{{.State.Status}}' "$MONGO" | grep -qx exited || { phase "FAIL mongo is not the retained stopped source"; exit 1; }
python3 - << 'PY'
import socket
socket.create_connection(("127.0.0.1", 8317), 2).close()
PY
phase "prepare"
table="$(tr -d '[:space:]' < "$OUT/plan/physical-table.txt")"
rows="$(docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"")"
printf '%s\n' "$rows" > "$REMAIN/sealed-row-count.txt"
[[ "$rows" == "16" ]] || { phase "FAIL sealed row count $rows"; exit 1; }

phase "admit rust"
admitted=0
for attempt in $(seq 1 30); do
  if python3 "$ADMIT" acquire rust "$REMAIN/admission-rust.json"; then
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
  cpus="$(docker inspect -f '{{.HostConfig.NanoCpus}}' $ids 2>/dev/null | awk '{s+=$1} END {print s+0}')"
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
  --label kind=framing-remain \
  -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_BUILD_JOBS=1 \
  -v "${PREFIX}-target:/target" \
  -v "$ROOT/tools/mongodb-preview:/work/tools/mongodb-preview:ro" \
  -v "$OUT:/evidence" \
  -v "$SECRETS:/secrets:ro" \
  -v "$TOKEN_FILE:/secrets/llm-token:ro" \
  "$RUST_IMAGE" \
  sleep infinity > "$REMAIN/docker-run.txt" || { phase "FAIL docker run"; exit 1; }
docker exec "$RUST" rustc --version > "$REMAIN/rustc.txt"
grep -q '^rustc 1.94.0 ' "$REMAIN/rustc.txt" || { phase "FAIL rustc"; exit 1; }
docker exec "$RUST" test -x "$PGBIN" || { phase "FAIL pg-builder binary"; exit 1; }
docker exec "$RUST" test -x "$OPBIN" || { phase "FAIL operator binary"; exit 1; }
docker exec "$RUST" test -x /target/proxy/loopback-proxy || { phase "FAIL proxy binary"; exit 1; }

python3 - "$OUT/builder/config-unknown.json" "$OUT/builder/config-unknown-price.json" << 'PY'
import json, sys
from pathlib import Path
doc = json.loads(Path(sys.argv[1]).read_text())
if doc["budget"]["deadline_ms"] != 180000 or doc["declared_price"]["status"] != "unknown":
    raise SystemExit("unknown config identity drifted")
doc["budget"]["max_completion_tokens"] = 16000
doc["budget"]["max_total_tokens"] = 24000
Path(sys.argv[2]).write_text(json.dumps(doc, indent=2) + "\n")
PY
cat > "$REMAIN/token-cap-note.txt" << 'EOF'
builder/cost-unknown is the preserved call. The proxy recorded HTTP 200, stop=chunked, and client shutdown in the same millisecond. pg-builder then refused budget_failure reason tokens and stored no pack. That result omits the usage counters. The visible completion is 1956 bytes, under the successful pack, so the cap was crossed by the reported token count rather than by a missing response. config-unknown-price.json keeps deadline_ms 180000 and declared_price status unknown. It raises only max_completion_tokens to 16000 and max_total_tokens to 24000, inside the builder bounds of 100000 and 200000, so the price gate can be reached. declared_non_billing on the pack config is unchanged.
EOF

start_proxy() {
  docker exec "$RUST" bash -c 'if [ -f /tmp/proxy.pid ]; then kill "$(cat /tmp/proxy.pid)" 2>/dev/null || true; rm -f /tmp/proxy.pid; fi; sleep 0.2'
  docker exec -d -e OO_PROXY_MODE="$1" -e OO_UPSTREAM=host.docker.internal:8317 "$RUST" \
    bash -c 'echo $$ > /tmp/proxy.pid; exec /target/proxy/loopback-proxy >> /evidence/builder/proxy.log 2>&1'
  sleep 0.5
}

phase "builder cost_unknown price"
start_proxy forward
mkdir -p "$OUT/builder/cost-unknown-price" "$OUT/builder/cache-unknown-price"
docker exec "$RUST" "$PGBIN" sealed \
  --config /evidence/builder/config-unknown-price.json \
  --catalog /evidence/sealed-catalog.json \
  --settings /evidence/builder/settings.json \
  --visibility /evidence/builder/visibility.json \
  --cache /evidence/builder/cache-unknown-price \
  --out /evidence/builder/cost-unknown-price \
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
  > "$OUT/builder/cost-unknown-price/sealed.invoke" 2>&1
echo $? > "$OUT/builder/cost-unknown-price/sealed.exit"

archive_audit() {
  local name="$1"
  mkdir -p "$OUT/operator/audit-segments/$name"
  cp -a "$OUT/builder/pack/audit/." "$OUT/operator/audit-segments/$name/"
  rm -f "$OUT/builder/pack/audit"/seg-*.wal
}

phase "operator cancel"
mkdir -p "$OUT/operator/remain"
archive_audit after-typed-query
docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"" > "$OUT/operator/remain/rows-before-cancel.txt"
docker exec -d "$PG" psql -U oo_w141_admin -d oo_w141 -c "BEGIN; LOCK TABLE \"_oo\".\"${table}\" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(50);"
sleep 1
docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/cancel-query.json > "$OUT/operator/remain/cancel-query.stdout" 2> "$OUT/operator/remain/cancel-query.stderr" &
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
printf '%s\n' "$running" > "$OUT/operator/remain/cancel-id.txt"
printf '%s\n' "$active" > "$OUT/operator/remain/cancel-backend-active.txt"
if [[ -n "$running" && "$active" == "1" ]]; then
  docker exec "$RUST" "$OPBIN" cancel --authority /evidence/builder/pack/authority.json --execution "$running" > "$OUT/operator/remain/cancel.stdout" 2> "$OUT/operator/remain/cancel.stderr"
  echo $? > "$OUT/operator/remain/cancel.exit"
else
  printf '%s\n' "running execution was not observed" > "$OUT/operator/remain/cancel.stdout"
  echo 10 > "$OUT/operator/remain/cancel.exit"
fi
wait "$query_pid" || true
echo $? > "$OUT/operator/remain/cancel-query.exit"
docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE 'BEGIN; LOCK TABLE%' AND pid <> pg_backend_pid();" > "$OUT/operator/remain/cancel-lock-stop.out" 2>&1 || true
docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM \"_oo\".\"${table}\"" > "$OUT/operator/remain/rows-after-cancel.txt"

phase "source off query"
archive_audit after-cancel
docker stop "$MONGO" > "$OUT/operator/remain/source-off-stop.out" 2> "$OUT/operator/remain/source-off-stop.err" || true
docker inspect -f '{{.State.Status}}' "$MONGO" > "$OUT/operator/remain/source-off-status.txt"
docker exec "$MONGO" mongosh --quiet --eval 'quit(0)' > "$OUT/operator/remain/source-off-ping.out" 2> "$OUT/operator/remain/source-off-ping.err" || true
echo $? > "$OUT/operator/remain/source-off-ping.exit"
docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/query.json > "$OUT/operator/remain/source-off-query.stdout" 2> "$OUT/operator/remain/source-off-query.stderr"
echo $? > "$OUT/operator/remain/source-off-query.exit"
if [[ "$(cat "$OUT/operator/remain/source-off-query.exit")" == "0" ]]; then
  python3 "$ROOT/tools/mongodb-preview/scripts/consumer-oracle.py" "$OUT/operator/remain/source-off-query.stdout" "$DECODED" > "$OUT/operator/remain/source-off-oracle.out"
  echo $? > "$OUT/operator/remain/source-off-oracle.exit"
else
  echo 1 > "$OUT/operator/remain/source-off-oracle.exit"
fi

phase "source reopen"
docker start "$MONGO" > "$OUT/operator/remain/source-reopen.out" 2> "$OUT/operator/remain/source-reopen.err"
echo $? > "$OUT/operator/remain/source-reopen.exit"
reopen_ready=0
if [[ "$(cat "$OUT/operator/remain/source-reopen.exit")" == "0" ]]; then
  for _ in $(seq 1 20); do
    sleep 2
    if docker exec -u 0 -e HOME=/tmp "$MONGO" \
      mongosh --quiet --norc 'mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-reopen' \
      --eval 'const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/reader","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_reader", pw)) quit(2); const n=db.getSiblingDB("oo_w141").shipments.find({}, {code:1}).limit(16).toArray().length; fs.writeFileSync("/tmp/oo-reopen-count.txt", "rows="+n+" role=read\n"); if (n!==16) quit(3);' \
      > "$OUT/operator/remain/source-reopen-count.out" 2> "$OUT/operator/remain/source-reopen-count.err"; then
      reopen_ready=1
      break
    fi
  done
  docker cp "$MONGO:/tmp/oo-reopen-count.txt" "$OUT/operator/remain/source-reopen-count.txt" >/dev/null 2>&1 || true
  docker exec -u 0 "$MONGO" rm -f /tmp/oo-reopen-count.txt >/dev/null 2>&1 || true
fi
printf '%s\n' "$reopen_ready" > "$OUT/operator/remain/source-reopen-ready.txt"
archive_audit after-source-off

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
unknown = load("builder/cost-unknown-price/result.json") or {}
denied = load("builder/probe-401/result.json") or {}
proxy = (out / "builder/proxy.log").read_text() if (out / "builder/proxy.log").exists() else ""
statuses = []
forward_statuses = []
for line in proxy.splitlines():
    if "response_status=" not in line:
        continue
    status = line.split("response_status=", 1)[1].split()[0]
    statuses.append(status)
    if "mode=forward " in line:
        forward_statuses.append(status)
cancel = {}
raw = load("operator/remain/cancel-query.stdout")
if isinstance(raw, str) and raw.startswith("{"):
    try:
        cancel = json.loads(raw)
    except json.JSONDecodeError:
        cancel = {}
facts = {
    "unit_exit": exit_of("unit/framing.exit"),
    "proxy_statuses": statuses,
    "probe_401_http_status": denied.get("http_status"),
    "probe_401_pack": denied.get("pack"),
    "pack_outcome": pack.get("outcome"),
    "pack_http_status": pack.get("http_status"),
    "pack_model": pack.get("model"),
    "pack_value": pack.get("pack"),
    "cache_model": cache.get("model"),
    "cache_socket": cache.get("socket"),
    "cache_pack": cache.get("pack"),
    "unknown_outcome": unknown.get("outcome"),
    "unknown_pack": unknown.get("pack"),
    "unknown_reason": unknown.get("reason"),
    "unknown_http_status": unknown.get("http_status"),
    "query_exit": exit_of("operator/query.exit"),
    "oracle_exit": exit_of("operator/query-oracle.exit"),
    "deny_exit": exit_of("operator/deny.exit"),
    "hidden_exit": exit_of("operator/hidden.exit"),
    "widen_exit": exit_of("operator/widen.exit"),
    "nobody_exit": exit_of("operator/nobody-select.exit"),
    "cancel_exit": exit_of("operator/remain/cancel.exit"),
    "cancel_outcome": cancel.get("outcome"),
    "cancel_sqlstate": cancel.get("sqlstate"),
    "cancel_rollback": cancel.get("rollback_confirmed", cancel.get("rollback")),
    "rows_before_cancel": load("operator/remain/rows-before-cancel.txt"),
    "rows_after_cancel": load("operator/remain/rows-after-cancel.txt"),
    "source_off_ping_exit": exit_of("operator/remain/source-off-ping.exit"),
    "source_off_status": load("operator/remain/source-off-status.txt"),
    "source_off_query_exit": exit_of("operator/remain/source-off-query.exit"),
    "source_off_oracle_exit": exit_of("operator/remain/source-off-oracle.exit"),
    "reopen_ready": load("operator/remain/source-reopen-ready.txt"),
}
(out / "remain/collected-facts.json").write_text(json.dumps(facts, indent=2) + "\n")
fresh_http = bool(forward_statuses) and forward_statuses[0] == "200"
gates = {
    "unit": facts["unit_exit"] == 0,
    "auth_401": facts["probe_401_http_status"] == 401 and facts["probe_401_pack"] == "not_produced",
    "fresh_pack": facts["pack_outcome"] == "packed" and facts["pack_value"] == "produced" and facts["pack_model"] == "called" and fresh_http,
    "cache_hit": facts["cache_model"] == "cache_hit" and facts["cache_socket"] == "not_opened" and facts["cache_pack"] == "produced",
    "cost_unknown": facts["unknown_outcome"] == "cost_unknown" and facts["unknown_pack"] == "not_produced",
    "typed_query": facts["query_exit"] == 0 and facts["oracle_exit"] == 0,
    "deny_update": facts["deny_exit"] not in (None, 0),
    "hidden": facts["hidden_exit"] not in (None, 0),
    "widen": facts["widen_exit"] not in (None, 0),
    "nobody": facts["nobody_exit"] not in (None, 0),
    "cancel_57014": facts["cancel_outcome"] == "canceled" and facts["cancel_sqlstate"] == "57014" and facts["cancel_rollback"] is True and facts["rows_before_cancel"] == "16" and facts["rows_after_cancel"] == "16",
    "source_off": facts["source_off_status"] == "exited" and facts["source_off_ping_exit"] not in (None, 0) and facts["source_off_query_exit"] == 0 and facts["source_off_oracle_exit"] == 0,
    "reopen": facts["reopen_ready"] == "1",
}
(out / "remain/gates.json").write_text(json.dumps(gates, indent=2) + "\n")
if all(gates.values()):
    text = "source_author_complete\n"
else:
    missing = [name for name, ok in gates.items() if not ok]
    text = "blocked\n" + "\n".join(missing) + "\n"
(out / "remain/product-outcome.txt").write_text(text)
(out / "product-outcome.txt").write_text(text)
PY
phase "recorded"
exit 0
