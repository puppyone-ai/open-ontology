#!/bin/bash
# Immutable archive a48db85 plus the three owned analyze-pg files. One 6GiB builder.
# Does not pull, prune, or publish. Retries once on OOM with line-tables-only.
# Origin: a223 proof launch 565c000e22451e27cda9895c150ddfa08811ffae96cb789ec55d6f9dffb84812
# and a204 overlay-3 launch 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a236/SP04/target-statistics-cli/linux/proof
SMOKE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a236/SP04/target-statistics-cli/linux/smoke
ROOT=/tmp/oo-a236-eef831aa-dba4-4af2-a78b-541bc9e0a236/proof
FIXED=a48db85fc13148874349d318affd995f7212188d
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a236-proof
NET=oo-a236-eef831aa-net
CARGO_VOL=oo-a236-eef831aa-cargo
TARGET_VOL=oo-a236-eef831aa-target
SEC=/tmp/oo-a236-eef831aa-dba4-4af2-a78b-541bc9e0a236/secrets
BUILDER_SEC=/tmp/oo-a236-eef831aa-dba4-4af2-a78b-541bc9e0a236/builder-secrets
TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a236/SP04/target-statistics-cli/tools
ATTEMPT=${1:-1}

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
rm -rf "$ROOT/tree"
mkdir -p "$ROOT/tree" "$EVIDENCE/live"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/tree"
cp "$SRC/crates/oo-pipeline/src/bin/analyze-pg.rs" "$ROOT/tree/crates/oo-pipeline/src/bin/analyze-pg.rs"
cp "$SRC/crates/oo-pipeline/tests/analyze_pg_cli.rs" "$ROOT/tree/crates/oo-pipeline/tests/analyze_pg_cli.rs"
cp "$SRC/contracts/pipeline/analyze-pg-cli-v1.md" "$ROOT/tree/contracts/pipeline/analyze-pg-cli-v1.md"
cp "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a236/SP04/target-statistics-cli/overlay/catalog_proof.rs" \
  "$ROOT/tree/crates/oo-gateway/tests/analyze_pg_catalog_proof.rs"
if [ -f "$ROOT/tree/crates/oo-pipeline/src/bin/analyze-pg.rs" ] \
  && [ -f "$ROOT/tree/crates/oo-pipeline/tests/analyze_pg_cli.rs" ] \
  && [ -f "$ROOT/tree/contracts/pipeline/analyze-pg-cli-v1.md" ] \
  && [ -f "$ROOT/tree/crates/oo-gateway/tests/analyze_pg_catalog_proof.rs" ]
then
  printf '%s\n' 0 > "$EVIDENCE/overlay-copied.exit"
else
  echo "overlay copy failed" >&2
  printf '%s\n' 2 > "$EVIDENCE/overlay-copied.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker exec -u postgres oo-a236-pg pg_isready -U oo_a236_admin -d postgres >/dev/null 2>&1; then
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
if [ ! -f "$BUILDER_SEC/login.pass" ]; then
  openssl rand -hex 24 > "$BUILDER_SEC/login.pass"
fi
chmod 600 "$BUILDER_SEC/admin.pass" "$BUILDER_SEC/login.pass"
chmod 644 "$BUILDER_SEC/ca.crt" "$BUILDER_SEC/wrong-ca.crt"
if [ -e "$BUILDER_SEC/ca.key" ] || [ -e "$BUILDER_SEC/server.key" ] || [ -e "$BUILDER_SEC/wrong.key" ]; then
  echo "builder secret dir contains a key" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a236 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a236 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
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
  -e OO_ANALYZE_PG_HOST=oo-a236-pg \
  -e OO_ANALYZE_PG_PORT=5432 \
  -e OO_ANALYZE_PG_ADMIN_USER=oo_a236_admin \
  -e OO_ANALYZE_PG_ADMIN_PASSWORD_FILE=/run/analyze-secrets/admin.pass \
  -e OO_ANALYZE_PG_CA_FILE=/run/analyze-secrets/ca.crt \
  -e OO_ANALYZE_PG_WRONG_CA_FILE=/run/analyze-secrets/wrong-ca.crt \
  -e OO_ANALYZE_PG_LOGIN_PASSWORD_FILE=/run/analyze-secrets/login.pass \
  -e OO_ANALYZE_PG_SECRET_DIR=/tmp/a236-live-secrets \
  -e OO_ANALYZE_PG_EVIDENCE=/evidence/live \
  $debug_flag \
  --mount "type=bind,source=${ROOT}/tree,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/archive-sha256.txt,target=/smoke-base-sha256.txt,readonly" \
  --mount "type=bind,source=${TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${BUILDER_SEC},target=/run/analyze-secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a236 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a236 \
  --label oo.task=analyze-pg-cli-proof \
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
  find "$EVIDENCE" -mindepth 1 -maxdepth 1 ! -name attempt-1 ! -name launch.sh ! -name inside.sh ! -name extract-libclang.sh -exec mv {} "$EVIDENCE/attempt-1/" \;
  printf '%s\n' "OOMKilled at 6g; retry with CARGO_PROFILE_DEV_DEBUG=line-tables-only" > "$EVIDENCE/attempt-1/why.txt"
  exec "$EVIDENCE/launch.sh" 2
fi
exit "$run_exit"
