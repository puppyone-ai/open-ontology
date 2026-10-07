#!/bin/bash
# Independent review builder. Fixed commit archive. No moving overlay.
# Origin: evidence/.../a327/SP06/numeric-query/linux/live-4/launch.sh
# sha256 2efc60a771d268bd531ad773f51b298e893ca7c0fe58de2531c89a34f25fe7dd
# Changes: owner a340, commit 21cc9624, 6GiB, 1 CPU, no source copy.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a340/SP06/numeric-query-review
LIVE=${EVIDENCE}/linux/proof
ROOT=${SRC}/.a340-work
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=21cc96241a841b47dbbcb116549030e38a643ba0
NAME=oo-a340-proof
NET=oo-a340-eef831aa-net
CARGO_VOL=oo-a340-eef831aa-cargo
TARGET_VOL=oo-a340-eef831aa-target

chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh" "$LIVE/extract-libclang.sh"
mkdir -p "$LIVE/phases"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ]; then
  echo "private secrets are missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$LIVE/fixed-commit.txt"
git -C "$SRC" merge-base --is-ancestor "$FIXED" HEAD
mkdir -p "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
# tools/gateway-token-store is read by crates/oo-gateway/build.rs at this commit.
git -C "$SRC" archive --format=tar "$FIXED" Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -xf - -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "archive is missing query-pg" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
bad=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  digest=${line%% *}
  rel=${line#*  }
  got=$(shasum -a 256 "$ROOT/archive/$rel" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "archive hash mismatch $rel" >&2
    bad=1
  fi
done < "$EVIDENCE/source/expected.sha256"
if [ "$bad" -ne 0 ]; then
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
echo "archive_hash_ok"
sync
sleep 2
ls "$ROOT/archive/Cargo.toml" "$SEC/ca.crt" >/dev/null

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a340 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a340 \
  --label oo.kind=network \
  "$NET" >/dev/null

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
own=$(docker ps -q --filter status=running --filter label=oo.owner=a340 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a340 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a340 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a340 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a340 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

printf '%s\n' "$FIXED" > "$LIVE/fixed-commit.txt"
cat > "$LIVE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a340
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
memory=6g
cpus=1
privileged=no
network=${NET}
pull=never
phase=proof
archive=${ROOT}/archive
overlay=none
fixed=${FIXED}
contract=3517cd131719cfdd572ba2b85bb7013e6c90588e
allowlist_addendum=c4d9316d28ceefefc7bc21cd8ac61ce949232d07
mapping_addendum=9183e79082341a1a2bd7d3b1a2ae3e8764735d0e
origin=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/linux/live-4/launch.sh
private_root=${ROOT}
private_root_reason=recreated_sibling_unbindable_workspace_child_binds
EOF

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
  -e CARGO_TARGET_DIR=/target/a340 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A327_HOST=oo-a340-pg \
  -e OO_A327_PORT=5432 \
  -e OO_A327_ADMIN_USER=oo_fixture_admin \
  -e OO_A327_ADMIN_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_A327_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A327_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
  -e OO_A327_LOGIN_PASSWORD_FILE=/secrets/login.pass \
  -e OO_A327_CA_FILE=/secrets/ca.crt \
  -e OO_A327_EVIDENCE=/evidence/linux/proof \
  -e OO_A327_PIPELINE=/target/a340/debug/oo-pipeline \
  -e OO_A327_BUILDER=/target/a340/debug/oo-builder \
  -e OO_A327_REVIEWED_EFFECTIVE=/target/a340/debug/reviewed-effective-oos \
  -e OO_A327_ROLE_PACK_BUNDLE=/target/a340/debug/role-pack-bundle \
  -e OO_A327_REVIEWED_POLICY=/target/a340/debug/reviewed-policy \
  -e OO_A327_INSTALL=/target/a340/debug/install-column-acl \
  -e OO_A327_MAPPING=/target/a340/debug/provision-mapping-reader \
  -e OO_A327_ANALYZE=/target/a340/debug/analyze-pg \
  -e OO_A327_INIT=/target/a340/debug/init-pg \
  -e OO_A327_QUERY=/target/a340/debug/query-pg \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${LIVE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/gold.rs,target=/tools/gold.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a340 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a340 \
  --label oo.task=numeric-query-review \
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
docker rm "$NAME" >/dev/null 2>&1 || true
exit "$run_exit"
