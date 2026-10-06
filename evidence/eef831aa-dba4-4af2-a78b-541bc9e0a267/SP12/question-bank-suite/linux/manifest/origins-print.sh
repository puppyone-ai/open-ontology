#!/bin/bash
# Write recipe-origin.sha256. The first origins container left the digest
# file inside its tmpfs, so its console has only the self-check summary.
# This container keeps that first console and writes the digest file here.
# Does not prune.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: origins-print.sh <console-outside-evidence-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a267-manifest-origins-print
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the evidence directory" >&2
    exit 2
    ;;
esac
if [ -f "$EVIDENCE/origins-print-console.log" ] || [ -f "$EVIDENCE/recipe-origin.sha256" ]; then
  echo "refusing to overwrite origins print output" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/origins-console.log" ]; then
  echo "origins-console.log is missing" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a267 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/origins-print-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/origins-print-docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named origins-print container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/origins-print-docker-run.exit"
  exit 2
fi
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 2048 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/repo,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a267 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a267 \
  --label oo.task=question-bank-suite-manifest-origins-print \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
set -u
rustc --edition 2021 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
printf "rustc_exit=%s\n" "$?"
mkdir -p /tmp/origins
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh /tmp/origins/a204-preoverlay-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh /tmp/origins/a204-preoverlay-inside.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh /tmp/origins/a200-smoke-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs /tmp/origins/a199-sha256_manifest.rs
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/launch.sh /tmp/origins/a256-impl5-launch.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/inside.sh /tmp/origins/a256-impl5-inside.sh
cp /repo/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/tools/sha256_manifest.rs /tmp/origins/a267-sha256_manifest.rs
/usr/local/bin/sha256_manifest /tmp/origins /evidence/linux/manifest/recipe-origin.sha256
printf "manifest_exit=%s\n" "$?"
' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/origins-print-docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/origins-print-hostconfig.txt"
cp "$CONSOLE" "$EVIDENCE/origins-print-console.log"
docker rm "$NAME" > "$EVIDENCE/origins-print-container-removed.txt"
exit "$run_exit"
