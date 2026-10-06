#!/bin/bash
# Independent review proof. Fresh archive of 9951cda plus one harness file.
# Origin: a266 postedit-attempt-4 launch c84dc04dbd711eff83af04e8740b1dca34e2873e8b60f351758d714e659b4a2a
# and a200 proof launch c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541.
# 6g, 1 CPU, private network, no PostgreSQL, no prune, no pull.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a273/SP11/answer-citations-review
EVIDENCE=${REVIEW}/linux/proof-attempt-2
ROOT=/tmp/oo-a273-eef831aa-dba4-4af2-a78b-541bc9e0a273
FIXED=9951cda7f128e30f1f6ce02e409d542d20a9e4fb
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a273-proof2
NET=oo-a273-eef831aa-net
CARGO_VOL=oo-a273-eef831aa-cargo
TARGET_VOL=oo-a273-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ "$(cat "$REVIEW/linux/smoke/smoke.exit" 2>/dev/null || true)" != "0" ]; then
  echo "smoke did not pass" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
rm -rf "$ROOT/proof-src"
mkdir -p "$ROOT/proof-src"
git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$ROOT/proof-src"
printf '%s\n' "$FIXED" > "$ROOT/proof-src/.fixed-base-sha"
if [ -e "$ROOT/proof-src/tools/answer-citations/tests/a273_review.rs" ]; then
  echo "archive already contains the review harness" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$REVIEW/overlay/a273_review.rs" "$ROOT/proof-src/tools/answer-citations/tests/a273_review.rs"
cp "$REVIEW/overlay/a273_review.rs" "$EVIDENCE/expect-review.rs"
cmp -s "$REVIEW/overlay/a273_review.rs" "$ROOT/proof-src/tools/answer-citations/tests/a273_review.rs"
printf '%s\n' "$?" > "$EVIDENCE/harness-cmp.exit"
if [ "$(cat "$EVIDENCE/harness-cmp.exit")" != "0" ]; then
  echo "harness copy mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -d "$ROOT/archive" ]; then
  diff -rq "$ROOT/archive" "$ROOT/proof-src" > "$EVIDENCE/archive-diff.txt" || true
  extra=$(grep -c 'Only in' "$EVIDENCE/archive-diff.txt" || true)
  changed=$(grep -c ' differ$' "$EVIDENCE/archive-diff.txt" || true)
  printf 'only_in=%s differ=%s\n' "$extra" "$changed" > "$EVIDENCE/overlay-count.txt"
  if [ "$extra" != "1" ] || [ "$changed" != "0" ]; then
    echo "proof tree is not the archive plus one harness file" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
  if ! grep -q 'a273_review.rs' "$EVIDENCE/archive-diff.txt"; then
    echo "the extra file is not the review harness" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
fi
want_files=$(git -C "$SRC" ls-tree -r --name-only "$FIXED" | wc -l | awk '{print $1}')
got_files=$(find "$ROOT/proof-src" -type f ! -name '.fixed-base-sha' | wc -l | awk '{print $1}')
harness_files=$((got_files - want_files))
printf 'listed=%s extracted_with_harness=%s extra=%s\n' "$want_files" "$got_files" "$harness_files" >> "$EVIDENCE/overlay-count.txt"
if [ "$harness_files" != "1" ]; then
  echo "harness file count is not 1" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' 0 > "$EVIDENCE/pg.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a273 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a273 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a273 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a273 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a273 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a273 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a273 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  --label oo.owner=a273 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a273 \
  --label oo.task=answer-citations-review-proof \
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
