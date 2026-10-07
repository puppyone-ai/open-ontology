#!/bin/bash
# One builder for the actual PG decimal chain. Fixed archives only.
# Origin: a333 actual-pg-chain linux/chain/launch.sh
# SHA-256 effe7455c3f4323456d1b4705d702520666f576c9771d24edf365f69d5d21bfa
# Changes: owner a344, exporter archive 62b4afec, producer archive 21cc9624,
# 6GiB, 1 CPU, private /tmp root. No source copy from the moving worktree.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a344/SP05/query-result-export-chain-review
LIVE=${REVIEW}/linux/chain
ROOT=/tmp/oo-a344-eef831aa-dba4-4af2-a78b-541bc9e0a344
SEC=${ROOT}/secrets
PRODUCER=21cc96241a841b47dbbcb116549030e38a643ba0
EXPORTER=62b4afecdd377fa94310f46467c48a01e61d14db
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a344-chain
NET=oo-a344-eef831aa-net
CARGO_VOL=oo-a344-eef831aa-cargo
TARGET_VOL=oo-a344-eef831aa-target

chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh" "$LIVE/extract-libclang.sh"
mkdir -p "$LIVE/phases"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite chain console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$REVIEW/linux/smoke/smoke.exit" ] || [ "$(cat "$REVIEW/linux/smoke/smoke.exit")" != "0" ]; then
  echo "exporter smoke has not passed" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ]; then
  echo "private secrets are missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker inspect -f '{{.State.Running}}' oo-a344-pg 2>/dev/null | grep -q true; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse "$PRODUCER" > "$LIVE/producer-commit.txt"
git -C "$SRC" rev-parse "$EXPORTER" > "$LIVE/exporter-commit.txt"
git -C "$SRC" merge-base --is-ancestor "$PRODUCER" HEAD
mkdir -p "$ROOT"
rm -rf "$ROOT/producer-archive"
mkdir -p "$ROOT/producer-archive"
git -C "$SRC" archive --format=tar "$PRODUCER" Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -xf - -C "$ROOT/producer-archive"
if [ ! -f "$ROOT/producer-archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "archive is missing query-pg" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -d "$ROOT/export-archive/tools/query-result-export" ]; then
  mkdir -p "$ROOT/export-archive"
  git -C "$SRC" archive --format=tar "$EXPORTER" tools/query-result-export | tar -xf - -C "$ROOT/export-archive"
fi
bad=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  digest=${line%% *}
  rel=${line#*  }
  got=$(shasum -a 256 "$ROOT/producer-archive/$rel" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "archive hash mismatch $rel" >&2
    bad=1
  fi
done < "$REVIEW/source/expected.sha256"
if [ "$bad" -ne 0 ]; then
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
echo "archive_hash_ok"

ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$LIVE/host-cargo-count.txt"
host_n=$(cat "$LIVE/host-cargo-count.txt")
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before.txt" || true
builder_n=$(grep -c . "$LIVE/builders-before.txt" || true)
printf 'host_cargo=%s rust_builders=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" > "$LIVE/capacity.txt"
if [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity host=$host_n builders=$builder_n" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter status=running --filter label=oo.owner=a344 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named chain container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
printf '%s\n' "producer=${PRODUCER} exporter=${EXPORTER} memory=6g cpus=1 network=${NET} pg=oo-a344-pg overlay=none" > "$LIVE/invocation.txt"
sync
sleep 2

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
  -e CARGO_TARGET_DIR=/target/a344 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A327_HOST=oo-a344-pg \
  -e OO_A327_PORT=5432 \
  -e OO_A327_ADMIN_USER=oo_fixture_admin \
  -e OO_A327_ADMIN_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_A327_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A327_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
  -e OO_A327_LOGIN_PASSWORD_FILE=/secrets/login.pass \
  -e OO_A327_CA_FILE=/secrets/ca.crt \
  -e OO_A327_EVIDENCE=/evidence/linux/chain \
  -e OO_A327_PIPELINE=/target/a344/debug/oo-pipeline \
  -e OO_A327_BUILDER=/target/a344/debug/oo-builder \
  -e OO_A327_REVIEWED_EFFECTIVE=/target/a344/debug/reviewed-effective-oos \
  -e OO_A327_ROLE_PACK_BUNDLE=/target/a344/debug/role-pack-bundle \
  -e OO_A327_REVIEWED_POLICY=/target/a344/debug/reviewed-policy \
  -e OO_A327_INSTALL=/target/a344/debug/install-column-acl \
  -e OO_A327_MAPPING=/target/a344/debug/provision-mapping-reader \
  -e OO_A327_ANALYZE=/target/a344/debug/analyze-pg \
  -e OO_A327_INIT=/target/a344/debug/init-pg \
  -e OO_A327_QUERY=/target/a344/debug/query-pg \
  --mount "type=bind,source=${ROOT}/producer-archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/export-archive/tools/query-result-export,target=/export-src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${LIVE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/chain-oracle.rs,target=/tools/chain-oracle.rs,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a344 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a344 \
  --label oo.task=query-result-export-chain-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$LIVE/hostconfig.txt"
docker rm "$NAME" > "$LIVE/container-removed.txt" 2>&1 || true
exit "$run_exit"
