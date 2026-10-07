#!/bin/bash
# One inventory rewrite after the child-verdict clarification. No tests. No PostgreSQL.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a298/SP04/cdc-ordinal-rereview/linux/manifest/launch.sh
# sha256 a9024348e971e21fdebe1601f76193cb575a3b5e0e47437eabf5549ee9881506
# Tool: tools/sha256_manifest.rs sha256 d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3
# Does not overwrite the first manifest console, hostconfig, or docker-run.exit.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a320/SP04/cdc-apply-pg-review
EVIDENCE=${REVIEW}/linux/manifest
OUT=/tmp/oo-a320-eef831aa-dba4-4af2-a78b-541bc9e0a320-refresh
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a320-manifest-refresh

chmod 755 "$EVIDENCE/refresh.sh"
if [ -f "$EVIDENCE/refresh-console.log" ]; then
  echo "refusing to overwrite refresh-console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/tools/sha256_manifest.rs" ] || [ ! -f "$EVIDENCE/MANIFEST-first.sha256" ]; then
  echo "owned hasher or first inventory copy is missing" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$OUT"
own=$(docker ps -q --filter label=oo.owner=a320 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'refresh own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/refresh-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/refresh.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named refresh container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/refresh.exit"
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
  --pids-limit 256 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a320 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a320 \
  --label oo.task=cdc-apply-pg-review-manifest-refresh \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc -vV; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$OUT/refresh-console.txt" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/refresh.exit"
cp "$OUT/refresh-console.txt" "$EVIDENCE/refresh-console.log"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/refresh-hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/refresh-removed.txt"
rm -rf "$OUT"
exit "$run_exit"
