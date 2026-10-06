#!/bin/bash
# Linux arm64 Rust 1.94 proof of the description-candidate CLI.
# Fixed archive fbc10b04 plus three owned files from source commit 827e768b.
# Origin launch: a253 commit 268e46972458fe53dd54412ab277b2cd09552a82
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/proof-7/launch.sh
#   blob c14c7146fe73f913d1e22efe8b605107ab56b07d
# which itself follows a204 commit b2b3adabd05de94723d8ed13b33be0144a0de1f9
#   blob 3737c1a8321814ff3b1070ddf8b89d6864c63f6f.
# Changes: owner a258, fixed fbc10b04, source 827e768b, CLI overlay only, 0 PostgreSQL.
# a200 commit a587031c719a484830d78e493605f48e2056cae4 is the PG recipe and is not applied.
# Does not overwrite an existing console.log.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a258-eef831aa-dba4-4af2-a78b-541bc9e0a258
FIXED=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a
SOURCE=827e768b9f94af6134a5734bd16e066ab0d47528
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a258-proof
NET=oo-a258-eef831aa-net
CARGO_VOL=oo-a258-eef831aa-cargo
TARGET_VOL=oo-a258-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/preoverlay/smoke.exit" ] || [ "$(cat "$REVIEW/linux/preoverlay/smoke.exit")" != "0" ]; then
  echo "preoverlay smoke did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" \
  "$EVIDENCE/overlay/crates/oo-builder/src/bin" \
  "$EVIDENCE/overlay/crates/oo-builder/tests" \
  "$EVIDENCE/overlay/contracts/builder" \
  "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
printf '%s\n' "$SOURCE" > "$EVIDENCE/source-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if [ ! -f "$ROOT/archive/crates/oo-builder/src/description_candidate.rs" ]; then
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$ROOT/archive"
  printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
fi
if [ -e "$ROOT/archive/crates/oo-builder/src/bin/description-candidate.rs" ]; then
  echo "fixed archive already contains the CLI" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
want_files=$(git -C "$SRC" ls-tree -r --name-only "$FIXED" | wc -l | awk '{print $1}')
got_files=$(find "$ROOT/archive" -type f ! -name '.fixed-base-sha' | wc -l | awk '{print $1}')
printf 'listed=%s extracted=%s\n' "$want_files" "$got_files" > "$EVIDENCE/archive-count.txt"
if [ "$want_files" != "$got_files" ]; then
  echo "archive file count mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
extract() {
  git -C "$SRC" cat-file blob "$SOURCE:$1" > "$2"
  want=$(git -C "$SRC" rev-parse "$SOURCE:$1")
  got=$(git -C "$SRC" hash-object "$2")
  printf '%s want=%s got=%s\n' "$1" "$want" "$got"
  if [ "$want" != "$got" ]; then
    echo "overlay blob mismatch $1" >&2
    return 1
  fi
}
{
  extract crates/oo-builder/src/bin/description-candidate.rs "$EVIDENCE/overlay/crates/oo-builder/src/bin/description-candidate.rs" &&
  extract crates/oo-builder/tests/description_candidate_cli.rs "$EVIDENCE/overlay/crates/oo-builder/tests/description_candidate_cli.rs" &&
  extract contracts/builder/description-candidate-cli-v1.md "$EVIDENCE/overlay/contracts/builder/description-candidate-cli-v1.md"
} > "$EVIDENCE/overlay-blobs.txt"
if [ "$?" -ne 0 ]; then
  echo "overlay extract failed" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a258 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a258 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a258 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a258 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a258 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a258 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a258 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a258 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a258 \
  --label oo.task=description-candidate-cli-proof \
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
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
