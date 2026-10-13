#!/bin/bash
# Own oo_w141 physical import. Fixed d0cc and f235 archives. No git worktree.
# Does not join or query oo-dl01w1-06-pg.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$EVIDENCE/raw/target-admin"
PLAN="$OUT/plan"
SRC="/tmp/oo-dl01w1-41-typed-src"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
ARROW="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow"
RECEIPT="$ROOT/evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json"
ADMISSION="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission"
BIND_PLAN="$EVIDENCE/raw/native-bind/plan"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="postgres@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f"
D0CC="d0cc59eb570bb49e7b911ec633d40799af8a3d06"
F235="f2357a026033fbac5beb20b4bf8555d808881fe7"
OWNER="23df6145-4592-4dc0-82df-fd226dd020fd"
SCHEMA_DIGEST="6ddbefe926004eec4f9bccba8ad266169ba2077739b8005396046d5ea0e76386"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-typed"
PG="${PREFIX}-pg"
NET="${PREFIX}-net"
TOKEN_FILE="/tmp/oo-customer-demo-20261007/llm-token"

phase() {
  mkdir -p "$OUT"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$OUT/phase.txt"
}

cleanup() {
  local ec=$?
  phase "cleanup $ec"
  docker logs "$RUST" > "$OUT/docker-rust.log" 2>&1 || true
  docker logs "$PG" > "$OUT/docker-pg.log" 2>&1 || true
  docker rm -f "$RUST" "$PG" "${PREFIX}-mongodb" "${PREFIX}-read" > "$OUT/docker-rm.txt" 2>&1 || true
  docker network rm "$NET" > "$OUT/docker-network-rm.txt" 2>&1 || true
  docker volume rm "${PREFIX}-cargo" "${PREFIX}-target" "${PREFIX}-data" > "$OUT/docker-volume-rm.txt" 2>&1 || true
  python3 "$ADMIT" release all > "$OUT/release.json" 2>&1 || true
  {
    echo "left-running:"
    docker ps --format '{{.Names}}' | grep -E 'oo-dl01w1-0(6|4)' || true
    echo "own-left:"
    docker ps -a --format '{{.Names}} {{.Status}}' | grep "$PREFIX" || true
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
  printf '%s\n' "$ec" > "$OUT/status.txt"
}
trap cleanup EXIT

phase "prepare"
mkdir -p "$PLAN" "$OUT/operator" "$OUT/source-read" "$SRC" "$SECRETS"
chmod 700 "$SECRETS"
test -f "$TOKEN_FILE"
test -f "$ADMISSION"
test -f "$ARROW"
test -f "$RECEIPT"
phase "source read"
if ! TYPED_READ_OUT="$OUT/source-read" bash "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/typed-file-read.sh"; then
  phase "FAIL source read"
  exit 1
fi
rm -rf "$SRC/d0cc" "$SRC/f235"
mkdir -p "$SRC/d0cc" "$SRC/f235"
git -C "$ROOT" archive "$D0CC" Cargo.toml Cargo.lock crates tools/pg-builder tools/description-pack tools/description-review | tar -C "$SRC/d0cc" -xf -
git -C "$ROOT" archive "$F235" Cargo.toml Cargo.lock crates tools/operator-preview tools/gateway-audit-wal tools/minimal-product-loop tools/pg-builder tools/pi-harness tools/description-pack tools/description-review testing/real-db/pg-e2e | tar -C "$SRC/f235" -xf -
if [[ -f "$SRC/d0cc/rust-toolchain.toml" || -f "$SRC/f235/rust-toolchain.toml" ]]; then
  phase "FAIL archive toolchain"
  exit 1
fi
printf 'd0cc %s\nf235 %s\n' "$D0CC" "$F235" > "$OUT/archive-note.txt"

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
# The token overlay needs a mountpoint inside the secrets directory before it is read-only.
if [[ ! -e "$SECRETS/llm-token" ]]; then
  : > "$SECRETS/llm-token"
  chmod 600 "$SECRETS/llm-token"
fi
docker volume create "${PREFIX}-cargo" >/dev/null
docker volume create "${PREFIX}-target" >/dev/null
docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET" >/dev/null
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
  -e OO_SKIP_OPERATOR=1 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC/d0cc:/src/d0cc:ro" \
  -v "$SRC/f235:/src/f235:ro" \
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
  if ! docker ps -q --filter "name=${RUST}" | grep -q .; then
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

phase "secrets"
mkdir -p "$SECRETS"
chmod 700 "$SECRETS"
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
docker rm -f "$PG" >/dev/null 2>&1 || true
docker run -d --name "$PG" \
  --platform linux/arm64 --pull never \
  --memory 1g --cpus 1 --pids-limit 256 \
  --network "$NET" --network-alias oo-dl01w1-41-pg \
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
printf '%s\n' \
  "runtime_principal is oo_w141_admin, the configured target administrator passed to init-pg." \
  "import-native reads the shadow count as that principal." \
  "The fixed loader grants INSERT on the new shadow to oo_pipeline_runtime." \
  "This run does not GRANT on schema _oo and does not create another login." \
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

phase "builder"
mkdir -p "$OUT/builder"
python3 - "$OUT/builder" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
columns = ["_id","code","flag","units","price","created","uid","blob","blob_presence","weight","warehouse","whCode","lane","memo","memo_presence","marks","marks_presence","pack","pack_presence"]
config = {
    "protocol": "oo.pg-builder.config/v1",
    "adapter": "loopback_http_local_dev",
    "namespace": "pg-builder",
    "sample_policy": "none",
    "temperature": 0,
    "purpose": "generate",
    "endpoint": {"model_id": "grok-4.7", "origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions"},
    "secret": {"bearer_file": "/secrets/llm-token"},
    "budget": {
        "deadline_ms": 120000,
        "max_completion_tokens": 4000,
        "max_in_flight": 1,
        "max_prompt_tokens": 8000,
        "max_request_bytes": 262144,
        "max_response_bytes": 262144,
        "max_tables": 16,
        "max_total_tokens": 12000,
    },
    "declared_price": {"status": "unknown"},
}
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
(out / "config.json").write_text(json.dumps(config, indent=2) + "\n")
(out / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")
(out / "visibility.json").write_text(json.dumps(visibility, indent=2) + "\n")
note = {
    "reviewer_cli": "dl01-w1-41-operator",
    "reviewed_at": "2026-10-10",
    "means": "CLI stamp passed to pg-builder sealed. Not an independent review.",
    "fixed_independent_review": "not_run",
    "declared_price": "unknown",
}
(out / "reviewer-stamp.json").write_text(json.dumps(note, indent=2) + "\n")
PY
if [[ -f "$OUT/sealed-catalog.json" ]]; then
  docker exec -d -e OO_UPSTREAM=host.docker.internal:8317 "$RUST" /target/proxy/loopback-proxy
  sleep 1
  docker exec "$RUST" /target/pg/debug/pg-builder sealed \
    --config /evidence/builder/config.json \
    --catalog /evidence/sealed-catalog.json \
    --settings /evidence/builder/settings.json \
    --visibility /evidence/builder/visibility.json \
    --cache /evidence/builder/cache \
    --out /evidence/builder/out \
    --reviewer dl01-w1-41-operator \
    --reviewed-at 2026-10-10 \
    --dataset oo_w141.shipments \
    --target-host oo-dl01w1-41-pg \
    --target-port 5432 \
    --target-database oo_w141 \
    --target-user oo_w141_admin \
    --login oo_w141_admin \
    --target-ca /secrets/ca.crt \
    --target-password /secrets/admin-password \
    > "$OUT/builder/sealed.invoke" 2>&1
  echo $? > "$OUT/builder/sealed.exit"
else
  printf '%s\n' "sealed catalog was not written" > "$OUT/builder/sealed.skipped"
fi

phase "operator"
printf '%s\n' "consumer not started. DL01-W1-21 fixed author handoff is the operator writer. f235 was not compiled on this run." > "$OUT/operator/waiting-21.txt"

phase "recorded"
exit 0
