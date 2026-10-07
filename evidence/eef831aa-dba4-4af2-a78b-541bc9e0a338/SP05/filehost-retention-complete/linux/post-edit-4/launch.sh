#!/bin/bash
# Post-edit proof. Fixed e9cba7df archive plus the owned completion overlay.
# Same private net and volumes as the pre-edit smoke. One 6GiB/1CPU builder.
# No pull, prune, PostgreSQL, host network, or copy-back onto the git tree.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete/linux/post-edit-4
ROOT=/tmp/oo-a338-eef831aa-dba4-4af2-a78b-541bc9e0a338
FIXED=e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a338-postedit4
NET=oo-a338-eef831aa-net
CARGO_VOL=oo-a338-eef831aa-cargo
TARGET_VOL=oo-a338-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a338
BASE=${ROOT}/fixed-archive
SNAP=${ROOT}/postedit4-snap

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite pre-edit console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

{
  echo "a338 post-edit attempt 4"
echo "attempt 3 evidence linux/post-edit-3 exit 101 repeated reap after same-config begin"
echo "change a later session may reuse the idempotency slot; that body is not the deleted object"
echo "attempt 2 evidence linux/post-edit-2 exit 101 clippy collapsible_if"
echo "change collapse the open-absence session comparison"
echo "attempt 1 evidence linux/post-edit exit 101"
echo "change create the legacy tombstone parent before the plant; one mismatched quota body after verify"
  echo "fixed ${FIXED}"
  echo "addendum 9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81"
  echo "product c27525a2a7c31dfbe9cb64b183312c7dab354511"
  echo "contract e6ebf93b5ac261c097a24d145611e0300e4f88b5"
  echo "origin evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete/linux/pre-edit/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete/linux/pre-edit/launch.sh"
  echo "origin inside"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete/linux/pre-edit/inside.sh"
  echo "change name preedit->postedit; overlay owned file_hosting.rs and artifact-retention sources and contracts; drop unchanged gates for those files; run the full oo-control lib tests"
  echo "a336 preoverlay launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review/linux/preoverlay/launch.sh"
  echo "a323 proof-3 launch"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete/linux/proof-3/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a323/SP05/filehost-retention-delete/linux/proof-3/launch.sh"
  echo "a204 launch"
  echo "commit b2b3adabd05de94723d8ed13b33be0144a0de1f9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "a200 launch"
  echo "commit a587031c719a484830d78e493605f48e2056cae4"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "a199 launch"
  echo "commit 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh"
  echo "a311 lock oracle"
  echo "path evidence/eef831aa-dba4-4af2-a78b-541bc9e0a311/SP05/artifact-retention/linux/proof-6/lock_from_baseline.rs"
  shasum -a 256 "$EVIDENCE/lock_from_baseline.rs"
  echo "change copied byte-for-byte from the a338 pre-edit copy; no edit"
} > "$EVIDENCE/recipe-origin.txt"

if [ ! -f "$BASE/Cargo.lock" ]; then
  rm -rf "$BASE"
  mkdir -p "$BASE"
  git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$BASE"
  if [ -f "$BASE/rust-toolchain.toml" ]; then
    mv "$BASE/rust-toolchain.toml" "$EVIDENCE/rust-toolchain.toml.aside"
  fi
fi
if [ ! -f "$BASE/Cargo.lock" ] || [ -f "$BASE/rust-toolchain.toml" ]; then
  echo "fixed archive is missing or still has rust-toolchain.toml" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$SNAP"
mkdir -p "$SNAP"
cp -a "$BASE/." "$SNAP/"
overlay() {
  local rel="$1"
  if [ ! -f "$SRC/$rel" ]; then
    echo "missing overlay $rel" >&2
    exit 2
  fi
  mkdir -p "$SNAP/$(dirname "$rel")"
  cp "$SRC/$rel" "$SNAP/$rel"
}
overlay crates/oo-control/src/file_hosting.rs
overlay tools/artifact-retention/src/lib.rs
overlay tools/artifact-retention/src/main.rs
overlay tools/artifact-retention/src/reaper.rs
overlay tools/artifact-retention/tests/live.rs
overlay tools/artifact-retention/CONTRACT.md
overlay tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md
overlay tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md
if ! cmp -s "$BASE/Cargo.lock" "$SNAP/Cargo.lock"; then
  echo "overlay changed the root lock" >&2
  exit 2
fi
if ! cmp -s "$BASE/tools/artifact-retention/Cargo.lock" "$SNAP/tools/artifact-retention/Cargo.lock"; then
  echo "overlay changed the child lock" >&2
  exit 2
fi
if ! cmp -s "$BASE/crates/oo-control/src/lib.rs" "$SNAP/crates/oo-control/src/lib.rs"; then
  echo "overlay changed lib.rs" >&2
  exit 2
fi
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" merge-base --is-ancestor c27525a2a7c31dfbe9cb64b183312c7dab354511 "$FIXED"
git -C "$SRC" merge-base --is-ancestor e6ebf93b5ac261c097a24d145611e0300e4f88b5 "$FIXED"
git -C "$SRC" merge-base --is-ancestor 9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81 HEAD
echo "product_ancestor=c27525a2" >> "$EVIDENCE/fixed-commit.txt"
echo "contract_ancestor=e6ebf93b" >> "$EVIDENCE/fixed-commit.txt"
echo "addendum_ancestor=9cdbc625" >> "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
(
  cd "$SNAP"
  shasum -a 256 \
    Cargo.lock \
    crates/oo-control/src/lib.rs \
    crates/oo-control/Cargo.toml \
    crates/oo-control/src/file_hosting.rs \
    tools/artifact-retention/Cargo.lock \
    tools/artifact-retention/Cargo.toml \
    tools/artifact-retention/src/lib.rs \
    tools/artifact-retention/src/main.rs \
    tools/artifact-retention/src/reaper.rs \
    tools/artifact-retention/tests/live.rs \
    tools/artifact-retention/CONTRACT.md \
    tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md \
    tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md \
    > "$EVIDENCE/archive-sha256.txt"
)

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create --label oo.owner=a338 --label oo.session="$SESSION" "$NET" > "$EVIDENCE/network-create.txt"
fi
docker network inspect --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.owner=a338 --label oo.session="$SESSION" "$CARGO_VOL" > "$EVIDENCE/volume-cargo-create.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create --label oo.owner=a338 --label oo.session="$SESSION" "$TARGET_VOL" > "$EVIDENCE/volume-target-create.txt"
fi
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

own=$(docker ps -q --filter label=oo.owner=a338 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named pre-edit container already exists" >&2
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
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a338 \
  --label oo.session="$SESSION" \
  --label oo.task=filehost-retention-complete-postedit4 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>/dev/null || true
exit "$run_exit"
