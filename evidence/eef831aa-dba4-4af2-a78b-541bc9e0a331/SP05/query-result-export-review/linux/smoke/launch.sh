#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of fixed 15558917.
# Origin: a325 smoke launch.sh
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a325/SP05/query-result-export/linux/smoke/launch.sh
#   sha256 9bbb9e8505cb71a1db8efe295df41affa21c8cd13a7462c8c1d096e0a6e0a2ed
# which reused a266 e42b1f05ed939b28f5271090ecf8b39dc810979c54c17a7a92b1f03bc473061a,
# a254 427e53f8d83ec981f05e2e512783209839a4b70d7cf852c17c2d911a23de4514,
# a204 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841,
# a200 c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541.
# Hasher origin: a266 tools/sha256.rs
#   sha256 55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e
#   (a199 manifest tool d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3).
# Changes: owner a331, this session, fixed 15558917, review evidence path,
# private network and volumes, and the archive must already contain
# tools/query-result-export. No product overlay. No cargo test. No PostgreSQL.
# 6g, 1 CPU, global rust-builder cap 10. No pull, prune, or host cargo.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a331/SP05/query-result-export-review
EVIDENCE=${REVIEW}/linux/smoke
ROOT=/tmp/oo-a331-eef831aa-dba4-4af2-a78b-541bc9e0a331
FIXED=15558917e2c5be12b74dd1d64fb542b7d05a03af
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a331-smoke
NET=oo-a331-eef831aa-net
CARGO_VOL=oo-a331-eef831aa-cargo
TARGET_VOL=oo-a331-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$REVIEW/source/blobs" "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
if [ ! -d "$ROOT/archive/tools/query-result-export" ]; then
  echo "archive is missing query-result-export" >&2
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
blob_ok=0
{
  while IFS='|' read -r path name; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s %s want=%s got=%s\n' "$path" "$name" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $path" >&2
      blob_ok=1
    fi
    cp "$ROOT/archive/$path" "$REVIEW/source/blobs/$name"
  done <<'BLOBS'
contracts/gateway/query-pg-cli-v1.md|query-pg-cli-v1.md
crates/oo-gateway/src/bin/query-pg.rs|query-pg.rs
docs/10-security.md|10-security.md
tools/query-result-export/CONTRACT.md|query-result-export-CONTRACT.md
tools/query-result-export/src/csv.rs|csv.rs
tools/query-result-export/src/lib.rs|lib.rs
BLOBS
} > "$EVIDENCE/archive-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" grep -h -n 'E44 / FILE-D04' "$FIXED" -- specs/open-ontology-v0.1-plan.md > "$REVIEW/source/blobs/e44-plan-line.txt"
if ! grep -q 'exactdecimal' "$REVIEW/source/blobs/e44-plan-line.txt"; then
  echo "frozen E44 line is missing exactdecimal" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume inspect --format '{{.Name}} {{.Mountpoint}}' "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a331 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  echo "named smoke container already exists" >&2
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
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a331 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a331 \
  --label oo.task=query-result-export-review-smoke \
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
