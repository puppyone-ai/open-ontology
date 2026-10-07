#!/bin/bash
# Linux arm64 Rust 1.94 review of consumer archive a13b1cc.
# Recipe origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a348/SP07/description-field-review/linux/proof-4/launch.sh
# content sha256 ae0ac0b4da595f94eae9f7838b212f08c5431db76073824a07d40a373ca23638
# and evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/focused-10/launch.sh
# content sha256 2fe3844e65566eb8508790482310768614d1a96ddff467a55bcd993736ed5c40
# Change: archive is a13b1cc, not the work tree. pids-limit 256. No lock is copied
# back into the repository. No PostgreSQL server. Receipts are read-only.
# The bearer file is not mounted.
set -u
set -e
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a357/SP07/pg-builder-review
EVIDENCE=${REVIEW}/linux/proof-4
ROOT=/tmp/oo-a357-eef831aa-dba4-4af2-a78b-541bc9e0a357/proof-4
RECEIPTS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
FIXED=a13b1cc80ed66c07199a41d47b31fd42530f2be9
PRODUCER=c23db28dc27f5cf20741dd36e8a8e88129ad1715
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a357-proof4
NET=oo-a357-eef831aa-net
CARGO_VOL=oo-a357-eef831aa-cargo
TARGET_VOL=oo-a357-eef831aa-target
LOCK_SRC=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a292/SP07/description-pack/tools/lock_subset.rs
LOCK_SHA=dfa03497423b1b35a427aadf6df175fd58d7355ad5c8dab7bb3364c2d31f5454
MANIFEST_SRC=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs
MANIFEST_SHA=d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -d "$RECEIPTS/live/ready" ]; then
  echo "a328 receipts are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive" "$REVIEW/tools" "$EVIDENCE/phases"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/description-pack \
  tools/pg-builder testing/real-db/pg-builder \
  | tar -x -C "$ROOT/archive"
find "$ROOT/archive" -name '._*' -delete
find "$ROOT/archive" -name '.DS_Store' -delete
if [ ! -f "$ROOT/archive/tools/pg-builder/src/lib.rs" ]; then
  echo "archive missing pg-builder" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/tools/pg-builder/tests/a357_consumer.rs" ]; then
  echo "archive already contains the oracle" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
: > "$EVIDENCE/archive-blobs.txt"
# Producer files must be the c23db28 blobs. Consumer pg-builder must be a13b1cc.
# rebind.rs is absent at c23db28. That absence is not a launch failure.
record_blob() {
  local class="$1"
  local rel="$2"
  local want got producer
  want=$(git -C "$SRC" rev-parse "$FIXED:$rel")
  got=$(git -C "$SRC" hash-object "$ROOT/archive/$rel")
  if git -C "$SRC" cat-file -e "$PRODUCER:$rel" 2>/dev/null; then
    producer=$(git -C "$SRC" rev-parse "$PRODUCER:$rel")
  else
    producer=absent
  fi
  printf '%s class=%s fixed=%s archive=%s producer=%s\n' "$rel" "$class" "$want" "$got" "$producer" >> "$EVIDENCE/archive-blobs.txt"
  if [ "$want" != "$got" ]; then
    echo "archive is not $FIXED for $rel" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
  if [ "$class" = producer ]; then
    if [ "$producer" = absent ] || [ "$producer" != "$got" ]; then
      echo "producer file is not $PRODUCER for $rel" >&2
      printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
      exit 2
    fi
  fi
}
for rel in \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/role_pack.rs \
  crates/oo-builder/src/reviewed_effective.rs \
  tools/description-review/src/lib.rs \
  tools/description-review/src/apply.rs \
  tools/description-pack/src/lib.rs \
  tools/description-pack/src/apply.rs \
  Cargo.lock
do
  record_blob producer "$rel"
done
for rel in \
  tools/pg-builder/Cargo.toml \
  tools/pg-builder/Cargo.lock \
  tools/pg-builder/CONTRACT.md \
  tools/pg-builder/src/lib.rs \
  tools/pg-builder/src/bridge.rs \
  tools/pg-builder/src/rebind.rs \
  tools/pg-builder/src/generate.rs \
  tools/pg-builder/tests/contract.rs \
  testing/real-db/pg-builder/Cargo.toml \
  testing/real-db/pg-builder/Cargo.lock
do
  record_blob consumer "$rel"
done
mkdir -p "$ROOT/overlay"
cp "$REVIEW/overlay/a357_consumer.rs" "$ROOT/overlay/a357_consumer.rs"
shasum -a 256 "$ROOT/overlay/a357_consumer.rs" > "$EVIDENCE/oracle.sha256"
cp "$LOCK_SRC" "$EVIDENCE/lock_subset.rs"
cp "$LOCK_SRC" "$REVIEW/tools/lock_subset.rs"
lock_got=$(shasum -a 256 "$EVIDENCE/lock_subset.rs" | awk '{print $1}')
printf 'want=%s got=%s\n' "$LOCK_SHA" "$lock_got" > "$EVIDENCE/lock-subset-tool.sha256"
if [ "$lock_got" != "$LOCK_SHA" ]; then
  echo "lock_subset.rs hash drifted" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$MANIFEST_SRC" "$REVIEW/tools/sha256_manifest.rs"
manifest_got=$(shasum -a 256 "$REVIEW/tools/sha256_manifest.rs" | awk '{print $1}')
printf 'want=%s got=%s\n' "$MANIFEST_SHA" "$manifest_got" > "$REVIEW/tools/sha256_manifest.sha256"
if [ "$manifest_got" != "$MANIFEST_SHA" ]; then
  echo "sha256_manifest.rs hash drifted" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
printf '%s\n' "$PRODUCER" > "$EVIDENCE/producer-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:Cargo.lock" > "$EVIDENCE/root-lock-blob.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || {
  echo "pinned image is not arm64" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
visible=0
attempt=0
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if docker run --rm --pull never --network none --entrypoint bash \
    --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
    "$IMAGE" -c 'test -f /src/tools/pg-builder/src/lib.rs && test -f /src/tools/pg-builder/Cargo.lock' >/dev/null 2>&1; then
    visible=1
    break
  fi
  sleep 1
done
printf 'visible=%s attempts=%s image=pinned-rust probe=archive-only postgres=not-started\n' "$visible" "$attempt" > "$EVIDENCE/archive-visible.txt"
if [ "$visible" != 1 ]; then
  echo "archive is not visible to docker" >&2
  printf '%s\n' 125 > "$EVIDENCE/docker-run.exit"
  exit 125
fi
own=$(docker ps -q --filter label=oo.owner=a357 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1 postgres=0\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a357 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a357 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a357 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a357 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a357 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a357 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 --pids-limit 256 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/pg-builder \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e A357_RECEIPTS=/receipts \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${RECEIPTS},target=/receipts,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a357 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a357 \
  --label oo.task=pg-builder-review-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$EVIDENCE/hostconfig.txt"
  docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
  docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
fi
exit "$run_exit"
