#!/bin/bash
# Post-edit Linux arm64 Rust 1.94 proof for answer-citations.
# Origin: evidence/.../a266/SP11/answer-citations/linux/smoke/launch.sh,
# which reused a254 smoke launch.sh blob 427e53f8d83ec981f05e2e512783209839a4b70d7cf852c17c2d911a23de4514
# (a204 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841,
# a200 c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541).
# Changes from that smoke launch: require smoke.exit 0, copy the existing
# archive to proof-src, overlay only tools/answer-citations, mount proof-src,
# container oo-a266-proof3, evidence linux/postedit-attempt-3. The archive is not rewritten.
# 6g, 1 CPU. No pull, prune, PostgreSQL, host cargo, or shared cache.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a266/SP11/answer-citations
EVIDENCE=${REVIEW}/linux/postedit-attempt-3
ROOT=/tmp/oo-a266-eef831aa-dba4-4af2-a78b-541bc9e0a266
FIXED=567d621e2e52b7e0011bdb025dc9337e92ecc5a6
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a266-proof3
NET=oo-a266-eef831aa-net
CARGO_VOL=oo-a266-eef831aa-cargo
TARGET_VOL=oo-a266-eef831aa-target
FIXTURE_SRC=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a242/SP12/benchmark-tools-review/linux/proof/live/desk-explicit.audit.jsonl

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite postedit console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases"
if [ "$(cat "$REVIEW/linux/smoke/smoke.exit" 2>/dev/null || true)" != "0" ]; then
  echo "pre-edit smoke did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -d "$ROOT/archive" ] || [ ! -f "$ROOT/archive/.fixed-base-sha" ]; then
  echo "fixed archive is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(cat "$ROOT/archive/.fixed-base-sha")" != "$FIXED" ]; then
  echo "fixed archive base mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/tools/answer-citations" ]; then
  echo "archive already contains answer-citations" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
cmp -s "$SRC/tools/answer-citations/tests/fixtures/desk-explicit.audit.jsonl" "$FIXTURE_SRC"
printf '%s\n' "$?" > "$EVIDENCE/fixture-cmp.exit"
if [ "$(cat "$EVIDENCE/fixture-cmp.exit")" != "0" ]; then
  echo "fixture bytes differ from the compiled query-pg receipt" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/proof-src"
mkdir -p "$ROOT/proof-src"
cp -a "$ROOT/archive/." "$ROOT/proof-src/"
mkdir -p "$ROOT/proof-src/tools"
rm -rf "$ROOT/proof-src/tools/answer-citations"
cp -a "$SRC/tools/answer-citations" "$ROOT/proof-src/tools/answer-citations"
if [ -e "$ROOT/archive/tools/answer-citations" ]; then
  echo "overlay mutated the fixed archive" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "archive_without_overlay" > "$EVIDENCE/archive-untouched.txt"
archive_files=$(find "$ROOT/archive" -type f ! -name '.fixed-base-sha' | wc -l | awk '{print $1}')
proof_files=$(find "$ROOT/proof-src" -type f ! -name '.fixed-base-sha' ! -path '*/tools/answer-citations/*' | wc -l | awk '{print $1}')
overlay_files=$(find "$ROOT/proof-src/tools/answer-citations" -type f | wc -l | awk '{print $1}')
printf 'archive=%s proof_without_overlay=%s overlay=%s\n' "$archive_files" "$proof_files" "$overlay_files" > "$EVIDENCE/overlay-count.txt"
if [ "$archive_files" != "$proof_files" ] || [ "$overlay_files" -lt 1 ]; then
  echo "proof-src copy mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a266 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a266 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a266 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a266 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a266 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a266 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a266 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/proof-src,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a266 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a266 \
  --label oo.task=answer-citations-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
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
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
if [ -e "$ROOT/archive/tools/answer-citations" ]; then
  echo "archive gained answer-citations during proof" >> "$EVIDENCE/archive-untouched.txt"
else
  echo "archive_still_without_overlay" >> "$EVIDENCE/archive-untouched.txt"
fi
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
