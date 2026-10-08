#!/bin/bash
# Private Git proof for the a381 curated-git-apply review.
# Archive of the fixed commit, then the review oracle copied on top.
# Adapted from the a204 launch. Does not prune. Does not pull.
set -eu
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a381/SP07/curated-git-apply-review
EVIDENCE=${REVIEW}/linux/proof-2
ORACLE=${REVIEW}/oracle/a381_review.rs
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6
NAME=oo-a381-proof2
NET=oo-a381-eef831aa-net
VOL_CARGO=oo-a381-eef831aa-cargo
VOL_TARGET=oo-a381-eef831aa-target
ARCHIVE=/tmp/a381-eef831aa-archive-2
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a381 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
git -C "$SRC" cat-file -e "${FIXED}^{commit}"
if git -C "$SRC" cat-file -e "${FIXED}:tools/curated-git-apply/tests/a381_review.rs" 2>/dev/null; then
  echo "fixed commit already contains the review oracle" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  echo "fixed=${FIXED}"
  echo "product_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "contract_blob=$(git -C "$SRC" rev-parse "${FIXED}:tools/curated-git-apply/CONTRACT.md")"
  echo "contract_9d33=$(git -C "$SRC" rev-parse 9d33bfc065901db78e8f736e3a840b57630a31e4:tools/curated-git-apply/CONTRACT.md)"
  echo "pinned_commit=$(git -C "$SRC" show "${FIXED}:tools/curated-git-apply/PINNED" | awk 'NR==1 { print $2 }')"
  echo "oo_core=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-core")"
  echo "oo_builder=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-builder")"
  echo "oo_jobs=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-jobs")"
  echo "description_review=$(git -C "$SRC" rev-parse "${FIXED}:tools/description-review")"
  echo "description_pack=$(git -C "$SRC" rev-parse "${FIXED}:tools/description-pack")"
  echo "tool_tree=$(git -C "$SRC" rev-parse "${FIXED}:tools/curated-git-apply")"
} > "$EVIDENCE/pins.txt"
grep -q 'product_tree=4b5b6477846aa4c2883898bddfb723f1b8218f57' "$EVIDENCE/pins.txt"
grep -q 'contract_blob=fd8162d95d717fbc65a9fc12874d3f0d92081169' "$EVIDENCE/pins.txt"
grep -q 'contract_9d33=fd8162d95d717fbc65a9fc12874d3f0d92081169' "$EVIDENCE/pins.txt"
grep -q 'oo_core=769dd0e8f7ee77a29017fb3e0f9d90917b0a8194' "$EVIDENCE/pins.txt"
grep -q 'oo_builder=40f74d7e9aac9c13e95d1da3d61d689184773c77' "$EVIDENCE/pins.txt"
grep -q 'oo_jobs=ca228d7c648e554675cdf31ae828320e9d05cd9e' "$EVIDENCE/pins.txt"
grep -q 'description_review=a60b6d82e5b114430a66d82f91de4d3a7f3647c4' "$EVIDENCE/pins.txt"
grep -q 'description_pack=9d99fe90ecf87c09b4bb33e46e89bd078c10586b' "$EVIDENCE/pins.txt"
grep -q 'pinned_commit=be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba' "$EVIDENCE/pins.txt"
python3 - "$ORACLE" "$EVIDENCE/oracle-expected.sha256" <<'PY'
import hashlib, pathlib, sys
path, dest = sys.argv[1:]
digest = hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
pathlib.Path(dest).write_text(f"{digest}  /src/tools/curated-git-apply/tests/a381_review.rs\n")
PY
rm -rf "$ARCHIVE"
mkdir -p "$ARCHIVE"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/curated-git-apply \
  | tar -x -C "$ARCHIVE"
test -f "$ARCHIVE/Cargo.toml"
test -f "$ARCHIVE/crates/oo-core/Cargo.toml"
test -f "$ARCHIVE/crates/oo-builder/Cargo.toml"
test -f "$ARCHIVE/crates/oo-jobs/Cargo.toml"
test -f "$ARCHIVE/crates/oo-connectors/vendor/sqlx-core/Cargo.toml"
test -f "$ARCHIVE/tools/description-review/Cargo.toml"
test -f "$ARCHIVE/tools/curated-git-apply/Cargo.lock"
if [ -e "$ARCHIVE/tools/curated-git-apply/tests/a381_review.rs" ]; then
  echo "extracted archive already has the review oracle" >&2
  rm -rf "$ARCHIVE"
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$ORACLE" "$ARCHIVE/tools/curated-git-apply/tests/a381_review.rs"
test ! -e "$ARCHIVE/.git"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create --driver bridge "$NET" > "$EVIDENCE/network-created.txt"
else
  printf '%s\n' "$NET" > "$EVIDENCE/network-created.txt"
fi
if ! docker volume inspect "$VOL_CARGO" >/dev/null 2>&1; then
  docker volume create "$VOL_CARGO" > "$EVIDENCE/volume-cargo.txt"
else
  printf '%s\n' "$VOL_CARGO" > "$EVIDENCE/volume-cargo.txt"
fi
if ! docker volume inspect "$VOL_TARGET" >/dev/null 2>&1; then
  docker volume create "$VOL_TARGET" > "$EVIDENCE/volume-target.txt"
else
  printf '%s\n' "$VOL_TARGET" > "$EVIDENCE/volume-target.txt"
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e TMPDIR=/target/tmp \
  -e CARGO_BUILD_JOBS=1 \
  -e MAKEFLAGS=-j1 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ARCHIVE},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo-home" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a381 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a381 \
  --label oo.task=curated-git-apply-review-proof \
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
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
rm -rf "$ARCHIVE"
exit "$run_exit"
