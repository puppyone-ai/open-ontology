#!/bin/bash
# Immutable archive of 6e1e6e57 plus the review harness only. One 6GiB builder.
# Does not pull, prune, or publish. Retries once on OOM with line-tables-only.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review/linux/proof
SMOKE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review/linux/smoke
ROOT=/tmp/oo-a232-eef831aa-dba4-4af2-a78b-541bc9e0a232/proof
FIXED=6e1e6e576d1afd2bde10dedad649f09dee95bd34
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a232-proof
NET=oo-a232-eef831aa-net
CARGO_VOL=oo-a232-eef831aa-cargo
TARGET_VOL=oo-a232-eef831aa-target
SEC=/tmp/oo-a232-eef831aa-dba4-4af2-a78b-541bc9e0a232/secrets
BUILDER_SEC=/tmp/oo-a232-eef831aa-dba4-4af2-a78b-541bc9e0a232/builder-secrets
OVERLAY=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review/overlay/init_pg_a232_review.rs
ATTEMPT=${1:-1}

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/tree"
mkdir -p "$ROOT/tree" "$EVIDENCE/live"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/tree"
cp "$OVERLAY" "$ROOT/tree/crates/oo-pipeline/tests/init_pg_a232_review.rs"
if [ ! -f "$ROOT/tree/crates/oo-pipeline/tests/init_pg_a232_review.rs" ]; then
  echo "overlay copy failed" >&2
  printf '%s\n' 2 > "$EVIDENCE/overlay-copied.exit"
  exit 2
fi
printf '%s\n' 0 > "$EVIDENCE/overlay-copied.exit"
shasum -a 256 "$OVERLAY" | awk '{print $1 "  crates/oo-pipeline/tests/init_pg_a232_review.rs"}' > "$EVIDENCE/overlay-expect.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker exec -u postgres oo-a232-pg pg_isready -U oo_a232_admin -d postgres >/dev/null 2>&1; then
  echo "private postgres is not ready" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
umask 077
mkdir -p "$BUILDER_SEC"
chmod 700 "$BUILDER_SEC"
cp "$SEC/admin.pass" "$BUILDER_SEC/admin.pass"
cp "$SEC/ca.crt" "$BUILDER_SEC/ca.crt"
cp "$SEC/wrong-ca.crt" "$BUILDER_SEC/wrong-ca.crt"
chmod 600 "$BUILDER_SEC/admin.pass"
chmod 644 "$BUILDER_SEC/ca.crt" "$BUILDER_SEC/wrong-ca.crt"
if [ -e "$BUILDER_SEC/ca.key" ] || [ -e "$BUILDER_SEC/server.key" ] || [ -e "$BUILDER_SEC/wrong-ca.key" ] || [ -e "$BUILDER_SEC/server.crt" ]; then
  echo "builder secret dir contains a key" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a232 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a232 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s own_pg=%s host_cargo=%s cap_global=10 memory=6g cpus=1 attempt=%s\n' \
  "$own" "$global_builders" "$own_pg" "$host_cargo" "$ATTEMPT" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: rust builder or postgres capacity" >&2
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
debug_flag=
if [ "$ATTEMPT" -ge 2 ]; then
  debug_flag="-e CARGO_PROFILE_DEV_DEBUG=line-tables-only"
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 attempt=${ATTEMPT} bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
# shellcheck disable=SC2086
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
  -e OO_A232_PG_HOST=oo-a232-pg \
  -e OO_A232_PG_PORT=5432 \
  -e OO_A232_ADMIN_USER=oo_a232_admin \
  -e OO_A232_ADMIN_PASSWORD_FILE=/run/init-secrets/admin.pass \
  -e OO_A232_CA_FILE=/run/init-secrets/ca.crt \
  -e OO_A232_WRONG_CA_FILE=/run/init-secrets/wrong-ca.crt \
  -e OO_A232_SECRET_DIR=/tmp/a232-live-secrets \
  -e OO_A232_EVIDENCE=/evidence/live \
  $debug_flag \
  --mount "type=bind,source=${ROOT}/tree,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/base-sha256.txt,target=/smoke-base-sha256.txt,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review/source/expected-product-sha256.txt,target=/expected-product-sha256.txt,readonly" \
  --mount "type=bind,source=${EVIDENCE}/overlay-expect.txt,target=/overlay-sha256.txt,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${BUILDER_SEC},target=/run/init-secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a232 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a232 \
  --label oo.task=init-pg-review-proof \
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
oom=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ "$oom" = "true" ] && [ "$ATTEMPT" -eq 1 ]; then
  echo "OOMKilled on attempt 1; preserving and retrying line-tables-only" >&2
  mkdir -p "$EVIDENCE/attempt-1"
  find "$EVIDENCE" -mindepth 1 -maxdepth 1 ! -name attempt-1 ! -name launch.sh ! -name inside.sh -exec mv {} "$EVIDENCE/attempt-1/" \;
  printf '%s\n' "OOMKilled at 6g; retry with CARGO_PROFILE_DEV_DEBUG=line-tables-only" > "$EVIDENCE/attempt-1/why.txt"
  exec "$EVIDENCE/launch.sh" 2
fi
exit "$run_exit"
