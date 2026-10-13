#!/bin/bash
# Live ClickHouse field case, then one pg-builder sealed call with an invalid bearer.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
EV="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-11-http401"
BASE="/tmp/oo-dl01w1-40-bind"
ADMIT="$ROOT/testing/dl01-driver-preflight/clickhouse/scripts/admit.py"
ARCHIVE="d3b3abcff32cc85602d7736eb1340b14813c069b"
LOCK_SHA="b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474"
CH_IMAGE="clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
FORWARDER="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-08-bytea/command/forwarder.rs"
OLD_NINE="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-10-wal/out/raw/source-rows.stdout"
OLD_CATALOG="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-08-bytea/catalog-faithful.json"
FIXTURE="$ROOT/testing/dl01-driver-preflight/clickhouse/sql/00-fixture.sql"
LIMITS="$ROOT/testing/dl01-driver-preflight/clickhouse/config/limits.xml"

mkdir -p "$EV/command" "$EV/plan/admitted" "$EV/plan/refusals" "$EV/inputs" "$EV/out/raw" "$EV/out/failure-cache" "$BASE/secrets/users.d" "$BASE/plan" "$BASE/inputs"
: > "$EV/out/launch.log"
exec > >(tee -a "$EV/out/launch.log") 2>&1
log() { echo "phase $* $(date -u +%Y-%m-%dT%H:%M:%SZ)"; }

cleanup() {
  docker rm -f oo-dl01w1-40-rust oo-dl01w1-40-clickhouse >/dev/null 2>&1 || true
  if [ "${1:-}" = "final" ]; then
    docker volume rm oo-dl01w1-40-cargo oo-dl01w1-40-target oo-dl01w1-40-chdata >/dev/null 2>&1 || true
    docker network rm oo-dl01w1-40-net >/dev/null 2>&1 || true
  else
    docker volume rm oo-dl01w1-40-chdata >/dev/null 2>&1 || true
  fi
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
  python3 "$ADMIT" release clickhouse >/dev/null 2>&1 || true
  python3 "$ADMIT" release postgres >/dev/null 2>&1 || true
  docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/out/foreign-pg-after.txt" 2>/dev/null || true
}
trap 'status=$?; cleanup; if [ "$status" -ne 0 ]; then echo LAUNCH_FAIL "$status"; fi' EXIT

log preflight
test -f "$FORWARDER"
test -f "$OLD_NINE"
test -f "$OLD_CATALOG"
test -f "$FIXTURE"
test -f "$LIMITS"
docker image inspect "$CH_IMAGE" >/dev/null
docker image inspect "$RUST_IMAGE" >/dev/null
python3 - << 'PY'
import socket
socket.create_connection(("127.0.0.1", 8317), 3).close()
PY
echo open > "$EV/out/raw/provider-tcp.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Status}}' > "$EV/out/neighbors-before.txt"
docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/out/foreign-pg-before.txt"
docker rm -f oo-dl01w1-40-rust oo-dl01w1-40-clickhouse >/dev/null 2>&1 || true
docker volume rm oo-dl01w1-40-chdata >/dev/null 2>&1 || true
cp "$ROOT/tools/clickhouse-preview/scripts/inside_http401.sh" "$EV/command/inside_http401.sh"
cp "$ROOT/tools/clickhouse-preview/scripts/extract_case.py" "$EV/command/extract_case.py"
cp "$ROOT/tools/clickhouse-preview/scripts/customer_case.py" "$EV/command/customer_case.py"
cp "$ROOT/tools/clickhouse-preview/scripts/run_http401.sh" "$EV/command/run_http401.sh"
cp "$FORWARDER" "$EV/command/forwarder.rs"
cp "$EV/command/inside_http401.sh" "$BASE/inside.sh"
cp "$EV/command/forwarder.rs" "$BASE/forwarder.rs"
chmod 755 "$BASE/inside.sh"

log customer-plan
python3 - << PY
import json
from pathlib import Path
plan = Path("$EV/plan")
customer = {
    "protocol": "oo.clickhouse.customer-case/v1",
    "source_id": "clickhouse_268",
    "secret_ref": "secret://oo-dl01w1-40/reader",
    "role": "oo_r_reader",
    "budget": {"unit": "Rows", "hard_limit": 100000, "confidence": "unknown"},
    "model": {
        "origin": "http://127.0.0.1:8317",
        "path": "/v1/chat/completions",
        "model_id": "grok-4.7",
    },
    "approved_objects": [
        {
            "database": "oo_dl01w40",
            "table": "readings",
            "logical_schema": "readings",
            "logical_table": "final",
            "fields": [
                {"name": "reading_id", "source_type": "uint64", "key": True},
                {"name": "ts", "source_type": "timestamp_nanos_utc"},
                {"name": "amount", "source_type": "decimal76_10"},
                {"name": "note", "source_type": "nullable_string"},
                {"name": "qty", "source_type": "int64"},
                {"name": "code_fixed", "source_type": "fixed_string"},
                {"name": "ts_raw", "source_type": "datetime64_9_utc"},
                {"name": "tags", "source_type": "array_int64"},
            ],
        }
    ],
}
(plan / "customer.json").write_text(json.dumps(customer, indent=2) + "\n")
requests = {
    "unapproved-field": {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["labels"]},
    "unapproved-object": {"object": {"database": "oo_dl01w40", "table": "other"}, "fields": ["reading_id"]},
    "fixed-string": {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["code_fixed"]},
    "datetime64": {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["ts_raw"]},
    "array": {"object": {"database": "oo_dl01w40", "table": "readings"}, "fields": ["tags"]},
    "approved": {
        "object": {"database": "oo_dl01w40", "table": "readings"},
        "fields": ["reading_id", "ts", "amount", "note", "qty"],
    },
}
for name, body in requests.items():
    (plan / f"request-{name}.json").write_text(json.dumps(body, indent=2) + "\n")
PY

refuse() {
  local name="$1"
  local dest="$EV/plan/refusals/$name"
  mkdir -p "$dest"
  set +e
  python3 "$ROOT/tools/clickhouse-preview/scripts/customer_case.py" \
    --customer "$EV/plan/customer.json" \
    --request "$EV/plan/request-${name}.json" \
    --out "$dest" \
    --run-id "11111111-2222-4333-8444-555555555555" \
    --rows 3
  local code=$?
  set -e
  echo "$code" > "$dest/cli.exit"
  test "$code" = "2"
  python3 - "$dest/decision.json" "$2" << 'PY'
import json, sys
body = json.loads(open(sys.argv[1], encoding="utf-8").read())
reason = sys.argv[2]
if body.get("reason") != reason or body.get("source_read") is not False or body.get("sql") is not None:
    raise SystemExit(f"refusal {reason} was not a pre-read refusal")
if "secret://" in json.dumps(body):
    raise SystemExit("refusal decision copied a secret reference")
PY
}
refuse unapproved-field unapproved_field
refuse unapproved-object unapproved_object
refuse fixed-string capability_undeclared
refuse datetime64 type_unsupported
refuse array type_unsupported

log users
if [ ! -f "$BASE/secrets/dba.password" ] || [ ! -f "$BASE/secrets/reader.password" ]; then
  umask 077
  openssl rand -hex 24 > "$BASE/secrets/dba.password"
  openssl rand -hex 24 > "$BASE/secrets/reader.password"
  umask 022
fi
chmod 600 "$BASE/secrets/dba.password" "$BASE/secrets/reader.password"
python3 - << 'PY'
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
      <max_result_rows>100000</max_result_rows>
      <max_result_bytes>20000000</max_result_bytes>
      <output_format_json_quote_64bit_integers>1</output_format_json_quote_64bit_integers>
      <output_format_json_quote_decimals>1</output_format_json_quote_decimals>
      <output_format_decimal_trailing_zeros>1</output_format_decimal_trailing_zeros>
      <log_queries>1</log_queries>
      <result_overflow_mode>throw</result_overflow_mode>
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
        <query>GRANT SELECT ON system.query_log</query>
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

log clickhouse
docker network inspect oo-dl01w1-40-net >/dev/null 2>&1 || docker network create --subnet 172.16.40.0/24 --gateway 172.16.40.1 oo-dl01w1-40-net >/dev/null
acquire() {
  local kind="$1"
  shift
  local out="$EV/out/acquire-${kind}.json"
  local attempt reason
  for attempt in 1 2; do
    if python3 "$ADMIT" acquire-start "$kind" "$out" -- "$@"; then
      return 0
    fi
    reason=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("reason"))' "$out")
    echo "REFUSED $kind $reason"
    case "$reason" in
      *headroom*|*CPU*|*rust\ builder*)
        if [ "$attempt" = 2 ]; then
          return 2
        fi
        python3 -c 'import time; time.sleep(900)'
        ;;
      *)
        return 2
        ;;
    esac
  done
}
acquire clickhouse docker run -d --name oo-dl01w1-40-clickhouse \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-40-net \
  --memory 2g --memory-swap 2g --cpus 1 \
  --ulimit nofile=262144:262144 \
  --mount type=volume,source=oo-dl01w1-40-chdata,target=/var/lib/clickhouse \
  --mount type=bind,source="$LIMITS",target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly \
  --mount type=bind,source="$BASE/secrets/users.d/oo-users.xml",target=/etc/clickhouse-server/users.d/oo-users.xml,readonly \
  --mount type=bind,source="$BASE/secrets/dba.password",target=/etc/oo/dba.password,readonly \
  --mount type=bind,source="$BASE/secrets/reader.password",target=/etc/oo/reader.password,readonly \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=clickhouse-source \
  "$CH_IMAGE"

python3 "$ROOT/tools/clickhouse-preview/scripts/customer_case.py" \
  --customer "$EV/plan/customer.json" \
  --request "$EV/plan/request-approved.json" \
  --out "$EV/plan/admitted" \
  --run-id "$(python3 -c 'import uuid; print(uuid.uuid4())')" \
  --rows 3 \
  --bearer-file /run/invalid-bearer
python3 - << PY
import json
from pathlib import Path
body = json.loads(Path("$EV/plan/admitted/decision.json").read_text())
packed = "".join(path.read_text() for path in Path("$EV/plan/admitted").glob("*.json"))
if body.get("admitted") is not True or body.get("source_read") is not True:
    raise SystemExit("approved plan was not admitted")
if "secret://" in packed:
    raise SystemExit("admitted plan copied a secret reference")
if body.get("project") != "clickhouse_268" or body.get("role") != "oo_r_reader" or body.get("model_id") != "grok-4.7":
    raise SystemExit("approved plan is not bound to this source role and model")
PY
python3 "$ROOT/tools/clickhouse-preview/scripts/extract_case.py" \
  --out "$EV/out/raw" \
  --decision "$EV/plan/admitted/decision.json" \
  --fixture "$FIXTURE" \
  --old-nine "$OLD_NINE"
rows=$(python3 -c 'import json; print(json.load(open("'"$EV"'/out/raw/nine-column-hash.json"))["approved_rows"])')
if [ "$rows" != "3" ]; then
  python3 "$ROOT/tools/clickhouse-preview/scripts/customer_case.py" \
    --customer "$EV/plan/customer.json" \
    --request "$EV/plan/request-approved.json" \
    --out "$EV/plan/admitted" \
    --run-id "$(python3 -c 'import json; print(json.load(open("'"$EV"'/plan/admitted/catalog.json"))["run_id"])')" \
    --rows "$rows" \
    --bearer-file /run/invalid-bearer
fi

log stop-clickhouse
docker rm -f oo-dl01w1-40-clickhouse >/dev/null
python3 "$ADMIT" release clickhouse

log catalogs
python3 - << PY
import json
import sys
from pathlib import Path
sys.path.insert(0, "$ROOT/tools/clickhouse-preview/scripts")
import run_product as rp
rows = 3
datetime_columns = rp.faithful_columns()[:1] + [
    rp.column("ts", 1184, 9, "datetime64_9_utc", False, None)
]
array_columns = rp.faithful_columns()[:1] + [
    rp.column("tags", 2277, -1, "array_int64", False, None),
    rp.column("labels", 2277, -1, "array_nullable_string", False, None),
]
inputs = Path("$EV/inputs")
(inputs / "catalog-datetime64.json").write_text(json.dumps(rp.catalog(datetime_columns, rows), indent=2) + "\n")
(inputs / "catalog-array.json").write_text(json.dumps(rp.catalog(array_columns, rows), indent=2) + "\n")
generated = json.dumps(rp.catalog(rp.faithful_columns(), rows), indent=2) + "\n"
old = Path("$OLD_CATALOG").read_text()
report = {
    "equal": generated == old,
    "generated_sha256": __import__("hashlib").sha256(generated.encode()).hexdigest(),
    "retained_sha256": __import__("hashlib").sha256(old.encode()).hexdigest(),
}
(Path("$EV/out/raw") / "faithful-catalog-bytes.json").write_text(json.dumps(report, indent=2) + "\n")
PY
rm -rf "$BASE/plan" "$BASE/inputs"
mkdir -p "$BASE/plan" "$BASE/inputs"
cp "$EV/plan/admitted/"*.json "$BASE/plan/"
cp "$EV/inputs/catalog-datetime64.json" "$EV/inputs/catalog-array.json" "$BASE/inputs/"
dataset=$(python3 -c 'import json; print(json.load(open("'"$EV"'/plan/admitted/decision.json"))["dataset"])')
test "$dataset" = "readings.final"

log bearer
python3 - << 'PY'
import secrets
from pathlib import Path
real = Path("/tmp/oo-customer-demo-20261007/llm-token").read_bytes()
token = secrets.token_hex(8).encode()
body = token + b"\n"
if body == real or token == real.strip() or len(body) == len(real) or real.strip() in body or token in real:
    raise SystemExit("invalid bearer is not distinct")
path = Path("/tmp/oo-dl01w1-40-bind/secrets/invalid-bearer")
path.write_bytes(body)
path.chmod(0o600)
Path("/tmp/oo-dl01w1-40-bind/secrets/invalid-bearer.meta").write_text(
    '{"bytes": %d, "mode": "0600", "distinct_from_customer_token": true}\n' % len(body)
)
PY
cp "$BASE/secrets/invalid-bearer.meta" "$EV/out/raw/invalid-bearer.meta.json"
chmod 600 "$BASE/secrets/invalid-bearer"

log archive
if [ "$(cat "$BASE/src/SOURCE_COMMIT" 2>/dev/null || true)" != "$ARCHIVE" ] || [ ! -f "$BASE/src/Cargo.lock" ]; then
  rm -rf "$BASE/src"
  mkdir -p "$BASE/src"
  git -C "$ROOT" archive "$ARCHIVE" \
    Cargo.toml Cargo.lock rust-toolchain.toml crates tools/pg-builder tools/description-pack \
    tools/description-review tools/source-admission contracts \
    | tar -x -C "$BASE/src"
  printf '%s\n' "$ARCHIVE" > "$BASE/src/SOURCE_COMMIT"
fi
printf 'git archive %s Cargo.toml Cargo.lock rust-toolchain.toml crates tools/pg-builder tools/description-pack tools/description-review tools/source-admission contracts\n' "$ARCHIVE" > "$EV/out/raw/archive-command.txt"
lock_now=$(shasum -a 256 "$BASE/src/Cargo.lock" | awk '{print $1}')
printf '%s\n' "$lock_now" > "$EV/out/raw/cargo-lock.host.sha"
test "$lock_now" = "$LOCK_SHA"
printf 'false\n' > "$EV/out/raw/live-workspace-compiled.txt"

log rust
rm -rf "$EV/out/failure-cache" "$EV/out/builder-401"
mkdir -p "$EV/out/failure-cache" "$EV/out/builder-401"
acquire rust docker run -d --name oo-dl01w1-40-rust \
  --platform linux/arm64 --pull never \
  --memory 6g --memory-swap 6g --cpus 1 --pids-limit 512 \
  --network oo-dl01w1-40-net \
  --add-host host.docker.internal:host-gateway \
  -e HOME=/work -e TMPDIR=/work -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 -e CARGO_BUILD_JOBS=1 \
  -e OO_DATASET="$dataset" \
  --mount type=bind,source="$BASE/src",target=/src,readonly \
  --mount type=bind,source="$BASE/plan",target=/plan,readonly \
  --mount type=bind,source="$BASE/inputs",target=/inputs,readonly \
  --mount type=bind,source="$EV/out",target=/out \
  --mount type=bind,source="$BASE/inside.sh",target=/inside.sh,readonly \
  --mount type=bind,source="$BASE/forwarder.rs",target=/forwarder.rs,readonly \
  --mount type=bind,source="$BASE/secrets/invalid-bearer",target=/run/invalid-bearer,readonly \
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
  "$RUST_IMAGE" \
  bash /inside.sh
set +e
docker wait oo-dl01w1-40-rust > "$EV/out/rust-wait.txt"
wait_rc=$?
set -e
echo "$wait_rc" > "$EV/out/docker-wait.exit"
docker inspect oo-dl01w1-40-rust --format '{{.State.StartedAt}} {{.State.FinishedAt}} {{.State.ExitCode}}' > "$EV/out/rust-inspect.txt"

log verify
python3 - << PY
import json
from pathlib import Path
ev = Path("$EV")
out = ev / "out"
result_path = out / "builder-401" / "result.json"
model_path = out / "builder-401" / "model-input.json"
result = json.loads(result_path.read_text())
model = json.loads(model_path.read_text())
cache_files = [str(path.relative_to(out / "failure-cache")) for path in (out / "failure-cache").rglob("*") if path.is_file()]
pack_names = ["role-pack", "pack", "authority.json", "policy.json", "effective.json", "reviewed.json"]
present = [name for name in pack_names if (out / "builder-401" / name).exists()]
upstream = (out / "raw" / "upstream.status").read_text().strip() if (out / "raw" / "upstream.status").exists() else None
summary = {
    "builder_exit": (out / "raw" / "builder.exit").read_text().strip(),
    "outcome": result.get("outcome"),
    "reason": result.get("reason"),
    "http_status": result.get("http_status"),
    "model": result.get("model"),
    "pack": result.get("pack"),
    "cache_store": result.get("cache_store"),
    "ready": result.get("ready"),
    "publication": result.get("publication"),
    "socket": result.get("socket"),
    "source_id": result.get("source_id"),
    "source_engine": result.get("source_engine"),
    "cache_files": cache_files,
    "pack_paths_present": present,
    "upstream_status": upstream,
    "target_host": False,
}
(out / "raw" / "http401-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
columns = model.get("columns") or []
ts = [row for row in columns if row.get("column") == "ts"]
problems = []
required = {
    "outcome": "provider_failure",
    "reason": "http_status",
    "http_status": 401,
    "model": "called",
    "pack": "not_produced",
    "cache_store": "not_stored",
    "ready": False,
    "publication": "not_published",
    "socket": "opened",
    "source_id": "clickhouse_268",
    "source_engine": "clickhouse",
}
for key, expect in required.items():
    if summary.get(key) != expect:
        problems.append(key)
if summary["builder_exit"] != "1":
    problems.append("builder_exit")
if cache_files:
    problems.append("cache_stored")
if present:
    problems.append("half_ready")
if upstream != "401":
    problems.append("upstream_status")
if len(ts) != 1 or ts[0].get("source_type") != "timestamp_nanos_utc" or ts[0].get("target_oid") != 1700:
    problems.append("ts_binding")
if any(row.get("target_type") == "timestamptz" for row in columns):
    problems.append("timestamptz")
real = Path("/tmp/oo-customer-demo-20261007/llm-token").read_bytes().strip()
bad = Path("/tmp/oo-dl01w1-40-bind/secrets/invalid-bearer").read_bytes().strip()
passwords = [
    Path("/tmp/oo-dl01w1-40-bind/secrets/dba.password").read_bytes().strip(),
    Path("/tmp/oo-dl01w1-40-bind/secrets/reader.password").read_bytes().strip(),
]
classes = [("customer_token", real), ("invalid_token", bad), ("password", passwords[0]), ("password", passwords[1])]
withheld = []
for path in ev.rglob("*"):
    if not path.is_file():
        continue
    data = path.read_bytes()
    hit = [name for name, needle in classes if needle and needle in data]
    if not hit:
        continue
    relative = str(path.relative_to(ev))
    record = {"path": relative, "bytes": len(data), "classes": sorted(set(hit))}
    withheld.append(record)
    if relative.startswith("out/raw/") and path.name.endswith(".stdout"):
        record["kept"] = "source extract"
        continue
    if relative == "out/builder-401/model-response.bin":
        record["kept"] = False
    path.unlink()
    record["removed"] = True
(out / "raw" / "secret-scan.json").write_text(json.dumps({"withheld": withheld}, indent=2) + "\n")
kept = [item for item in withheld if item.get("removed") and item["path"] != "out/builder-401/model-response.bin"]
if kept:
    problems.append("secret_material")
extract_problems = (out / "raw" / "extract-problems.txt").read_text().strip()
if extract_problems:
    problems.append("extract")
seal_customer = (out / "raw" / "seal-customer.exit").read_text().strip()
seal_dt = (out / "raw" / "seal-datetime64.exit").read_text().strip()
seal_array = (out / "raw" / "seal-array.exit").read_text().strip()
if not (out / "raw" / "HTTP401_OK").is_file():
    problems.append("http401_ok")
if seal_customer != "0":
    problems.append("seal_customer")
if seal_dt != "1" or seal_array != "1":
    problems.append("seal_unsupported")
(out / "raw" / "gate-problems.txt").write_text((" ".join(problems) + "\n") if problems else "")
if problems:
    raise SystemExit("gate " + " ".join(problems))
PY
log verified
cleanup final
trap - EXIT
echo LAUNCH_DONE
exit 0
