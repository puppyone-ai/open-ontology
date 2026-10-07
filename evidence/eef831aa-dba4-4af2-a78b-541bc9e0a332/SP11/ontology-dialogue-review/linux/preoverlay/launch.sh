#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of 63566b60. No pull. No PostgreSQL client.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review
PRE=${EVIDENCE}/linux/preoverlay
ROOT=/tmp/oo-a332-eef831aa-dba4-4af2-a78b-541bc9e0a332
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
OLD=96afe70fd68a56ae8eec7504bf20e09ecd3cdf65
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a332-preoverlay
NET=oo-a332-eef831aa-net
CARGO_VOL=oo-a332-eef831aa-cargo
TARGET_VOL=oo-a332-eef831aa-target
AUTHOR=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue

hash_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

fail() {
  printf '%s\n' "$1" > "$PRE/docker-run.exit"
  echo "$2" >&2
  exit "$1"
}

mkdir -p "$PRE/phases" "$ROOT"
printf '%s\n' "$$" > "$PRE/launch.pid"
chmod 755 "$PRE/inside.sh" "$PRE/launch.sh" "$EVIDENCE/linux/libclang-extract.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh"

if [ -f "$PRE/console.log" ]; then
  echo "refusing to overwrite preoverlay console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  fail 1 "pinned rust image is not present locally; not pulling"
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PRE/image.txt"
grep -q 'arch=arm64' "$PRE/image.txt" || fail 1 "rust image is not arm64"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  fail 2 "task network is missing"
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  fail 2 "named preoverlay container already exists"
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PRE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PRE/host-cargo-procs.txt" || true)
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$PRE/builders-before.txt" || true
builder_n=$(grep -c . "$PRE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
own=$(docker ps -q --filter status=running --filter label=oo.owner=a332 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'host_cargo=%s rust_builders=%s global=%s own=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" "$global_n" "$own" > "$PRE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_n" -ge 10 ]; then
  fail 3 "deferred: rust builder capacity is full"
fi

git -C "$SRC" rev-parse HEAD > "$PRE/repo-head.txt"
git -C "$SRC" rev-parse "${FIXED}^{commit}" > "$PRE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PRE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/ontology-dialogue" > "$PRE/dialogue-tree.txt"
grep -q '^1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0$' "$PRE/dialogue-tree.txt" || fail 2 "dialogue tree is not the frozen tree"
if git -C "$SRC" cat-file -e "${OLD}:tools/ontology-dialogue" 2>/dev/null; then
  fail 2 "old archive unexpectedly contains the dialogue tool"
fi
printf '%s\n' "absent" > "$PRE/old-dialogue.txt"

{
  echo "fixed=${FIXED}"
  echo "old_archive=${OLD}"
  echo "old_dialogue=absent"
  echo "dialogue_tree=1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0"
  echo "dialogue_commit=e31635b213911c725159a23dbccc8463cc45cdd7"
  for path in Cargo.lock Cargo.toml tools/ontology-dialogue tools/answer-citations tools/pg-onboarding tools/description-provider crates/oo-gateway crates/oo-pipeline crates/oo-connectors crates/oo-core crates/oo-builder crates/oo-jobs crates/oo-control; do
    old_id=$(git -C "$SRC" rev-parse "${OLD}:${path}" 2>/dev/null || echo MISSING)
    new_id=$(git -C "$SRC" rev-parse "${FIXED}:${path}" 2>/dev/null || echo MISSING)
    if [ "$old_id" = "$new_id" ]; then
      echo "SAME ${path} ${new_id}"
    else
      echo "DIFF ${path} old=${old_id} fixed=${new_id}"
    fi
  done
} > "$EVIDENCE/provenance.txt"

{
  echo "a204_launch=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "a204_launch_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh")"
  echo "a199_launch=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/launch.sh"
  echo "a199_launch_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/preoverlay/launch.sh")"
  echo "a204_sha256_manifest=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs"
  echo "a204_sha256_manifest_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/tools/sha256_manifest.rs")"
  echo "copy_sha256_manifest_sha256=$(hash_file "${EVIDENCE}/tools/sha256_manifest.rs")"
  echo "a200_extract=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh"
  echo "a200_extract_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh")"
  echo "a200_select_debs=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs"
  echo "a200_select_debs_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/select-debs.rs")"
  echo "copy_select_debs_sha256=$(hash_file "${EVIDENCE}/tools/select-debs.rs")"
  echo "a200_sha256=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs"
  echo "a200_sha256_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/tools/sha256.rs")"
  echo "copy_sha256_sha256=$(hash_file "${EVIDENCE}/tools/sha256.rs")"
  echo "a200_pg_wrap=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh"
  echo "a200_pg_wrap_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh")"
  echo "copy_pg_wrap_sha256=$(hash_file "${EVIDENCE}/pg/pg-wrap.sh")"
  echo "a200_pg_up=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  echo "a200_pg_up_sha256=$(hash_file "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh")"
  echo "a300_proof8_launch=${AUTHOR}/linux/proof-8/launch.sh"
  echo "a300_proof8_launch_sha256=$(hash_file "${AUTHOR}/linux/proof-8/launch.sh")"
  echo "a300_proof8_inside=${AUTHOR}/linux/proof-8/inside.sh"
  echo "a300_proof8_inside_sha256=$(hash_file "${AUTHOR}/linux/proof-8/inside.sh")"
  echo "changes=owner a332; archive is 63566b60 with the committed tool and no overlay; rust-toolchain.toml omitted; memory 6g; cpus 1; pids 4096; no python package selector; no noexec tmpfs; env names stay OO_A300_* because the frozen test reads them; values point at a332"
} > "$EVIDENCE/recipe-origin.txt"

rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock crates contracts \
  tools/ontology-dialogue tools/answer-citations tools/pg-onboarding tools/description-provider \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/tools/ontology-dialogue/src/lib.rs" ]; then
  fail 2 "archive missing dialogue"
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  fail 2 "archive missing query-pg"
fi
if [ ! -f "$ROOT/archive/contracts/sdk/json-query/fixtures/tls/ca.pem" ]; then
  fail 2 "archive missing the fixture CA"
fi
if [ -f "$ROOT/archive/rust-toolchain.toml" ] || [ -d "$ROOT/archive/evidence" ]; then
  fail 2 "archive contains a toolchain file or the evidence tree"
fi
printf '%s\n' "$FIXED" > "$ROOT/archive-stamp.txt"

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a332 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a332 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" > "$PRE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a332 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a332 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" > "$PRE/volume-target.txt"

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$PRE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PRE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/review,readonly" \
  --mount "type=bind,source=${AUTHOR}/source.sha256,target=/review-author/source.sha256,readonly" \
  --mount "type=bind,source=${PRE},target=/evidence/linux/preoverlay" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a332 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a332 \
  --label oo.task=ontology-dialogue-review-preoverlay \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PRE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PRE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PRE/hostconfig.txt"
docker rm "$NAME" > "$PRE/container-removed.txt" 2>&1 || true
exit "$run_exit"
