#!/bin/bash
# DL01-W1-40 customer five-column import, pack, typed query, cancel, and reopen.
set -eu
ROOT="/Users/redux/puppy-one/open-ontology"
BASE="/tmp/oo-dl01w1-40-bind"
EV="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-12-customer-chain"
ADMIT="$ROOT/testing/dl01-driver-preflight/clickhouse/scripts/admit.py"
SCRIPTS="$ROOT/tools/clickhouse-preview/scripts"
RUN11="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-11-http401/plan/admitted"
cd "$ROOT"

if docker ps -a --format '{{.Names}} {{.Status}}' | grep -E '^oo-dl01w1-40-' | grep -v 'Exited'; then
  echo "a same-name container is still running; not restarting" >&2
  exit 2
fi
docker ps -a --filter name=oo-dl01w1-40 --format '{{.Names}} {{.Status}}' > /tmp/oo-dl01w1-40-collected.txt || true
docker rm -f oo-dl01w1-40-rust oo-dl01w1-40-pg oo-dl01w1-40-clickhouse >/dev/null 2>&1 || true

token_bytes=$(wc -c < /tmp/oo-customer-demo-20261007/llm-token | awk '{print $1}')
if [ "$token_bytes" != "6" ]; then
  echo "token file length is not 6" >&2
  exit 2
fi
curl -sS -m 3 -o /dev/null -w '%{http_code}' http://127.0.0.1:8317/v1/models > /tmp/oo-dl01w1-40-model-port.txt 2>/tmp/oo-dl01w1-40-model-port.err || true

mkdir -p "$BASE/plan" "$BASE/inputs" "$BASE/certs" "$BASE/secrets/users.d" "$BASE/admission" "$EV/out/raw" "$EV/out/queries" "$EV/command"
rm -rf "$BASE/out"
ln -s "$EV/out" "$BASE/out"
# A previous attempt's markers must not classify, bind, or seal this input.
rm -f \
  "$EV/out/raw/READER_BUILT" \
  "$EV/out/raw/FIXTURE_OK" \
  "$EV/out/raw/SOURCE_SIDE_OK" \
  "$EV/out/raw/SOURCE_SIDE_FAIL" \
  "$EV/out/raw/BIND_READY" \
  "$EV/out/raw/BIND_FAIL" \
  "$EV/out/raw/SEALED_OK" \
  "$EV/out/raw/AUTHORITIES_OK" \
  "$EV/out/raw/PROVISION_OK" \
  "$EV/out/raw/PROVISION_FAIL" \
  "$EV/out/raw/COUNT_MISMATCH" \
  "$EV/out/raw/NEED_LOCK" \
  "$EV/out/raw/LOCK_HELD" \
  "$EV/out/raw/RELEASE_LOCK" \
  "$EV/out/raw/LOCK_RELEASED" \
  "$EV/out/raw/NEED_SOURCE_OFF" \
  "$EV/out/raw/SOURCE_STOPPED" \
  "$EV/out/raw/RUNNING" \
  "$EV/out/raw/BACKEND_SEEN" \
  "$EV/out/raw/PROBLEMS" \
  "$EV/out/raw/non-utf.arrow" \
  "$EV/out/raw/non-utf-classify.json" \
  "$EV/out/raw/non-utf-classify.exit" \
  "$EV/out/raw/customer-verdict.txt"
# Fresh must miss. A cache left by a failed attempt is not this run's hit.
rm -rf "$EV/out/builder" "$EV/out/builder-cache" "$EV/out/builder-cache-replay"
mkdir -p "$EV/out/builder" "$EV/out/builder-cache" "$EV/out/builder-cache-replay"
cp "$SCRIPTS/inside_customer_chain.sh" "$BASE/inside.sh"
cp "$SCRIPTS/coordinate_customer.sh" "$BASE/coordinate.sh"
cp "$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-11-http401/command/forwarder.rs" "$BASE/forwarder.rs"
chmod 755 "$BASE/inside.sh" "$BASE/coordinate.sh"
printf '%s\n' "oo_dl01w1_40_reader" > "$BASE/secrets/reader.user"
printf '%s\n' 'SELECT reading_id, ts, amount, note, qty FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT ArrowStream' > "$BASE/plan/customer-arrow.sql"
cp "$RUN11/catalog.json" "$BASE/plan/catalog.json"
cp "$RUN11/catalog.json" "$BASE/inputs/catalog.json"
cp "$RUN11/settings.json" "$BASE/plan/settings.json"
cp "$RUN11/visibility.json" "$BASE/plan/visibility.json"
cmp "$BASE/plan/catalog.json" "$RUN11/catalog.json"
python3 - <<'PY'
import json
from pathlib import Path
src = Path("/Users/redux/puppy-one/open-ontology/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-11-http401/plan/admitted/builder-config.json")
cfg = json.loads(src.read_text())
cfg["secret"]["bearer_file"] = "/run/llm-token"
Path("/tmp/oo-dl01w1-40-bind/plan/builder-config.json").write_text(json.dumps(cfg, indent=2) + "\n")
Path("/tmp/oo-dl01w1-40-bind/plan/init-pg.json").write_text(json.dumps({
    "protocol": "oo.pipeline.init-pg/v1",
    "target": {
        "host": "oo-dl01w1-40-pg",
        "port": 5432,
        "database": "previewdb",
        "administrator": "oo_admin",
        "password_file": "/run/pg-secrets/pg_admin_password",
        "ca_file": "/run/pg-secrets/pg-ca.crt",
    },
}) + "\n")
common = {"protocol": "oo.operator-preview.query/v1", "max_rows": 2000, "max_result_bytes": 262144}
queries = Path("/Users/redux/puppy-one/open-ontology/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-12-customer-chain/out/queries")
sql = "SELECT reading_id, ts, amount, note, qty FROM readings.final ORDER BY reading_id"
(queries / "query.json").write_text(json.dumps({**common, "sql": sql, "wall_clock_ms": 5000}) + "\n")
(queries / "query-denied.json").write_text(json.dumps({
    **common,
    "sql": "SELECT version, code, tags FROM readings.final",
    "wall_clock_ms": 5000,
}) + "\n")
(queries / "query-cancel.json").write_text(json.dumps({**common, "sql": sql, "wall_clock_ms": 20000}) + "\n")
PY
cp "$ROOT/tools/source-admission/fixtures/native/clickhouse-attempt06-report.json" "$BASE/inputs/report.json"
cp "$ROOT/tools/source-admission/fixtures/native/clickhouse-attempt06-config.json" "$BASE/inputs/config.json"
cp "$ROOT/evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json" "$BASE/inputs/admission-receipt.json"
receipt_sha=$(shasum -a 256 "$BASE/inputs/admission-receipt.json" | awk '{print $1}')
test "$receipt_sha" = "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef"

if [ ! -d "$BASE/op/tools/operator-preview" ]; then
  mkdir -p "$BASE/op"
  git archive 6d9d6cbb98f249709a462915afc8f45e4db3c047 \
    Cargo.toml Cargo.lock rust-toolchain.toml crates tools testing contracts supply-chain \
    | tar -x -C "$BASE/op"
fi
if [ ! -f "$BASE/src/Cargo.toml" ]; then
  mkdir -p "$BASE/src"
  git archive d3b3abcff32cc85602d7736eb1340b14813c069b \
    Cargo.toml Cargo.lock rust-toolchain.toml crates tools/pg-builder tools/description-pack \
    tools/description-review tools/source-admission contracts \
    | tar -x -C "$BASE/src"
fi
if [ ! -f "$BASE/certs/server.crt" ]; then
  openssl req -new -x509 -days 2 -nodes -keyout "$BASE/certs/ca.key" -out "$BASE/certs/ca.crt" -subj "/CN=oo-dl01w1-40-ca" >/dev/null 2>&1
  openssl req -new -nodes -keyout "$BASE/certs/server.key" -out "$BASE/certs/server.csr" -subj "/CN=oo-dl01w1-40-pg" >/dev/null 2>&1
  printf 'subjectAltName=DNS:oo-dl01w1-40-pg\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > "$BASE/certs/san.cnf"
  openssl x509 -req -in "$BASE/certs/server.csr" -CA "$BASE/certs/ca.crt" -CAkey "$BASE/certs/ca.key" -CAcreateserial -out "$BASE/certs/server.crt" -days 2 -extfile "$BASE/certs/san.cnf" >/dev/null 2>&1
  chmod 600 "$BASE/certs/server.key" "$BASE/certs/ca.key"
fi
if [ ! -f "$BASE/secrets/pg_admin_password" ]; then
  umask 077
  openssl rand -hex 24 > "$BASE/secrets/pg_admin_password"
  openssl rand -hex 24 > "$BASE/secrets/login_password"
  openssl rand -hex 24 > "$BASE/secrets/dba.password"
  openssl rand -hex 24 > "$BASE/secrets/reader.password"
  umask 022
fi
cp "$BASE/certs/ca.crt" "$BASE/secrets/pg-ca.crt"
python3 - <<'PY'
import hashlib
from pathlib import Path
base = Path("/tmp/oo-dl01w1-40-bind/secrets")
def sha(name):
    return hashlib.sha256((base / name).read_text().strip().encode()).hexdigest()
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
      <password_sha256_hex>{sha("dba.password")}</password_sha256_hex>
      <networks><ip>::/0</ip></networks>
      <profile>default</profile>
      <quota>default</quota>
      <grants><query>GRANT ALL ON *.* WITH GRANT OPTION</query></grants>
    </oo_dl01w1_40_dba>
    <oo_dl01w1_40_reader>
      <password_sha256_hex>{sha("reader.password")}</password_sha256_hex>
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
  </users>
</clickhouse>
"""
path = base / "users.d" / "oo-users.xml"
path.write_text(xml)
path.chmod(0o600)
PY
cp "$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission" "$BASE/admission/oo-source-admission"
chmod 755 "$BASE/admission/oo-source-admission"
admission_sha=$(shasum -a 256 "$BASE/admission/oo-source-admission" | awk '{print $1}')
test "$admission_sha" = "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2"
docker ps --format '{{.ID}} {{.Names}} {{.Status}}' > "$EV/out/neighbors-before.txt"
docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/out/foreign-pg-before.txt"
docker network inspect oo-dl01w1-40-net >/dev/null 2>&1 || docker network create --subnet 172.16.40.0/24 --gateway 172.16.40.1 oo-dl01w1-40-net >/dev/null
cp /tmp/oo-dl01w1-40-model-port.txt "$EV/out/model-port.txt" 2>/dev/null || true

cleanup() {
  docker rm -f oo-dl01w1-40-rust oo-dl01w1-40-pg oo-dl01w1-40-clickhouse >/dev/null 2>&1 || true
  docker volume rm oo-dl01w1-40-cargo oo-dl01w1-40-target oo-dl01w1-40-pgdata oo-dl01w1-40-chdata >/dev/null 2>&1 || true
  docker network rm oo-dl01w1-40-net >/dev/null 2>&1 || true
  python3 "$ADMIT" release all >/dev/null 2>&1 || true
  docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/out/foreign-pg-after.txt" 2>/dev/null || true
}
trap cleanup EXIT

acquire() {
  local kind="$1"
  shift
  local out="$EV/out/acquire-${kind}.json"
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
  echo "REFUSED $kind" | tee "$EV/out/refused-${kind}.txt"
  if [ "${WAITED:-0}" = 1 ]; then
    echo "second capacity refusal" >&2
    return 2
  fi
  WAITED=1
  python3 -c 'import time; time.sleep(900)'
  python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@"
}

acquire clickhouse docker run -d --name oo-dl01w1-40-clickhouse \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-40-net \
  --memory 2g --memory-swap 2g --cpus 1 \
  --ulimit nofile=262144:262144 \
  --mount type=volume,source=oo-dl01w1-40-chdata,target=/var/lib/clickhouse \
  --mount type=bind,source="$ROOT/testing/dl01-driver-preflight/clickhouse/config/limits.xml",target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly \
  --mount type=bind,source="$BASE/secrets/users.d/oo-users.xml",target=/etc/clickhouse-server/users.d/oo-users.xml,readonly \
  --mount type=bind,source="$BASE/secrets/dba.password",target=/etc/oo/dba.password,readonly \
  --mount type=bind,source="$BASE/secrets/reader.password",target=/etc/oo/reader.password,readonly \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=clickhouse-source \
  clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa

acquire postgres docker run -d --name oo-dl01w1-40-pg \
  --platform linux/arm64 --pull never \
  --memory 1g --memory-swap 1g --cpus 1 --pids-limit 128 \
  --network oo-dl01w1-40-net \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/pg_admin_password \
  -e POSTGRES_USER=oo_admin \
  -e POSTGRES_DB=previewdb \
  --mount type=bind,source="$BASE/certs",target=/certs,readonly \
  --mount type=bind,source="$BASE/secrets/pg_admin_password",target=/run/secrets/pg_admin_password,readonly \
  --mount type=bind,source="$BASE/secrets/login_password",target=/run/secrets/login_password,readonly \
  --mount type=bind,source="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-07/command/pg-entry.sh",target=/pg-entry.sh,readonly \
  --mount type=volume,source=oo-dl01w1-40-pgdata,target=/var/lib/postgresql/data \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=postgres-target \
  docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb \
  bash /pg-entry.sh

acquire rust docker run -d --name oo-dl01w1-40-rust \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --memory 6g --memory-swap 6g --cpus 1 --pids-limit 512 \
  --network oo-dl01w1-40-net \
  --add-host host.docker.internal:host-gateway \
  -e HOME=/work -e TMPDIR=/work -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 -e CARGO_BUILD_JOBS=1 \
  --mount type=bind,source="$ROOT/testing/dl01-driver-preflight/clickhouse",target=/driver,readonly \
  --mount type=bind,source="$BASE/op",target=/op,readonly \
  --mount type=bind,source="$BASE/src",target=/src,readonly \
  --mount type=bind,source="$BASE/plan",target=/plan,readonly \
  --mount type=bind,source="$BASE/inputs",target=/inputs,readonly \
  --mount type=bind,source="$EV/out",target=/out \
  --mount type=bind,source="$BASE/inside.sh",target=/inside.sh,readonly \
  --mount type=bind,source="$BASE/forwarder.rs",target=/forwarder.rs,readonly \
  --mount type=bind,source="$BASE/secrets",target=/run/pg-secrets,readonly \
  --mount type=bind,source="$BASE/secrets/reader.user",target=/run/ch/reader.user,readonly \
  --mount type=bind,source="$BASE/secrets/reader.password",target=/run/ch/reader.password,readonly \
  --mount type=bind,source="/tmp/oo-customer-demo-20261007/llm-token",target=/run/llm-token,readonly \
  --mount type=bind,source="$BASE/admission/oo-source-admission",target=/admission/oo-source-admission,readonly \
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

bash "$BASE/coordinate.sh" > "$EV/out/coordinate.outer" 2>&1 &
coord=$!
echo "WAIT $(date -u +%Y-%m-%dT%H:%M:%SZ)"
set +e
docker wait oo-dl01w1-40-rust > "$EV/out/rust-wait.txt"
wait "$coord"
echo "$?" > "$EV/out/coordinate.exit"
docker inspect oo-dl01w1-40-rust --format '{{.State.StartedAt}} {{.State.FinishedAt}} {{.State.ExitCode}}' > "$EV/out/rust-inspect.txt" || true
docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/out/foreign-pg-after.txt" || true
set -e
mkdir -p "$EV/command"
cp "$SCRIPTS/inside_customer_chain.sh" "$SCRIPTS/run_customer_chain.sh" "$SCRIPTS/coordinate_customer.sh" \
  "$SCRIPTS/bind_customer.py" "$SCRIPTS/provision_customer.py" "$SCRIPTS/source_customer.py" \
  "$SCRIPTS/row_oracle.py" "$EV/command/"
rust_code=$(cat "$EV/out/rust-wait.txt")
echo "RUST $rust_code COORD $(cat "$EV/out/coordinate.exit")"
if [ "$rust_code" != "0" ]; then
  exit 1
fi
