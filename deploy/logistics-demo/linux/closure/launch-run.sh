#!/bin/bash
# Start the logistics product runner. Copies private files without printing them.
# Requires the offline product-check-2 proof. Does not seed or drop a database.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=${GPT_REPO_ROOT:-$(cd "$HERE/../../../.." && pwd)}
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a419
PROOF=${GPT_PROOF_FILE:-${SRC}/evidence/gpt-minimal-closure/build-13/proof.exit}
EVIDENCE=${SRC}/evidence/gpt-minimal-closure/${GPT_EVIDENCE_NAME:-deployment}
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=${GPT_RUNTIME_NAME:-oo-a419-product-run}
NET=${GPT_NETWORK:-oo-a419-eef831aa-net}
VOL=${GPT_STATE_VOLUME:-oo-a419-eef831aa-loop}
PRIV=${GPT_RUNTIME_PRIVATE_DIR:-/tmp/oo-a419-${RUN_ID}/runtime-private}
COPY_PRIV=${GPT_COPY_PRIVATE_DIR:-/tmp/oo-a419-${RUN_ID}/copy-private}
SEC=${GPT_SECRET_DIR:-/tmp/oo-a419-${RUN_ID}/secrets}
CFG=${GPT_CONFIG_DIR:-/tmp/oo-a419-${RUN_ID}/config}
SOURCE_CONTAINER=${GPT_SOURCE_CONTAINER-oo-a419-src-pg}
mkdir -p "$EVIDENCE"
if [ ! -f "$PROOF" ] || [ "$(tr -d '[:space:]' < "$PROOF")" != "0" ]; then
  echo "offline proof is not 0; refusing the product run" >&2
  printf '%s\n' 2 > "$EVIDENCE/launch.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a419 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/launch.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Names}}' | grep -q .; then
  echo "product runner already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/launch.exit"
  exit 2
fi
umask 077
mkdir -p "$PRIV" "$COPY_PRIV" "$CFG"
chmod 700 "$PRIV" "$COPY_PRIV" "$CFG"
install -m 600 "${GPT_SOURCE_PASSWORD_FILE:-$SEC/src/reader.pass}" "$COPY_PRIV/source-reader"
install -m 644 "${GPT_SOURCE_CA_FILE:-$SEC/src/ca.crt}" "$COPY_PRIV/source-ca.crt"
install -m 600 "${GPT_TARGET_ADMIN_PASSWORD_FILE:-$SEC/dst/admin.pass}" "$COPY_PRIV/target-admin"
install -m 600 "${GPT_TARGET_RUNTIME_PASSWORD_FILE:-$SEC/dst/runtime.pass}" "$COPY_PRIV/target-runtime"
install -m 600 "${GPT_TARGET_READER_PASSWORD_FILE:-$SEC/dst/reader.pass}" "$PRIV/target-reader"
install -m 600 "${GPT_TARGET_READER_PASSWORD_FILE:-$SEC/dst/reader.pass}" "$COPY_PRIV/target-reader"
install -m 644 "${GPT_TARGET_CA_FILE:-$SEC/dst/ca.crt}" "$PRIV/target-ca.crt"
install -m 644 "${GPT_TARGET_CA_FILE:-$SEC/dst/ca.crt}" "$COPY_PRIV/target-ca.crt"
install -m 600 "${GPT_LLM_BEARER_FILE:-/tmp/oo-customer-demo-20261007/llm-token}" "$PRIV/llm-token"
openssl rand -hex 16 > "$PRIV/gateway-marker"
chmod 600 "$PRIV/gateway-marker"
install -m 644 "${GPT_PRODUCT_TEMPLATE:-$SRC/deploy/logistics-demo/config.example.json}" "$CFG/product.json"
install -m 644 "${GPT_SOURCE_TEMPLATE:-$SRC/deploy/logistics-demo/source.example.json}" "$CFG/source.json"
install -m 644 "$SRC/deploy/logistics-demo/mirror-invoke.example.json" "$CFG/mirror-invoke.json"
# Read only public connection metadata, not secret bytes.
SOURCE_BLOCK_HOST=$(python3 - "$CFG/source.json" "$CFG/product.json" "$CFG/mirror-invoke.json" <<'PYSOURCE'
import json,sys,ipaddress
source=json.load(open(sys.argv[1]));product=json.load(open(sys.argv[2]));sidecar=json.load(open(sys.argv[3]))
if source['engine']!='postgresql' or product['database']['type']!='postgresql':
    raise SystemExit('unsupported: PostgreSQL is the only validated connector')
sidecar['admin_user']=source['target_admin_user']
open(sys.argv[3],'w').write(json.dumps(sidecar,indent=2)+'\n')
host=source['host']
try:ipaddress.ip_address(host);host=''
except ValueError:pass
if host==product['database']['host']:host=''
print(host)
PYSOURCE
)
SOURCE_HOST_ARGS=()
if [ -n "$SOURCE_BLOCK_HOST" ]; then SOURCE_HOST_ARGS=(--add-host "$SOURCE_BLOCK_HOST:127.0.0.2"); fi
docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=loop-state \
  "$VOL" >/dev/null
# The copy process has source credentials; the runtime never mounts this folder.
COPY_NAME=${GPT_COPY_NAME:-oo-a419-product-copy}
if docker container inspect "$COPY_NAME" >/dev/null 2>&1; then
  echo "copy runner already exists; preserve its receipt and choose a clean run" >&2
  exit 2
fi
set +e
docker run --name "$COPY_NAME" --platform linux/arm64 --pull never \
  --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
  --memory 1g --memory-swap 1g --cpus 1 --pids-limit 128 --network "$NET" \
  --mount "type=volume,source=${GPT_BINARY_VOLUME:-oo-a419-eef831aa-target},target=/target,readonly" \
  --mount "type=volume,source=${VOL},target=/var/lib/oo" \
  --mount "type=bind,source=${COPY_PRIV},target=/var/lib/oo/private,readonly" \
  --mount "type=bind,source=${CFG},target=/var/lib/oo/config,readonly" \
  --label oo.owner=a419 --label oo.kind=copy-runner "$IMAGE" \
  bash -c 'mkdir -p /var/lib/oo/copy; /target/loop/debug/minimal-product-loop copy --config /var/lib/oo/config/product.json && exec /target/loop/debug/minimal-product-loop prepare --config /var/lib/oo/config/product.json > /var/lib/oo/copy/prepare-receipt.json' \
  > "$EVIDENCE/copy.stdout" 2> "$EVIDENCE/copy.stderr"
copy_code=$?
set -e
printf '%s\n' "$copy_code" > "$EVIDENCE/copy.exit"
if [ "$copy_code" -ne 0 ]; then exit "$copy_code"; fi
docker cp "$COPY_NAME:/var/lib/oo/copy/manifest.json" "$CFG/sealed-manifest.json"
python3 - "$CFG" <<'PYREPLAY'
import json, pathlib, sys
config=pathlib.Path(sys.argv[1])
manifest=json.loads((config/'sealed-manifest.json').read_text())
sidecar=json.loads((config/'mirror-invoke.json').read_text())
replay={key:sidecar[key] for key in ('protocol','binary','plan','manifest','target_config')}
replay['import_hash']=manifest['import_hash']
(config/'mirror-invoke.json').write_text(json.dumps(replay,indent=2)+'\n')
PYREPLAY
# Only this explicitly owned synthetic source is detached. The target persists.
if [ -n "$SOURCE_CONTAINER" ] && docker network inspect "$NET" --format '{{range .Containers}}{{.Name}}{{"\n"}}{{end}}' | grep -Fxq "$SOURCE_CONTAINER"; then
  docker network disconnect "$NET" "$SOURCE_CONTAINER"
fi
docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add SETUID \
  --cap-add SETGID \
  --cap-add KILL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --add-host host.docker.internal:host-gateway \
  "${SOURCE_HOST_ARGS[@]}" \
  -e HOME=/tmp \
  --mount "type=volume,source=${GPT_PI_KIT_VOLUME:-oo-a406-eef831aa-pi-kit},target=/opt/pi-runtime-kit,readonly" \
  --mount "type=volume,source=${GPT_BINARY_VOLUME:-oo-a419-eef831aa-target},target=/target,readonly" \
  --mount "type=volume,source=${VOL},target=/var/lib/oo" \
  --mount "type=bind,source=${PRIV},target=/var/lib/oo/private,readonly" \
  --mount "type=bind,source=${CFG},target=/var/lib/oo/config,readonly" \
  --mount "type=bind,source=${HERE}/run-inside.sh,target=/run-inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=logistics-product-run \
  --label oo.kind=product-runner \
  "$IMAGE" \
  bash /run-inside.sh > "$EVIDENCE/container.id"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}}' > "$EVIDENCE/hostconfig.txt"
set +e
docker exec "$NAME" /opt/oo/bin/minimal-product-loop run --config /var/lib/oo/config/product.json > "$EVIDENCE/run.stdout" 2> "$EVIDENCE/run.stderr"
run_code=$?
set -e
printf '%s\n' "$run_code" > "$EVIDENCE/run.exit"
if [ "$run_code" -ne 0 ]; then exit "$run_code"; fi
printf '%s\n' 0 > "$EVIDENCE/launch.exit"
echo "product_draft_ready name=${NAME}"
