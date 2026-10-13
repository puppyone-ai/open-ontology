#!/bin/bash
# Bind the sealed a410 import into the acceptance card. Does not execute the producer.
# Network is none. Secrets are not mounted. The kit volume is read-only.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a407
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP11/minimal-loop-acceptance
OUT=${ROOT}/bind/card
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a407-bind-card
TARGET_VOL=oo-a407-eef831aa-target
KIT_VOL=oo-a406-eef831aa-pi-kit
PIN=5df69049267bf16eca36d311a1b3082f27aa9754
PI=7c10bd4337495ee613f2224843ecdf349b80d1df
PI_SHA=e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774
NODE_SHA=d09e299258c24f7cdf6f5d5ec185e3a56512b27a697113735dac909f1cac7b8d
mkdir -p "$OUT/archive" "$OUT/out"
if [ -f "$OUT/card.stdout" ]; then
  echo "refusing to overwrite bind card" >&2
  exit 2
fi
printf '%s\n' "$PIN" > "$OUT/archive/COMMIT"
set +e
git -C "$REPO" rev-parse "${PIN}:tools/agent-entry" > "$OUT/agent-entry.stdout" 2> "$OUT/agent-entry.stderr"
agent_exit=$?
set -e
printf '%s\n' "$agent_exit" > "$OUT/agent-entry.exit"
cat > "$OUT/config.json" <<EOF
{
  "protocol": "oo.minimal-loop-acceptance.readiness/v1",
  "mode": "operator-development",
  "development_preview": true,
  "publication": "not_published",
  "llm_review": "unreviewed",
  "runtime_acknowledgement": "local-process-not-a-sandbox-certificate",
  "database": {
    "type": "postgresql",
    "host": "oo-a410-tgt",
    "port": 5432,
    "name": "mirrordb",
    "username": "oo_mirror_read",
    "tls": "verify-full",
    "password_file": "/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/reader.pass",
    "secret_ref": "secret://pg/a410/mirror-reader",
    "ca_file": "/tmp/oo-a410-eef831aa-dba4-4af2-a78b-541bc9e0a410/secrets/ca.crt"
  },
  "local_copy": {
    "host": "oo-a410-tgt",
    "port": 5432,
    "name": "mirrordb",
    "import_hash": "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73",
    "snapshot_id": "00000004-00000013-1",
    "generation": 1,
    "source_password_file": ""
  },
  "llm": {
    "model_id": "grok-4.7",
    "bearer_file": "/tmp/oo-customer-demo-20261007/llm-token"
  },
  "pi": {
    "commit": "${PI}",
    "executable": "/opt/pi-runtime-kit/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js",
    "executable_sha256": "${PI_SHA}",
    "runtime": "/opt/pi-runtime-kit/node/bin/node",
    "runtime_sha256": "${NODE_SHA}",
    "provider": "grok",
    "model": "grok-4.7"
  }
}
EOF
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$OUT/card.exit"
  exit 1
fi
if ! docker volume inspect "$KIT_VOL" >/dev/null 2>&1; then
  echo "a406 kit volume is absent; not creating it" >&2
  printf '%s\n' 1 > "$OUT/card.exit"
  exit 1
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "bind card container already exists" >&2
  printf '%s\n' 2 > "$OUT/card.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 1g --cpus 1 image=pinned-rust-1.94 /target/debug/minimal-loop-acceptance card" > "$OUT/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 128 \
  --network none \
  -e HOME=/tmp \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${OUT},target=/evidence" \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --mount "type=volume,source=${KIT_VOL},target=/opt/pi-runtime-kit,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=minimal-loop-acceptance-bind \
  --label oo.kind=acceptance-runner \
  "$IMAGE" \
  /target/debug/minimal-loop-acceptance card \
  --repo /src \
  --archive /evidence/archive \
  --out /evidence/out \
  --pinned-commit "$PIN" \
  --config /evidence/config.json \
  --kit-root /opt/pi-runtime-kit \
  --pi-commit "$PI" \
  --pi-executable /opt/pi-runtime-kit/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js \
  --pi-runtime /opt/pi-runtime-kit/node/bin/node \
  --sealed-manifest /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a410/SP03/local-mirror-single-source/linux/proof/cases/bulk.target.json \
  --import-manifest /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a410/SP03/local-mirror-single-source/linux/proof/cases/bulk.manifest.json \
  --copy-review /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/verdict.json \
  --product-note /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a405/SP11/minimal-product-loop/kit-consumed.txt \
  --binding-probe /src/evidence/${RUN_ID}/SP11/minimal-loop-acceptance/bind/probe.txt \
  > "$OUT/card.stdout" 2> "$OUT/card.stderr"
card_exit=$?
set -e
printf '%s\n' "$card_exit" > "$OUT/card.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' > "$OUT/hostconfig.txt"
docker rm "$NAME" > "$OUT/container-removed.txt"
exit "$card_exit"
