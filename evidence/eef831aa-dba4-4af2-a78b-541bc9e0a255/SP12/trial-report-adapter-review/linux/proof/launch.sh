#!/bin/bash
# Independent proof on a readonly archive of fixed commit 8e7925d7.
# The review harness is mounted at /review. It is not copied into the product tree.
# Does not overwrite an existing console.log. Does not prune.
# Origin: a204 preoverlay launch b2b3adabd05de94723d8ed13b33be0144a0de1f9
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
# sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
# a200 is not copied: no PostgreSQL and no private TLS.
# Delta: owner a255, fixed 8e7925d7, 6 GiB, one CPU, adapter already in the archive, harness mount only.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a255/SP12/trial-report-adapter-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a255-eef831aa-dba4-4af2-a78b-541bc9e0a255/proof
FIXED=8e7925d7f4576cb6a9f4027eca66ab1671e3fd1a
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a255-proof
NET=oo-a255-eef831aa-net
CARGO_VOL=oo-a255-eef831aa-cargo
TARGET_VOL=oo-a255-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/archive" "$ROOT/fresh"
mkdir -p "$ROOT/archive" "$ROOT/fresh" "$REVIEW/source/blobs"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/sql-runner benchmarks/trial-statistics benchmarks/trial-report-adapter \
  | tar -x -C "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" benchmarks/trial-report-adapter \
  | tar -x -C "$REVIEW/source/blobs"
if ! diff -rq "$REVIEW/source/blobs/benchmarks/trial-report-adapter" "$ROOT/archive/benchmarks/trial-report-adapter" > "$EVIDENCE/blob-diff.txt"; then
  echo "saved blobs differ from the archive" >&2
  exit 2
fi
if grep -R -q 'A255-PASS' "$ROOT/archive/benchmarks/trial-report-adapter"; then
  echo "review harness marker is inside the sealed adapter" >&2
  exit 2
fi
git -C "$SRC" rev-parse "$FIXED" > "$REVIEW/source/commit.txt"
git -C "$SRC" rev-parse "${FIXED}^" > "$REVIEW/source/parent.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$REVIEW/source/tree.txt"
git -C "$SRC" log -1 --format='%s%n%an%n%ad' "$FIXED" > "$REVIEW/source/subject.txt"
git -C "$SRC" rev-parse HEAD > "$REVIEW/source/repo-head-at-extract.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
(
  cd "$ROOT/archive/benchmarks/trial-report-adapter"
  find . -type f -print | sort | while IFS= read -r file; do
    shasum -a 256 "$file"
  done
) > "$EVIDENCE/source-sha256.txt"
shasum -a 256 "$ROOT/archive/rust-toolchain.toml" > "$EVIDENCE/toolchain-file.sha256"
mv "$ROOT/archive/rust-toolchain.toml" "$ROOT/archive/rust-toolchain.toml.repo"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a255 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a255 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a255 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a255 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a255 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a255 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a255 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  --pids-limit 2048 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/review,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a255 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a255 \
  --label oo.task=trial-report-adapter-review-proof \
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
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/sql-runner benchmarks/trial-statistics benchmarks/trial-report-adapter \
  | tar -x -C "$ROOT/fresh"
mv "$ROOT/fresh/rust-toolchain.toml" "$ROOT/fresh/rust-toolchain.toml.repo"
diff -rq "$ROOT/fresh" "$ROOT/archive" > "$EVIDENCE/tree-diff.txt" || true
exit "$run_exit"
