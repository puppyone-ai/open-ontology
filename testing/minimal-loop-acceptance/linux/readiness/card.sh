#!/bin/bash
# Readiness card. It does not execute the producer and does not reach the source database.
# Network is none. The kit volume is mounted read-only. Secrets are not mounted.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a407
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP11/minimal-loop-acceptance
OUT=${ROOT}/readiness
PROBE=${ROOT}/fixture/load/probe.txt
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a407-readiness
TARGET_VOL=oo-a407-eef831aa-target
KIT_VOL=oo-a406-eef831aa-pi-kit
PIN=7c10bd4337495ee613f2224843ecdf349b80d1df
PI_SHA=e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774
NODE_SHA=d09e299258c24f7cdf6f5d5ec185e3a56512b27a697113735dac909f1cac7b8d
mkdir -p "$OUT/archive" "$OUT/out"
if [ ! -f "$PROBE" ]; then
  echo "fixture probe is absent" >&2
  printf '%s\n' 1 > "$OUT/card.exit"
  exit 1
fi
cp "$PROBE" "$OUT/probe.txt"
HEAD=$(git -C "$REPO" rev-parse HEAD)
printf '%s\n' "$HEAD" > "$OUT/archive/COMMIT"
set +e
git -C "$REPO" rev-parse "HEAD:tools/agent-entry" > "$OUT/agent-entry.stdout" 2> "$OUT/agent-entry.stderr"
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
    "host": "oo-a407-src-pg",
    "port": 5432,
    "name": "oo_a407_src",
    "username": "oo_a407_reader",
    "tls": "verify-full",
    "password_file": "/tmp/oo-a407-${RUN_ID}/secrets/src/reader.pass",
    "secret_ref": "secret://pg/a407/source-reader",
    "ca_file": "/tmp/oo-a407-${RUN_ID}/secrets/src/ca.crt"
  },
  "local_copy": {
    "host": "oo-a407-dst-pg",
    "port": 5432,
    "name": "oo_a407_dst",
    "username": "oo_a407_reader",
    "password_file": "/tmp/oo-a407-${RUN_ID}/secrets/dst/reader.pass",
    "secret_ref": "secret://pg/a407/target-reader",
    "ca_file": "/tmp/oo-a407-${RUN_ID}/secrets/dst/ca.crt",
    "source_password_file": ""
  },
  "llm": {
    "model_id": "grok-4.7",
    "bearer_file": "/tmp/oo-customer-demo-20261007/llm-token"
  },
  "pi": {
    "commit": "${PIN}",
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
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "readiness container already exists" >&2
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
  --label oo.task=minimal-loop-acceptance-readiness \
  --label oo.kind=acceptance-runner \
  "$IMAGE" \
  /target/debug/minimal-loop-acceptance card \
  --repo /src \
  --archive /evidence/archive \
  --out /evidence/out \
  --pinned-commit "$HEAD" \
  --config /evidence/config.json \
  --kit-root /opt/pi-runtime-kit \
  --pi-commit "$PIN" \
  --pi-executable /opt/pi-runtime-kit/install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js \
  --pi-runtime /opt/pi-runtime-kit/node/bin/node \
  --sealed-manifest /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a408/SP03/local-mirror-single-source/sealed-manifest.json \
  --copy-review /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/verdict.json \
  --product-note /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a405/SP11/minimal-product-loop/kit-consumed.txt \
  --fixture-probe /evidence/probe.txt \
  > "$OUT/card.stdout" 2> "$OUT/card.stderr"
card_exit=$?
set -e
printf '%s\n' "$card_exit" > "$OUT/card.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' > "$OUT/hostconfig.txt"
docker rm "$NAME" > "$OUT/container-removed.txt"
exit "$card_exit"
