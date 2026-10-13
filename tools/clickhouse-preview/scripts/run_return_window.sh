#!/bin/bash
# DL01-W1-40 return window. Provider failures and the zero-write source window.
# Does not rewrite product-run-12 and does not start Postgres.
# provider-only reuses the recorded zero-write window and does not start ClickHouse.
# with-source runs both. The default is provider-only.
set -u
MODE="${1:-provider-only}"
ROOT="/Users/redux/puppy-one/open-ontology"
EV="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-13-return"
ADMIT="$ROOT/testing/dl01-driver-preflight/clickhouse/scripts/admit.py"
SCRIPTS="$ROOT/tools/clickhouse-preview/scripts"
BASE="/tmp/oo-dl01w1-40-bind"
SECRETS="/tmp/oo-dl01w1-40-return/secrets"
PLAN="$BASE/plan"
EXPECTED="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-12-customer-chain/out/builder/model-input.json"
TOKEN="/tmp/oo-customer-demo-20261007/llm-token"

mkdir -p "$EV/provider/raw" "$EV/zero-write" "$EV/command" "$SECRETS"
exec > >(tee -a "$EV/launcher.log") 2>&1
echo "START $(date -u +%Y-%m-%dT%H:%M:%SZ) pid $$ mode $MODE"

if docker ps -a --format '{{.Names}}' | grep -q '^oo-dl01w1-40-'; then
  echo "named container still present" >&2
  exit 1
fi
token_bytes=$(wc -c < "$TOKEN" | awk '{print $1}')
test "$token_bytes" = "6" || { echo "token file is not 6 bytes" >&2; exit 1; }
test -f "$PLAN/catalog.json" || { echo "catalog missing" >&2; exit 1; }
test -f "$EXPECTED" || { echo "model input missing" >&2; exit 1; }

python3 - <<'PY'
import hashlib, json, os
from pathlib import Path
ev = Path("/Users/redux/puppy-one/open-ontology/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-13-return/provider")
plan = Path("/tmp/oo-dl01w1-40-bind/plan/builder-config.json")
cache = Path("/Users/redux/puppy-one/open-ontology/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-12-customer-chain/out/builder-cache/pg-builder/255506fdc1679846af7091492b1b5ba562cb0f167bd325725ee82bd2d6c563e5.json")
cfg = json.loads(plan.read_text())
cfg["declared_price"] = {"status": "unknown"}
(ev / "builder-config-unknown.json").write_text(json.dumps(cfg, indent=2) + "\n")
content = json.loads(cache.read_text())["model_content"]
body = {
    "id": "oo-dl01w1-40-ee499-cost-unknown",
    "choices": [{
        "finish_reason": "stop",
        "index": 0,
        "message": {"role": "assistant", "content": content},
    }],
    "usage": {"prompt_tokens": 12, "completion_tokens": 48, "total_tokens": 60},
}
(ev / "completion-body.json").write_text(json.dumps(body))
(ev / "status-499-body.json").write_text('{"error":"client closed request"}')
secrets = Path("/tmp/oo-dl01w1-40-return/secrets")
bind = Path("/tmp/oo-dl01w1-40-bind/secrets")
for name in ("dba.password", "reader.password"):
    target = secrets / name
    target.write_bytes((bind / name).read_bytes())
    os.chmod(target, 0o600)
neg = secrets / "dba-neg.password"
if not neg.exists():
    neg.write_text(os.urandom(24).hex() + "\n")
    os.chmod(neg, 0o600)
def sha(path):
    return hashlib.sha256(path.read_text().strip().encode()).hexdigest()
xml = f"""<clickhouse>
  <profiles>
    <oo_reader>
      <readonly>1</readonly>
      <max_execution_time>8</max_execution_time>
      <function_sleep_max_microseconds_per_block>30000000</function_sleep_max_microseconds_per_block>
      <max_result_rows>100000</max_result_rows>
      <max_result_bytes>20000000</max_result_bytes>
      <output_format_json_quote_64bit_integers>1</output_format_json_quote_64bit_integers>
      <output_format_json_quote_decimals>1</output_format_json_quote_decimals>
      <output_format_decimal_trailing_zeros>1</output_format_decimal_trailing_zeros>
      <log_queries>1</log_queries>
      <output_format_arrow_compression_method>none</output_format_arrow_compression_method>
      <output_format_arrow_string_as_string>0</output_format_arrow_string_as_string>
      <output_format_arrow_low_cardinality_as_dictionary>1</output_format_arrow_low_cardinality_as_dictionary>
      <result_overflow_mode>throw</result_overflow_mode>
      <cancel_http_readonly_queries_on_client_close>1</cancel_http_readonly_queries_on_client_close>
      <constraints>
        <max_execution_time><min>1</min><max>8</max><changeable_in_readonly/></max_execution_time>
        <max_result_rows><min>1</min><max>100000</max><changeable_in_readonly/></max_result_rows>
        <max_result_bytes><min>1</min><max>20000000</max><changeable_in_readonly/></max_result_bytes>
      </constraints>
    </oo_reader>
    <oo_deny>
      <log_queries>1</log_queries>
    </oo_deny>
  </profiles>
  <quotas>
    <oo_reader>
      <interval>
        <duration>3600</duration>
        <queries>400</queries>
        <errors>80</errors>
        <result_rows>1000000</result_rows>
        <read_rows>10000000</read_rows>
        <execution_time>300</execution_time>
      </interval>
    </oo_reader>
  </quotas>
  <users>
    <oo_dl01w1_40_dba>
      <password_sha256_hex>{sha(secrets / "dba.password")}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>default</profile>
      <quota>default</quota>
      <grants><query>GRANT ALL ON *.* WITH GRANT OPTION</query></grants>
    </oo_dl01w1_40_dba>
    <oo_dl01w1_40_reader>
      <password_sha256_hex>{sha(secrets / "reader.password")}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>oo_reader</profile>
      <quota>oo_reader</quota>
      <grants>
        <query>GRANT SELECT ON oo_dl01w40.*</query>
        <query>GRANT SELECT ON system.columns</query>
        <query>GRANT SELECT ON system.tables</query>
        <query>GRANT SELECT ON system.databases</query>
        <query>GRANT SELECT ON system.parts</query>
        <query>GRANT SELECT ON system.query_log</query>
        <query>GRANT SELECT ON system.mutations</query>
        <query>GRANT SELECT ON system.processes</query>
        <query>GRANT SELECT ON system.grants</query>
        <query>GRANT SELECT ON system.settings</query>
        <query>GRANT SHOW DATABASES ON *.*</query>
      </grants>
    </oo_dl01w1_40_reader>
    <oo_dl01w1_40_dba_neg>
      <password_sha256_hex>{sha(secrets / "dba-neg.password")}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>oo_deny</profile>
      <quota>default</quota>
      <grants>
        <query>GRANT SELECT ON oo_dl01w40.*</query>
      </grants>
    </oo_dl01w1_40_dba_neg>
  </users>
</clickhouse>
"""
path = secrets / "users.d" / "oo-users.xml"
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(xml)
os.chmod(path, 0o600)
print("secrets-ready")
PY

docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-before.txt"
docker network inspect oo-dl01w1-40-net >/dev/null 2>&1 || docker network create --subnet 172.16.40.0/24 --gateway 172.16.40.1 oo-dl01w1-40-net >/dev/null

KEEP_VOLUMES=0
ZERO_PID=""
cleanup() {
  if [ -n "$ZERO_PID" ]; then
    kill "$ZERO_PID" 2>/dev/null || true
  fi
  docker rm -f oo-dl01w1-40-rust oo-dl01w1-40-clickhouse >/dev/null 2>&1 || true
  if [ "$KEEP_VOLUMES" != "1" ]; then
    docker volume rm oo-dl01w1-40-cargo oo-dl01w1-40-target oo-dl01w1-40-chdata >/dev/null 2>&1 || true
  fi
  docker network rm oo-dl01w1-40-net >/dev/null 2>&1 || true
  python3 "$ADMIT" release all >/dev/null 2>&1 || true
  docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-after.txt" 2>/dev/null || true
}
trap cleanup EXIT

acquire() {
  local kind="$1"
  shift
  local out="$EV/acquire-${kind}.json"
  if python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@"; then
    return 0
  fi
  local admitted reason
  admitted=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("admitted"))' "$out")
  reason=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("reason"))' "$out")
  if [ "$admitted" = "True" ] || [ "$reason" = "docker start failed" ]; then
    echo "docker start failed for $kind" >&2
    return 3
  fi
  echo "REFUSED $kind $reason" | tee "$EV/refused-${kind}.txt"
  if [ "${WAITED:-0}" = "1" ]; then
    echo "second capacity refusal" >&2
    return 2
  fi
  WAITED=1
  echo "WAIT_900 $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  python3 -c 'import time; time.sleep(900)'
  python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@"
}

if [ "$MODE" = "provider-only" ]; then
  test -f "$EV/zero-write/SOURCE_SIDE_OK" || { echo "missing zero-write evidence" >&2; exit 1; }
  test -f "$EV/zero-write/DBA_NEG_OK" || { echo "missing dba-neg evidence" >&2; exit 1; }
  test "$(cat "$EV/zero-write.exit")" = "0" || { echo "zero-write exit is not 0" >&2; exit 1; }
  echo "ZERO_SKIPPED reuse product-run-13-return/zero-write"
elif [ "$MODE" = "with-source" ]; then
  acquire clickhouse docker run -d --name oo-dl01w1-40-clickhouse \
    --platform linux/arm64 --pull never \
    --network oo-dl01w1-40-net \
    --memory 2g --memory-swap 2g --cpus 1 \
    --ulimit nofile=262144:262144 \
    --mount type=volume,source=oo-dl01w1-40-chdata,target=/var/lib/clickhouse \
    --mount type=bind,source="$ROOT/testing/dl01-driver-preflight/clickhouse/config/limits.xml",target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly \
    --mount type=bind,source="$SECRETS/users.d/oo-users.xml",target=/etc/clickhouse-server/users.d/oo-users.xml,readonly \
    --mount type=bind,source="$SECRETS/dba.password",target=/etc/oo/dba.password,readonly \
    --mount type=bind,source="$SECRETS/reader.password",target=/etc/oo/reader.password,readonly \
    --mount type=bind,source="$SECRETS/dba-neg.password",target=/etc/oo/dba-neg.password,readonly \
    --label oo.project=open-ontology \
    --label oo.owner=oo-dl01w1-40 \
    --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
    --label oo.task=DL01-W1-40 \
    --label oo.kind=clickhouse-source \
    clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa
  echo "CLICKHOUSE_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  (
    set -e
    export OO_RETURN_WINDOW=1
    export OO_CUSTOMER_OUT="$EV/zero-write"
    python3 "$SCRIPTS/source_customer.py"
    python3 "$SCRIPTS/dba_negative_window.py"
  ) > "$EV/zero-write-host.log" 2>&1 &
  ZERO_PID=$!
  echo "ZERO_PID $ZERO_PID"
else
  echo "unknown mode $MODE" >&2
  exit 1
fi

acquire rust docker run -d --name oo-dl01w1-40-rust \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --memory 6g --memory-swap 6g --cpus 1 --pids-limit 512 \
  --network oo-dl01w1-40-net \
  --add-host host.docker.internal:host-gateway \
  -e HOME=/work -e TMPDIR=/work -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 -e CARGO_BUILD_JOBS=1 \
  --mount type=bind,source="$BASE/src",target=/src,readonly \
  --mount type=bind,source="$PLAN",target=/plan,readonly \
  --mount type=bind,source="$EV/provider",target=/out \
  --mount type=bind,source="$SCRIPTS/inside_provider_failures.sh",target=/inside.sh,readonly \
  --mount type=bind,source="$SCRIPTS/failure_forwarder.rs",target=/forwarder.rs,readonly \
  --mount type=bind,source="$EXPECTED",target=/expected/model-input.json,readonly \
  --mount type=bind,source="$TOKEN",target=/run/llm-token,readonly \
  --mount type=volume,source=oo-dl01w1-40-cargo,target=/cargo \
  --mount type=volume,source=oo-dl01w1-40-target,target=/target \
  --tmpfs /tmp:rw,nosuid,nodev,size=512m \
  --tmpfs /work:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 \
  bash /inside.sh
echo "RUST_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"

set +e
docker wait oo-dl01w1-40-rust > "$EV/rust-wait.txt"
rust_code=$(cat "$EV/rust-wait.txt")
if [ -n "$ZERO_PID" ]; then
  wait "$ZERO_PID"
  zero_code=$?
  echo "$zero_code" > "$EV/zero-write.exit"
else
  zero_code=0
fi
set -u
echo "RUST $rust_code ZERO $zero_code $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker inspect oo-dl01w1-40-rust --format '{{.State.StartedAt}} {{.State.FinishedAt}} {{.State.ExitCode}}' > "$EV/rust-inspect.txt" || true
if [ "$rust_code" != "0" ]; then
  KEEP_VOLUMES=1
fi
mkdir -p "$EV/command"
cp "$SCRIPTS/source_customer.py" "$SCRIPTS/dba_negative_window.py" \
  "$SCRIPTS/inside_provider_failures.sh" "$SCRIPTS/run_return_window.sh" \
  "$SCRIPTS/failure_forwarder.rs" "$EV/command/"
if [ "$rust_code" != "0" ] || [ "$zero_code" != "0" ]; then
  echo "CHAIN_EXIT:1"
  exit 1
fi
echo "CHAIN_EXIT:0"
exit 0
