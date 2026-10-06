#!/bin/bash
# Build the fixed-archive query-pg and the evidence example, then run one TLS probe.
# The adapter and comparator binaries stay the ones already built in the post target.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/post/launch.sh
# Delta: no product overlay, no fmt sync, secrets and the private Postgres network,
# container oo-a276-live. Postgres is started by pg/up.sh before this script.
set -uo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/live
ROOT=/tmp/oo-a276-eef831aa-dba4-4af2-a78b-541bc9e0a276/live
SEC=/tmp/oo-a276-eef831aa-dba4-4af2-a78b-541bc9e0a276/secrets
FIXED=8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a276-live
NET=oo-a276-eef831aa-net
CARGO_VOL=oo-a276-eef831aa-cargo
TARGET_VOL=oo-a276-eef831aa-target
PG_NAME=oo-a276-pg

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite live console.log" >&2
  exit 2
fi
trap 'code=$?; if [ ! -f "$EVIDENCE/docker-run.exit" ]; then printf "%s\n" "$code" > "$EVIDENCE/docker-run.exit"; fi' EXIT
if [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/ca.crt" ]; then
  echo "postgres secrets are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker inspect -f '{{.State.Running}}' "$PG_NAME" 2>/dev/null | grep -q true; then
  echo "owned postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/question-bank benchmarks/sql-runner \
  benchmarks/query-result-adapter benchmarks/question-bank-suite \
  benchmarks/result-comparator \
  | tar -x -C "$ROOT/archive"
mkdir -p "$ROOT/archive/crates/oo-gateway/examples"
cp "$EVIDENCE/a276_empty_result.rs" "$ROOT/archive/crates/oo-gateway/examples/a276_empty_result.rs"
shasum -a 256 "$EVIDENCE/a276_empty_result.rs" > "$EVIDENCE/example-sha256.txt"
shasum -a 256 \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/post/launch.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/pg/up.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/pg/pg-wrap.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a244/SP12/sql-runner-review/linux/proof/extract-libclang.sh" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/sha256.rs" \
  "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/tools/select-debs.rs" \
  > "$EVIDENCE/origin-recipes.txt"
shasum -a 256 "$ROOT/archive/rust-toolchain.toml" > "$EVIDENCE/toolchain-file.sha256"
mv "$ROOT/archive/rust-toolchain.toml" "$ROOT/archive/rust-toolchain.toml.repo"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" merge-base --is-ancestor "$FIXED" HEAD
printf '%s\n' "$?" > "$EVIDENCE/fixed-ancestor.exit"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a276 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a276 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_pg=%s cap_global=10 cap_own=1 cap_pg=1\n' \
  "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: rust builder capacity or postgres count" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named live container already exists" >&2
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
  -e CARGO_TARGET_DIR=/target/a276-live \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A276_QUERY_PG=/target/a276-live/debug/query-pg \
  -e OO_A276_ADAPTER=/target/a276-deps/debug/query-result-adapter \
  -e OO_A276_COMPARATOR=/target/a276-deps/debug/result-comparator \
  -e OO_A276_LOGIN_FILE=/secrets/login.pass \
  -e OO_A276_ADMIN_FILE=/secrets/admin.pass \
  -e OO_A276_CA=/secrets/ca.crt \
  -e OO_A276_MIGRATIONS=/src/crates/oo-pipeline/migrations \
  -e OO_A276_OUT=/evidence/out \
  --mount "type=bind,source=${ROOT}/archive,target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a276 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a276 \
  --label oo.task=benchmark-empty-result-live \
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
