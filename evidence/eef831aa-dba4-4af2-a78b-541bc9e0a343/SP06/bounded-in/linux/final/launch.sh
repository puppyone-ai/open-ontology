#!/bin/bash
# Final builder. Archive 21cc962 plus sql_guard.rs and bounded_in.rs only.
# One 6 GiB builder. PostgreSQL is started separately. No pull. No prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/linux/final
ROOT=/tmp/oo-a343-eef831aa-dba4-4af2-a78b-541bc9e0a343/final
SEC=/tmp/oo-a343-eef831aa-dba4-4af2-a78b-541bc9e0a343/secrets
FIXED=21cc96241a841b47dbbcb116549030e38a643ba0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a343-final
NET=oo-a343-eef831aa-net
CARGO_VOL=oo-a343-eef831aa-cargo
TARGET_VOL=oo-a343-eef831aa-target
GUARD_BLOB=1db8f3ed11a2d816d485bdd2865f37aff3e5cbed

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/ca.crt" ]; then
  echo "private fixture secrets are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-gateway/src/sql_guard.rs" > "$EVIDENCE/archive-sql-guard-blob.txt"
if [ "$(cat "$EVIDENCE/archive-sql-guard-blob.txt")" != "$GUARD_BLOB" ]; then
  echo "archive sql_guard blob is not the fixed commit" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/bounded_in.rs" ]; then
  echo "archive already contains bounded_in.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
for rel in \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  Cargo.lock
do
  want=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  got=$(git -C "$SRC" hash-object "$ROOT/archive/${rel}")
  if [ "$want" != "$got" ]; then
    echo "archive drift ${rel} want=${want} got=${got}" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
cp "$SRC/crates/oo-gateway/src/sql_guard.rs" "$ROOT/archive/crates/oo-gateway/src/sql_guard.rs"
cp "$SRC/crates/oo-gateway/tests/bounded_in.rs" "$ROOT/archive/crates/oo-gateway/tests/bounded_in.rs"
if cmp -s "$SRC/crates/oo-gateway/src/sql_guard.rs" <(git -C "$SRC" show "${FIXED}:crates/oo-gateway/src/sql_guard.rs"); then
  echo "overlay sql_guard.rs is identical to the archive; the admission change is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
(
  cd "$ROOT/archive"
  shasum -a 256 \
    crates/oo-gateway/src/sql_guard.rs \
    crates/oo-gateway/tests/bounded_in.rs \
    Cargo.lock \
    crates/oo-gateway/Cargo.toml \
    crates/oo-gateway/src/query_executor.rs \
    crates/oo-gateway/src/policy.rs \
    crates/oo-gateway/src/policy_catalog.rs \
    crates/oo-gateway/src/lib.rs \
    crates/oo-gateway/src/bin/query-pg.rs \
    > "$EVIDENCE/overlay.sha256"
)
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a343 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named final container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker ps --filter name=^/oo-a343-pg$ --filter status=running --format '{{.Names}}' | grep -q '^oo-a343-pg$'; then
  echo "private postgres is not running" >&2
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
  -e CARGO_TARGET_DIR=/target/a343-final \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_NET_OFFLINE=true \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/run/oo-a343/fixture,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in,target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.task=bounded-in-final \
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
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
