#!/bin/bash
# One oo_w141 closure from producer a682 (ancestor of the d0cc loader and core 29aae36b)
# and the fixed 6d9 consumer. Does not compile the live workspace.
# Does not join oo-dl01w1-06-pg. A valid seal is kept when the consumer chain is unfinished.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="${CLOSURE_OUT:-$EVIDENCE/raw/consumer-closure}"
PLAN="$OUT/plan"
SRC="/tmp/oo-dl01w1-41-typed-src"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
ARROW="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow"
DECODED="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/probe/decoded.json"
RECEIPT="$ROOT/evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json"
ADMISSION="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission"
BIND_PLAN="$EVIDENCE/raw/native-bind/plan"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="postgres@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f"
PRODUCER="a6825165dd83ffaa30e17fbb2da1a481a05f6559"
PRODUCER_CORE="29aae36b58d61c67f001893cd38aac28ec0f959f"
TYPED_ANCESTOR="d0cc59eb570bb49e7b911ec633d40799af8a3d06"
CONSUMER="6d9d6cbb98f249709a462915afc8f45e4db3c047"
OPERATOR_SHA="f7fdba26e8ec13bfd5a423f741f80c9e238abd74b39f8ea96e592f1b34c76a78"
OWNER="23df6145-4592-4dc0-82df-fd226dd020fd"
SCHEMA_DIGEST="6ddbefe926004eec4f9bccba8ad266169ba2077739b8005396046d5ea0e76386"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-consumer"
PG="${PREFIX}-pg"
NET="${PREFIX}-net"
TOKEN_FILE="/tmp/oo-customer-demo-20261007/llm-token"
OPBIN="/target/w21-native/debug/operator-preview"

phase() {
  mkdir -p "$OUT"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$OUT/phase.txt"
}

cleanup() {
  local ec=$?
  phase "cleanup $ec"
  docker logs "$RUST" > "$OUT/docker-rust.log" 2>&1 || true
  docker logs "$PG" > "$OUT/docker-pg.log" 2>&1 || true
  local keep=0
  local positive_ec=""
  local source_off_ec=""
  local cancel_ec=""
  local reopen_ready=""
  if [[ -f "$OUT/positive.exit" ]]; then
    positive_ec="$(tr -d '[:space:]' < "$OUT/positive.exit")"
  fi
  if [[ -f "$OUT/operator/source-off-query.exit" ]]; then
    source_off_ec="$(tr -d '[:space:]' < "$OUT/operator/source-off-query.exit")"
  fi
  if [[ -f "$OUT/operator/cancel.exit" ]]; then
    cancel_ec="$(tr -d '[:space:]' < "$OUT/operator/cancel.exit")"
  fi
  if [[ -f "$OUT/operator/source-reopen-ready.txt" ]]; then
    reopen_ready="$(tr -d '[:space:]' < "$OUT/operator/source-reopen-ready.txt")"
  fi
  if [[ "$positive_ec" == "0" && ( "$source_off_ec" != "0" || "$cancel_ec" != "0" || "$reopen_ready" != "1" ) ]]; then
    keep=1
  fi
  if [[ "$keep" -eq 1 ]]; then
    docker rm -f "$RUST" "${PREFIX}-read" > "$OUT/docker-rm.txt" 2>&1 || true
    python3 "$ADMIT" acquire mongodb "$OUT/admission-mongodb-retain.json" > "$OUT/admission-mongodb-retain.stdout" 2>&1 || true
    python3 "$ADMIT" release rust > "$OUT/release.json" 2>&1 || true
    {
      echo "retained_for_later_model_consume"
      echo "pg_container ${PG}"
      echo "pg_volume ${PREFIX}-pgdata"
      echo "mongo_volume ${PREFIX}-data"
      echo "network ${NET}"
      echo "secrets_dir ${SECRETS}"
      echo "cargo_volume ${PREFIX}-cargo"
      echo "target_volume ${PREFIX}-target"
      docker ps -a --filter "name=${PREFIX}" --format '{{.Names}} {{.Status}}'
      docker volume ls --filter "name=${PREFIX}" --format '{{.Name}}'
    } > "$OUT/resource-retained.txt"
    printf '%s\n' "network and sealed postgres were kept" > "$OUT/docker-network-rm.txt"
    printf '%s\n' "sealed volumes were kept" > "$OUT/docker-volume-rm.txt"
  else
    docker rm -f "$RUST" "${PREFIX}-read" > "$OUT/docker-rm.txt" 2>&1 || true
    printf '%s\n' "sealed postgres and mongodb were kept" > "$OUT/docker-network-rm.txt"
    printf '%s\n' "sealed volumes were kept" > "$OUT/docker-volume-rm.txt"
    python3 "$ADMIT" release all > "$OUT/release.json" 2>&1 || true
  fi
  {
    echo "left-running:"
    docker ps --format '{{.Names}}' | grep -E 'oo-dl01w1-0(6|4)|oo-dl01w1-21' || true
    echo "own-left:"
    docker ps -a --format '{{.Names}} {{.Status}}' | grep "$PREFIX" || true
    echo "keep ${keep}"
  } > "$OUT/cleanup.txt"
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
  rm -rf "$SRC"
  if [[ "$keep" -eq 0 ]]; then
    rm -rf "$SECRETS"
  fi
  printf '%s\n' "$ec" > "$OUT/status.txt"
}
trap cleanup EXIT

if [[ -e "$OUT" ]]; then
  printf '%s FAIL %s already exists\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$OUT" >&2
  exit 1
fi
phase "prepare"
mkdir -p "$PLAN" "$OUT/operator" "$OUT/builder" "$OUT/source-read"
test -f "$TOKEN_FILE"
test -f "$ADMISSION"
test -f "$ARROW"
test -f "$DECODED"
test -f "$RECEIPT"
phase "source read"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --subnet 10.41.41.0/24 \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session="$OWNER" --label task=DL01-W1-41 --label kind=mongodb-source \
    "$NET" >/dev/null || { phase "FAIL network"; exit 1; }
fi
if ! TYPED_READ_OUT="$OUT/source-read" bash "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/typed-file-read.sh"; then
  phase "FAIL source read"
  exit 1
fi
rm -rf "$SRC/d0cc" "$SRC/consumer"
mkdir -p "$SRC/d0cc" "$SRC/consumer"
git -C "$ROOT" merge-base --is-ancestor "$TYPED_ANCESTOR" "$PRODUCER" || { phase "FAIL producer is not the typed loader descendant"; exit 1; }
git -C "$ROOT" merge-base --is-ancestor "$PRODUCER_CORE" "$PRODUCER" || { phase "FAIL producer is not the core descendant"; exit 1; }
git -C "$ROOT" archive "$PRODUCER" Cargo.toml Cargo.lock crates tools/pg-builder tools/description-pack tools/description-review | tar -C "$SRC/d0cc" -xf -
git -C "$ROOT" archive "$CONSUMER" Cargo.toml Cargo.lock .cargo crates tools/operator-preview tools/gateway-audit-wal tools/gateway-token-store tools/minimal-product-loop tools/pg-builder tools/pi-harness tools/description-pack tools/description-review testing/real-db/pg-e2e | tar -C "$SRC/consumer" -xf -
if [[ -f "$SRC/d0cc/rust-toolchain.toml" || -f "$SRC/consumer/rust-toolchain.toml" ]]; then
  phase "FAIL archive toolchain"
  exit 1
fi
grep -q '"bytea"' "$SRC/consumer/crates/oo-gateway/src/policy_catalog.rs" || { phase "FAIL consumer archive types"; exit 1; }
{
  printf 'producer %s\n' "$PRODUCER"
  printf 'producer_core %s\n' "$PRODUCER_CORE"
  printf 'typed_ancestor %s\n' "$TYPED_ANCESTOR"
  printf 'consumer %s\n' "$CONSUMER"
  printf 'operator_sha_pin %s\n' "$OPERATOR_SHA"
  printf 'compile_input git-archive mounted at /src/d0cc\n'
  printf 'bit_identical_to_pin not_claimed\n'
  shasum -a 256 "$SRC/d0cc/Cargo.lock" "$SRC/consumer/Cargo.lock"
} > "$OUT/archive-note.txt"
printf '%s\n' \
  "whCode stays in the sealed catalog and the physical oracle." \
  "The frozen operator policy grammar accepts only lowercase plain identifiers." \
  "The physical mapping stores q_7768436f6465 for whCode, which is not the schema-digest name." \
  "This role visibility therefore publishes the 18 plain-ident columns, including the 12-byte and 16-byte bytea columns." \
  "A separate operator call names whCode and records that refusal." \
  > "$OUT/operator/whcode-binding.txt"

python3 - "$BIND_PLAN" "$PLAN" "$SCHEMA_DIGEST" "$ARROW" << 'PY'
import hashlib, json, shutil, sys
from pathlib import Path
src = Path(sys.argv[1])
dst = Path(sys.argv[2])
schema_digest = sys.argv[3]
arrow = Path(sys.argv[4]).read_bytes()
for name in ("report.json", "config.json", "columns.json", "capture-receipt.json", "schema.json"):
    shutil.copyfile(src / name, dst / name)
previous = json.loads((src / "digests.json").read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
report = (dst / "report.json").read_bytes()
config = (dst / "config.json").read_bytes()
capture = (dst / "capture-receipt.json").read_bytes()
assert digest(report) == previous["report_sha256"]
assert digest(config) == previous["config_digest"]
assert digest(capture) == previous["capture_receipt_sha256"]
assert digest(arrow) == previous["input_digest"]
body = json.loads(report)
assert body["source_id"] == "oo_w141"
assert "clickhouse" not in json.dumps(body)
digests = dict(previous)
digests["schema_digest"] = schema_digest
catalog = {
    "protocol": "oo.pipeline.sealed-import-catalog/v1",
    "engine": "mongodb",
    "source_id": "oo_w141",
    "source_generation": 1,
    "config_digest": digests["config_digest"],
    "schema_digest": schema_digest,
    "type_digest": digests["type_digest"],
    "input_digest": digests["input_digest"],
    "state": "not_sealed",
    "capture": "identity_only",
}
text = json.dumps(catalog, indent=2) + "\n"
(dst / "catalog.json").write_text(text)
digests["catalog_sha256"] = digest(text.encode())
(dst / "digests.json").write_text(json.dumps(digests, indent=2) + "\n")
(dst / "schema-digest-note.txt").write_text(
    "schema_digest is the empty-caveat oid preimage for oo_w141.shipments.\n"
    "It is fixed before import. The catalog builder does not rewrite it.\n"
)
PY

python3 - "$PLAN" "$OUT" "$OWNER" << 'PY'
import json, sys
from pathlib import Path
plan = Path(sys.argv[1])
out = Path(sys.argv[2])
owner = sys.argv[3]
digests = json.loads((plan / "digests.json").read_text())
columns = json.loads((plan / "columns.json").read_text())
fields = []
for column in columns:
    fields.append({
        "name": column["name"],
        "logical_name": column["name"],
        "arrow_type": column["arrow_type"],
        "nullable": column["nullable"],
        "precision": column["precision"],
        "scale": column["scale"],
    })
def document(owner_value, password_file):
    return {
        "protocol": "oo.pipeline.native-arrow-import/v1",
        "source_engine": "mongodb",
        "source_id": "oo_w141",
        "source_generation": 1,
        "snapshot": {"kind": "token", "value": "oo-dl01w1-41-frozen-batch"},
        "target_storage": "postgresql",
        "catalog_identity": {
            "config_digest": digests["config_digest"],
            "schema_digest": digests["schema_digest"],
            "type_digest": digests["type_digest"],
            "input_digest": digests["input_digest"],
        },
        "tables": [{
            "logical_schema": "oo_w141",
            "logical_table": "shipments",
            "row_count": 16,
            "batch_sha256": digests["arrow_sha256"],
            "arrow_path": "/frozen/batch.arrow",
            "columns": fields,
        }],
        "target": {
            "host": "oo-dl01w1-41-pg",
            "port": 5432,
            "database": "oo_w141",
            "runtime_principal": "oo_w141_admin",
            "password_file": password_file,
            "ca_file": "/secrets/ca.crt",
            "owner": owner_value,
            "shard": "oo_w141",
        },
    }
positive = document(owner, "/secrets/admin-password")
negative = document("not-a-uuid", "/secrets/admin-password")
for name, body in (("manifest-positive.json", positive), ("manifest-negative.json", negative)):
    text = json.dumps(body, indent=2) + "\n"
    if "secret://" in text or "clickhouse_268" in text or "admission_bin" in text:
        raise SystemExit("manifest identity")
    (plan / name).write_text(text)
PY

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
if [[ "$admitted" -ne 1 ]]; then
  phase "FAIL rust slot"
  exit 1
fi

phase "container"
mkdir -p "$SECRETS"
chmod 700 "$SECRETS"
if [[ ! -e "$SECRETS/llm-token" ]]; then
  : > "$SECRETS/llm-token"
  chmod 600 "$SECRETS/llm-token"
fi
docker volume create "${PREFIX}-cargo" >/dev/null
docker volume create "${PREFIX}-target" >/dev/null
docker network inspect "$NET" >/dev/null 2>&1 || docker network create --subnet 10.41.41.0/24 --label project=open-ontology --label owner=dl01-w1-41 --label session="$OWNER" --label task=DL01-W1-41 "$NET" >/dev/null
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
  -e CARGO_HOME=/cargo \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_BUILD_JOBS=1 \
  -e OO_OPERATOR_SRC=/src/consumer \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC/d0cc:/src/d0cc:ro" \
  -v "$SRC/consumer:/src/consumer:ro" \
  -v "$ROOT/tools/mongodb-preview:/work/tools/mongodb-preview:ro" \
  -v "$OUT:/evidence" \
  -v "$ADMISSION:/admission/oo-source-admission:ro" \
  -v "$RECEIPT:/frozen/native-readonly-receipt.json:ro" \
  -v "$ARROW:/frozen/batch.arrow:ro" \
  -v "$SECRETS:/secrets:ro" \
  -v "$TOKEN_FILE:/secrets/llm-token:ro" \
  "$RUST_IMAGE" \
  bash /work/tools/mongodb-preview/scripts/typed-file-inside.sh > "$OUT/docker-run.txt" || { phase "FAIL docker run"; exit 1; }

phase "wait build"
waited=0
while [[ ! -f "$OUT/build-ready" ]]; do
  if [[ -f "$OUT/build-failed" ]]; then
    phase "FAIL build"
    exit 1
  fi
  if ! docker ps -q --filter "name=^${RUST}$" | grep -q .; then
    phase "FAIL container exited"
    exit 1
  fi
  if [[ "$waited" -ge 7200 ]]; then
    phase "FAIL build timeout"
    exit 1
  fi
  sleep 20
  waited=$((waited + 20))
done
phase "build ready"
grep -q '^rustc 1.94.0 ' "$OUT/build/rustc.txt"

reuse_pg=0
if docker inspect "$PG" >/dev/null 2>&1 && [[ -s "$SECRETS/admin-password" && -s "$SECRETS/ca.crt" ]]; then
  reuse_pg=1
  phase "reuse sealed postgres; password file and container stay"
fi
if [[ "$reuse_pg" -eq 0 ]]; then
phase "secrets"
openssl rand -hex 24 > "$SECRETS/admin-password"
chmod 600 "$SECRETS/admin-password"
cat > "$SECRETS/ca.cnf" << 'EOF'
[req]
distinguished_name = dn
x509_extensions = v3_ca
prompt = no
[dn]
CN = oo-dl01w1-41-ca
[v3_ca]
basicConstraints = critical,CA:TRUE
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
EOF
cat > "$SECRETS/server.cnf" << 'EOF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = oo-dl01w1-41-pg
[v3_req]
basicConstraints = CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:oo-dl01w1-41-pg
subjectKeyIdentifier = hash
EOF
openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
  -keyout "$SECRETS/ca.key" -out "$SECRETS/ca.crt" \
  -config "$SECRETS/ca.cnf" >/dev/null 2>&1 || { phase "FAIL ca"; exit 1; }
openssl req -newkey rsa:2048 -nodes \
  -keyout "$SECRETS/server.key" -out "$SECRETS/server.csr" \
  -config "$SECRETS/server.cnf" >/dev/null 2>&1 || { phase "FAIL csr"; exit 1; }
openssl x509 -req -in "$SECRETS/server.csr" \
  -CA "$SECRETS/ca.crt" -CAkey "$SECRETS/ca.key" -CAcreateserial \
  -out "$SECRETS/server.crt" -days 2 \
  -extfile "$SECRETS/server.cnf" -extensions v3_req >/dev/null 2>&1 || { phase "FAIL leaf"; exit 1; }
cat "$SECRETS/server.crt" "$SECRETS/ca.crt" > "$SECRETS/server-full.crt"
chmod 600 "$SECRETS/server.key" "$SECRETS/ca.key"
openssl x509 -in "$SECRETS/server.crt" -noout -ext subjectAltName | grep -q 'DNS:oo-dl01w1-41-pg' || { phase "FAIL san"; exit 1; }
cp "$SECRETS/ca.crt" "$PLAN/ca.crt"
cat > "$SECRETS/pg-entry.sh" << 'EOF'
#!/bin/bash
set -euo pipefail
cp /run/secrets/server.key /tmp/server.key
cp /run/secrets/server.crt /tmp/server.crt
chown postgres:postgres /tmp/server.key /tmp/server.crt
chmod 600 /tmp/server.key
chmod 644 /tmp/server.crt
exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.crt -c ssl_key_file=/tmp/server.key -c listen_addresses='*'
EOF
chmod 755 "$SECRETS/pg-entry.sh"

ids="$(docker ps -q)"
cpus=0
if [[ -n "$ids" ]]; then
  cpus="$(docker inspect -f '{{.HostConfig.NanoCpus}}' $ids | awk '{s+=$1} END {print s+0}')"
fi
if [[ "$cpus" -gt 15000000000 ]]; then
  phase "FAIL cpu cap before postgres"
  exit 1
fi

phase "postgres"
docker volume create "${PREFIX}-pgdata" \
  --label project=open-ontology --label owner=dl01-w1-41 \
  --label session="$OWNER" --label task=DL01-W1-41 --label kind=postgres-target >/dev/null
docker rm -f "$PG" >/dev/null 2>&1 || true
docker run -d --name "$PG" \
  --platform linux/arm64 --pull never \
  --memory 1g --cpus 1 --pids-limit 256 \
  --network "$NET" --network-alias oo-dl01w1-41-pg \
  -v "${PREFIX}-pgdata:/var/lib/postgresql/data" \
  --label owner=dl01-w1-41 \
  --label session="$OWNER" \
  --label project=open-ontology \
  --label task=DL01-W1-41 \
  -e POSTGRES_USER=oo_w141_admin \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/admin-password \
  -e POSTGRES_DB=oo_w141 \
  -v "$SECRETS/admin-password:/run/secrets/admin-password:ro" \
  -v "$SECRETS/server.key:/run/secrets/server.key:ro" \
  -v "$SECRETS/server-full.crt:/run/secrets/server.crt:ro" \
  -v "$SECRETS/pg-entry.sh:/pg-entry.sh:ro" \
  "$PG_IMAGE" \
  bash /pg-entry.sh > "$OUT/docker-pg-run.txt"
else
  if [[ "$(docker inspect -f '{{.State.Status}}' "$PG")" != "running" ]]; then
    docker start "$PG" > "$OUT/docker-pg-start.txt"
  fi
fi
ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG" pg_isready -h 127.0.0.1 -p 5432 -U oo_w141_admin -d oo_w141 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [[ "$ready" -ne 1 ]]; then
  phase "FAIL postgres"
  exit 1
fi

if [[ "$reuse_pg" -eq 1 ]]; then
  phase "init-pg skipped; sealed database stays"
  printf '%s\n' "0" > "$OUT/init-pg.exit"
  printf '%s\n' '{"protocol":"oo.pipeline.init-pg-result/v1","install":"reused"}' > "$OUT/init-pg.json"
  init_ok=1
else
phase "init-pg"
init_ok=0
for _ in $(seq 1 8); do
  docker exec -i "$RUST" /target/pipeline/debug/init-pg > "$OUT/init-pg.json" 2> "$OUT/init-pg.stderr" << 'EOF'
{"protocol":"oo.pipeline.init-pg/v1","target":{"host":"oo-dl01w1-41-pg","port":5432,"database":"oo_w141","administrator":"oo_w141_admin","password_file":"/secrets/admin-password","ca_file":"/secrets/ca.crt"}}
EOF
  echo $? > "$OUT/init-pg.exit"
  if [[ "$(cat "$OUT/init-pg.exit")" == "0" ]]; then
    init_ok=1
    break
  fi
  sleep 2
done
if [[ "$init_ok" -ne 1 ]]; then
  phase "FAIL init-pg"
  exit 1
fi
fi
printf '%s\n' \
  "runtime_principal is oo_w141_admin, the configured target administrator passed to init-pg." \
  "The operator login role is oo_r_w141_reader: NOLOGIN and no inherit, with column SELECT only." \
  "oo_w141_admin remains the import principal. Schema-wide table GRANT is not issued." \
  > "$OUT/role-choice.txt"
docker exec -i "$PG" psql -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT shard_id||'|'||state||'|'||source_id||'|'||source_generation||'|'||target_generation FROM _oo.full_import_run ORDER BY 1" \
  > "$OUT/pg-before.txt"

phase "negative import"
reg_sha="$(tr -d '[:space:]' < "$OUT/registry.sha256")"
docker exec -e OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
  -e OO_NATIVE_BINDING_REGISTRY=/evidence/registry.json \
  -e OO_NATIVE_BINDING_REGISTRY_SHA256="$reg_sha" \
  "$RUST" /target/pg/debug/pg-builder import-native \
  --manifest /evidence/plan/manifest-negative.json \
  --out /evidence/negative.json \
  --pipeline /target/pipeline/debug/oo-pipeline \
  --stderr /evidence/negative.stderr > "$OUT/negative.invoke" 2>&1
echo $? > "$OUT/negative.exit"
python3 "$ROOT/tools/mongodb-preview/scripts/typed-file-oracle.py" negative "$OUT" > "$OUT/negative-oracle.out"
echo $? > "$OUT/negative-oracle.exit"
if [[ "$(cat "$OUT/negative-oracle.exit")" != "0" ]]; then
  phase "FAIL negative changed runs"
  exit 1
fi

phase "positive import"
docker exec -e OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
  -e OO_NATIVE_BINDING_REGISTRY=/evidence/registry.json \
  -e OO_NATIVE_BINDING_REGISTRY_SHA256="$reg_sha" \
  "$RUST" /target/pg/debug/pg-builder import-native \
  --manifest /evidence/plan/manifest-positive.json \
  --out /evidence/positive.json \
  --pipeline /target/pipeline/debug/oo-pipeline \
  --stderr /evidence/positive.stderr > "$OUT/positive.invoke" 2>&1
echo $? > "$OUT/positive.exit"
python3 "$ROOT/tools/mongodb-preview/scripts/typed-file-oracle.py" positive "$OUT" > "$OUT/positive-oracle.out"
echo $? > "$OUT/positive-oracle.exit"

phase "catalog"
docker exec -i "$PG" psql -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT coalesce(json_agg(row_to_json(t)), '[]'::json) FROM _oo.physical_mappings t WHERE source='oo_w141'" \
  > "$OUT/physical-mappings.json"
docker exec -i "$PG" psql -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT coalesce(json_agg(row_to_json(t)), '[]'::json) FROM _oo.full_import_run t WHERE source_id='oo_w141'" \
  > "$OUT/sealed-run.json"
if python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); raise SystemExit(0 if d.get("import",{}).get("load_state")=="awaiting_publish" else 1)' "$OUT/positive.json"; then
  python3 "$ROOT/tools/mongodb-preview/scripts/typed-file-catalog.py" "$OUT"
else
  phase "positive not awaiting_publish"
fi

phase "builder files"
python3 - "$OUT" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
builder = out / "builder"
columns = ["_id","code","flag","units","price","created","uid","blob","blob_presence","weight","warehouse","lane","memo","memo_presence","marks","marks_presence","pack","pack_presence"]
settings = {
    "catalog_version": "pending",
    "knowledge": {"id": "oo_w141-shipments", "version": "1"},
    "ontology": {
        "classification": {"auto_accept": "rule_high_confidence"},
        "compile": {"budgets": {"readme_tokens": 2000, "table_file_tokens": 4000}, "per_role": True, "sample_values": {"default": "masked", "max_per_column": 0}},
        "default_tier_preference": ["raw"],
        "include_schemas": ["oo_w141"],
        "lifecycle": {"deprecation_min_releases": 2},
        "locale": "en-US",
        "name": "oo_w141",
        "publish": {"include_unreviewed": []},
        "spec_version": "oos/v1",
    },
    "ontology_version": "ont-1",
    "policy": {"id": "oo_w141-read", "version": "1"},
    "project": "oo_w141",
}
visibility = {
    "catalog_version": "pending",
    "datasets": [{
        "columns": columns,
        "dataset": "oo_w141.shipments",
        "global_stats": "denied",
        "row_security": "none",
    }],
    "issued_for_compiler": "oo-builder/role-pack/v1",
    "knowledge": {"id": "oo_w141-shipments", "version": "1"},
    "limits": {"max_entries": 128, "max_file_bytes": 262144, "max_total_bytes": 1048576},
    "ontology_version": "ont-1",
    "policy": {"id": "oo_w141-read", "version": "1"},
    "principal": "oo_w141_admin",
    "project": "oo_w141",
    "protocol": "oo.builder.role-visibility/v1",
    "role": "oo_r_w141_reader",
}
def config(deadline, price):
    return {
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "sample_policy": "none",
        "temperature": 0,
        "purpose": "generate",
        "endpoint": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions"},
        "secret": {"bearer_file": "/secrets/llm-token"},
        "budget": {
            "deadline_ms": deadline,
            "max_completion_tokens": 4000,
            "max_in_flight": 1,
            "max_prompt_tokens": 8000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 12000,
        },
        "declared_price": price,
    }
declared = {"status": "declared_non_billing", "currency": "USD", "per_total_token": "0.000001"}
(builder / "config-401.json").write_text(json.dumps(config(30000, declared), indent=2) + "\n")
(builder / "config-pack.json").write_text(json.dumps(config(180000, declared), indent=2) + "\n")
(builder / "config-unknown.json").write_text(json.dumps(config(180000, {"status": "unknown"}), indent=2) + "\n")
(builder / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")
(builder / "visibility.json").write_text(json.dumps(visibility, indent=2) + "\n")
note = {
    "reviewer_cli": "dl01-w1-41-operator",
    "reviewed_at": "2026-10-11",
    "means": "CLI stamp passed to pg-builder sealed. Not an independent review.",
    "fixed_independent_review": "not_run",
    "declared_price": "declared_non_billing USD 0.000001 is the e4f test estimate, not a billing certificate",
    "unknown_price": "separate call, pack not_produced, no success side effect",
    "preserved_timeout": "raw/target-admin builder deadline 120000 unknown price, not repeated",
    "one_model_retry": "config-pack.json deadline 180000 fresh cache, same max_completion_tokens 4000 as the preserved timeout",
    "deadline_not_raised": "180000 remains the contract maximum; it was not increased",
    "declared_price_is_not_a_bill": True,
    "proxy": "owned forwarder keeps the upstream write side open, then shuts the client write side after one complete response so pg-builder can observe EOF",
}
(builder / "reviewer-stamp.json").write_text(json.dumps(note, indent=2) + "\n")
customer = {
    "protocol": "oo.source-customer-input/v1",
    "source_id": "oo_w141",
    "connection": {
        "secret_ref": "secret://oo-dl01w1-41/reader",
        "private_config": "plan/manifest-positive.json",
        "ca_ref": "plan/ca.crt",
    },
    "namespace": "oo_w141",
    "approved_objects": ["oo_w141.shipments"],
    "approved_fields": [
        "_id", "code", "flag", "units", "price", "created", "uid", "blob", "blob_presence",
        "weight", "warehouse", "whCode", "lane", "memo", "memo_presence", "marks",
        "marks_presence", "pack", "pack_presence"
    ],
    "budgets": config(180000, declared)["budget"],
    "budget_limit": 16,
    "role": "read",
    "model": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317", "bearer_file": "/secrets/llm-token", "cli": "pg-builder sealed"},
    "operator_cli": "operator-preview describe|query|cancel",
}
(out / "customer-input.json").write_text(json.dumps(customer, indent=2) + "\n")
PY

start_proxy() {
  docker exec "$RUST" bash -c 'if [ -f /tmp/proxy.pid ]; then kill "$(cat /tmp/proxy.pid)" 2>/dev/null || true; rm -f /tmp/proxy.pid; fi'
  sleep 1
  docker exec -d -e OO_PROXY_MODE="$1" -e OO_UPSTREAM=host.docker.internal:8317 "$RUST" \
    bash -c 'echo $$ > /tmp/proxy.pid; exec /target/proxy/loopback-proxy >> /evidence/builder/proxy.log 2>&1'
  sleep 1
}

run_sealed() {
  local name="$1" config="$2" cache="$3"
  mkdir -p "$OUT/builder/$name"
  rm -rf "$OUT/builder/$cache"
  mkdir -p "$OUT/builder/$cache"
  if [[ ! -f "$OUT/sealed-catalog.json" ]]; then
    printf '%s\n' "sealed catalog was not written" > "$OUT/builder/$name/sealed.skipped"
    return
  fi
  docker exec "$RUST" /target/pg/debug/pg-builder sealed \
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

cat > "$OUT/builder/prior-timeout-diagnosis.txt" << 'EOF'
Preserved pack result raw/consumer-closure/builder/pack/result.json: model called, socket opened, http_status 0, reason timeout, pack not_produced. Phase clock 2026-10-10T19:56:19Z to 2026-10-10T19:59:22Z matches deadline_ms 180000. The 401 call on the same proxy returned http_status 401 in about 2 seconds.
pg-builder http.rs wraps connect, write, and read-until-EOF in one deadline. A timeout discards any bytes already read and stores http_status 0. refusal unavailable would mean connect or write failed before the deadline. The recorded reason is timeout, so the exchange was still open at 180s.
The previous owned forwarder copied bytes in both directions and joined the client-read thread before dropping the client socket. pg-builder does not close its write side until it sees EOF. That waits forever even after the upstream response is complete. The 401 mode returned and dropped the socket, which is why that call finished. Shutting the upstream write side after the request is a client FIN. The product proxy answers that FIN with HTTP 499 and context canceled, before a model body exists. This retry uses the same catalog input, the same declared_non_billing estimate, and the same 180000 deadline. The forwarder reads one request, writes it, leaves the upstream write side open, reads one response to content-length, chunked end, or EOF, then shuts down the client write side. Phase lines record accept, connect, request bytes, first response byte, status, and shutdown. They do not record the bearer or the body.
EOF
phase "builder 401"
start_proxy 401
run_sealed probe-401 config-401.json cache-401
phase "builder pack"
start_proxy forward
run_sealed pack config-pack.json cache-pack
pack_outcome="$(python3 - "$OUT/builder/pack/result.json" << 'PY'
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
if not path.exists():
    print("missing")
    raise SystemExit
doc = json.loads(path.read_text())
print("|".join([
    str(doc.get("outcome")),
    str(doc.get("reason")),
    str(doc.get("pack")),
    str(doc.get("model")),
]))
PY
)"
printf '%s\n' "$pack_outcome" > "$OUT/builder/pack-outcome.txt"
case "$pack_outcome" in
  *timeout*)
    printf '%s\n' "cost_unknown was not called. The one pack retry timed out. The preserved unknown-price run also timed out before cost_unknown." > "$OUT/builder/cost-unknown-skipped.txt"
    ;;
  missing|compiler_refused*)
    printf '%s\n' "cost_unknown was not called. The pack attempt did not return a model completion." > "$OUT/builder/cost-unknown-skipped.txt"
    ;;
  *)
    phase "builder cost_unknown"
    run_sealed cost-unknown config-unknown.json cache-unknown
    ;;
esac

phase "operator"
if [[ ! -x "$OUT/../.." ]]; then
  :
fi
if ! docker exec "$RUST" test -x "$OPBIN"; then
  printf '%s\n' "operator binary was not produced" > "$OUT/operator/binary-missing.txt"
else
  docker exec "$RUST" "$OPBIN" --help > "$OUT/operator/help.stdout" 2> "$OUT/operator/help.stderr"
  echo $? > "$OUT/operator/help.exit"
fi
if [[ -f "$OUT/builder/pack/authority.json" && -f "$OUT/builder/pack/result.json" ]]; then
  python3 - "$OUT" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
doc = json.loads((out / "builder/pack/result.json").read_text())
(out / "operator/pack-ready.txt").write_text(
    f"outcome={doc.get('outcome')} pack={doc.get('pack')}\n"
)
PY
fi
if [[ -f "$OUT/builder/pack/authority.json" ]] && docker exec "$RUST" test -x "$OPBIN"; then
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
quoted = ", ".join(f"\"{row['physical_name']}\"" for row in columns)
sql = (
    "CREATE ROLE oo_r_w141_reader NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOLOGIN NOBYPASSRLS NOREPLICATION;\n"
    f"GRANT USAGE ON SCHEMA \"_oo\" TO oo_r_w141_reader;\n"
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
wh = dict(query)
wh["sql"] = "SELECT whCode FROM oo_w141.shipments"
(out / "operator/whcode-query.json").write_text(json.dumps(wh, indent=2) + "\n")
(out / "operator/cancel-query.json").write_text(json.dumps(query, indent=2) + "\n")
PY
  docker exec -i "$PG" psql -U oo_w141_admin -d oo_w141 -v ON_ERROR_STOP=1 -f - < "$OUT/operator/grants.sql" > "$OUT/operator/grants.out" 2> "$OUT/operator/grants.err"
  echo $? > "$OUT/operator/grants.exit"
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
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/whcode-query.json > "$OUT/operator/whcode.stdout" 2> "$OUT/operator/whcode.stderr"
  echo $? > "$OUT/operator/whcode.exit"
  openssl rand -hex 24 > "$SECRETS/nobody-password"
  chmod 600 "$SECRETS/nobody-password"
  python3 - "$SECRETS/nobody-password" "$SECRETS/create-nobody.sql" << 'PY'
import sys
from pathlib import Path
password = Path(sys.argv[1]).read_text().strip().replace("'", "''")
Path(sys.argv[2]).write_text(
    "CREATE ROLE oo_w141_nobody LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '"
    + password
    + "';\n"
)
PY
  docker cp "$SECRETS/create-nobody.sql" "$PG:/tmp/create-nobody.sql"
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -v ON_ERROR_STOP=1 -f /tmp/create-nobody.sql > "$OUT/operator/nobody-create.out" 2> "$OUT/operator/nobody-create.err"
  echo $? > "$OUT/operator/nobody-create.exit"
  docker exec "$PG" rm -f /tmp/create-nobody.sql
  table="$(tr -d '[:space:]' < "$OUT/operator/physical-table.txt")"
  docker cp "$SECRETS/nobody-password" "$PG:/tmp/nobody-password"
  docker exec "$PG" bash -c "export PGPASSWORD=\$(cat /tmp/nobody-password); rm -f /tmp/nobody-password; psql -U oo_w141_nobody -d oo_w141 -c 'SELECT count(*) FROM \"_oo\".\"${table}\"'" > "$OUT/operator/nobody-select.out" 2> "$OUT/operator/nobody-select.err"
  echo $? > "$OUT/operator/nobody-select.exit"
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
  docker exec "$PG" psql -U oo_w141_admin -d oo_w141 -c "SELECT pid, state, wait_event_type, left(query, 160) FROM pg_stat_activity WHERE datname='oo_w141'" > "$OUT/operator/cancel-activity.txt" 2>&1 || true
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
  phase "source off query"
  docker stop "${PREFIX}-mongodb" > "$OUT/operator/source-off-stop.out" 2> "$OUT/operator/source-off-stop.err" || true
  docker inspect -f '{{.State.Status}}' "${PREFIX}-mongodb" > "$OUT/operator/source-off-status.txt" 2> "$OUT/operator/source-off-inspect.err" || true
  docker exec "${PREFIX}-mongodb" mongosh --quiet --eval 'quit(0)' > "$OUT/operator/source-off-ping.out" 2> "$OUT/operator/source-off-ping.err" || true
  echo $? > "$OUT/operator/source-off-ping.exit"
  docker exec "$RUST" "$OPBIN" query --authority /evidence/builder/pack/authority.json --query /evidence/operator/query.json > "$OUT/operator/source-off-query.stdout" 2> "$OUT/operator/source-off-query.stderr"
  echo $? > "$OUT/operator/source-off-query.exit"
  if [[ "$(cat "$OUT/operator/source-off-query.exit")" == "0" ]]; then
    python3 "$ROOT/tools/mongodb-preview/scripts/consumer-oracle.py" "$OUT/operator/source-off-query.stdout" "$DECODED" > "$OUT/operator/source-off-oracle.out"
    echo $? > "$OUT/operator/source-off-oracle.exit"
  fi
  phase "source reopen"
  docker start "${PREFIX}-mongodb" > "$OUT/operator/source-reopen.out" 2> "$OUT/operator/source-reopen.err"
  echo $? > "$OUT/operator/source-reopen.exit"
  if [[ "$(cat "$OUT/operator/source-reopen.exit")" == "0" ]]; then
    reopen_ready=0
    for _ in $(seq 1 20); do
      sleep 2
      if docker exec -u 0 -e HOME=/tmp "${PREFIX}-mongodb" \
        mongosh --quiet --norc 'mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-reopen' \
        --eval 'const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/reader","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_reader", pw)) quit(2); const n=db.getSiblingDB("oo_w141").shipments.find({}, {code:1}).limit(16).toArray().length; fs.writeFileSync("/tmp/oo-reopen-count.txt", "rows="+n+" role=read\n"); if (n!==16) quit(3);' \
        > "$OUT/operator/source-reopen-count.out" 2> "$OUT/operator/source-reopen-count.err"; then
        reopen_ready=1
        break
      fi
    done
    docker cp "${PREFIX}-mongodb:/tmp/oo-reopen-count.txt" "$OUT/operator/source-reopen-count.txt" >/dev/null 2>&1 || true
    docker exec -u 0 "${PREFIX}-mongodb" rm -f /tmp/oo-reopen-count.txt >/dev/null 2>&1 || true
    printf '%s\n' "$reopen_ready" > "$OUT/operator/source-reopen-ready.txt"
  fi
else
  printf '%s\n' "operator query was not started because the pack authority or the binary is absent" > "$OUT/operator/not-started.txt"
fi

phase "recorded"
exit 0
