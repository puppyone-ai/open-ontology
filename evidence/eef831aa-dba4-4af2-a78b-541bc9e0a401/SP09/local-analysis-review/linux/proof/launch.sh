#!/bin/bash
# One builder for the independent SP09-W05 review. Fixed archives only.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/linux/chain-full/launch.sh
# sha256 93d21511c3670f636e044b754dc928f002940778cbedd2f15ebfbaa98e0f9709
# Changes: owner a401, immutable local-analysis archive e2732e323, private PG,
# 6GiB, 1 CPU. No worktree mount. No root manifest change.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a401/SP09/local-analysis-review
LIVE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a401-eef831aa-dba4-4af2-a78b-541bc9e0a401
SEC=${ROOT}/secrets
PRODUCER=21cc96241a841b47dbbcb116549030e38a643ba0
EXPORTER=62b4afecdd377fa94310f46467c48a01e61d14db
ANALYSIS=e2732e323915a332a85f008c605b50c1f4d5e948
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a401-proof
NET=oo-a401-eef831aa-net
CARGO_VOL=oo-a401-eef831aa-cargo
TARGET_VOL=oo-a401-eef831aa-target
NATIVE_VOL=oo-a401-eef831aa-native

chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh" "$LIVE/extract-libclang.sh"
mkdir -p "$LIVE/phases" "$REVIEW/source"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$REVIEW/linux/smoke/smoke.exit" ] || [ "$(cat "$REVIEW/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-edit smoke has not passed" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ]; then
  echo "private secrets are missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker inspect -f '{{.State.Running}}' oo-a401-pg 2>/dev/null | grep -q true; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi
{
  echo "analysis=$ANALYSIS"
  echo "producer=$PRODUCER"
  echo "exporter=$EXPORTER"
  echo "analysis_tree=$(git -C "$SRC" rev-parse "$ANALYSIS^{tree}")"
  echo "head=$(git -C "$SRC" rev-parse HEAD)"
} > "$LIVE/fixed-commit.txt"
git -C "$SRC" rev-parse "$PRODUCER" > "$LIVE/producer-commit.txt"
git -C "$SRC" rev-parse "$EXPORTER" > "$LIVE/exporter-commit.txt"
git -C "$SRC" merge-base --is-ancestor "$PRODUCER" HEAD
git -C "$SRC" merge-base --is-ancestor "$EXPORTER" HEAD
git -C "$SRC" merge-base --is-ancestor "$ANALYSIS" HEAD
mkdir -p "$ROOT"
extract_archives() {
  rm -rf "$ROOT/producer-archive" "$ROOT/export-archive" "$ROOT/analysis-archive"
  mkdir -p "$ROOT/producer-archive" "$ROOT/export-archive" "$ROOT/analysis-archive"
  git -C "$SRC" archive --format=tar "$PRODUCER" Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
    | tar -xf - -C "$ROOT/producer-archive"
  if [ ! -f "$ROOT/producer-archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
    echo "archive is missing query-pg" >&2
    printf '%s\n' 2 > "$LIVE/docker-run.exit"
    exit 2
  fi
  git -C "$SRC" archive --format=tar "$EXPORTER" tools/query-result-export | tar -xf - -C "$ROOT/export-archive"
  git -C "$SRC" archive --format=tar "$ANALYSIS" tools/local-analysis | tar -xf - -C "$ROOT/analysis-archive"
  if [ ! -f "$ROOT/analysis-archive/tools/local-analysis/src/lib.rs" ]; then
    echo "archive is missing local-analysis" >&2
    printf '%s\n' 2 > "$LIVE/docker-run.exit"
    exit 2
  fi
  sync
  sleep 5
}
if [ ! -f "$ROOT/producer-archive/crates/oo-gateway/src/bin/query-pg.rs" ] \
  || [ ! -d "$ROOT/export-archive/tools/query-result-export" ] \
  || [ ! -f "$ROOT/analysis-archive/tools/local-analysis/src/lib.rs" ]; then
  extract_archives
fi
pass=0
while [ "$pass" -lt 2 ]; do
  pass=$((pass + 1))
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
  : > "$REVIEW/source/la-blobs.sha256"
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    git -C "$SRC" rev-parse "$ANALYSIS:$rel" > "$ROOT/blob-expect"
    git -C "$SRC" hash-object "$ROOT/analysis-archive/$rel" > "$ROOT/blob-got"
    if ! cmp -s "$ROOT/blob-expect" "$ROOT/blob-got"; then
      echo "local-analysis blob mismatch $rel" >&2
      bad=1
    fi
    echo "$(cat "$ROOT/blob-got")  $rel" >> "$REVIEW/source/la-blobs.sha256"
  done < <(git -C "$SRC" ls-tree -r --name-only "$ANALYSIS" tools/local-analysis)
  rm -f "$ROOT/blob-expect" "$ROOT/blob-got"
  if [ "$bad" -eq 0 ]; then
    break
  fi
  if [ "$pass" -eq 2 ]; then
    printf '%s\n' 2 > "$LIVE/docker-run.exit"
    exit 2
  fi
  extract_archives
done
cp "$REVIEW/linux/focused/a401_bounds.rs" "$ROOT/analysis-archive/tools/local-analysis/tests/a401_bounds.rs"
cp "$REVIEW/linux/focused/extract_otf.rs" "$ROOT/analysis-archive/tools/local-analysis/examples/extract_otf.rs"
if [ -f "$ROOT/analysis-archive/tools/local-analysis/tests/a401_bounds.rs" ] && git -C "$SRC" cat-file -e "$ANALYSIS:tools/local-analysis/tests/a401_bounds.rs" 2>/dev/null; then
  echo "overlay path already exists in the fixed commit" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
printf '%s\n' "archive_hash_ok analysis=$ANALYSIS" > "$LIVE/archive-ok.txt"

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
own=$(docker ps -q --filter status=running --filter label=oo.owner=a401 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a401 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a401 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a401 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a401 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
docker volume inspect "$NATIVE_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a401 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a401 --label oo.kind=native \
  "$NATIVE_VOL" >/dev/null
printf '%s\n' "producer=${PRODUCER} exporter=${EXPORTER} analysis=${ANALYSIS} memory=6g cpus=1 network=${NET} pg=oo-a401-pg" > "$LIVE/invocation.txt"
sync
sleep 2

stage_binds() {
  STAGE="$ROOT/stage-$(date -u +%Y%m%dT%H%M%S)-$1"
  mkdir -p "$STAGE"
  cp -a "$ROOT/producer-archive" "$STAGE/producer-archive"
  cp -a "$ROOT/export-archive" "$STAGE/export-archive"
  cp -a "$ROOT/analysis-archive" "$STAGE/analysis-archive"
  PRODUCER_MNT="$STAGE/producer-archive"
  EXPORT_MNT="$STAGE/export-archive/tools/query-result-export"
  ANALYSIS_MNT="$STAGE/analysis-archive/tools/local-analysis"
  printf '%s\n' "$STAGE" > "$LIVE/archive-stage.txt"
  sync
}
run_exit=125
bind_try=0
while [ "$bind_try" -lt 5 ]; do
  bind_try=$((bind_try + 1))
  stage_binds "$bind_try"
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
  -e CARGO_TARGET_DIR=/target/a401 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A327_HOST=oo-a401-pg \
  -e OO_A327_PORT=5432 \
  -e OO_A327_ADMIN_USER=oo_fixture_admin \
  -e OO_A327_ADMIN_PASSWORD_FILE=/secrets/admin.pass \
  -e OO_A327_READER_PASSWORD_FILE=/secrets/reader.pass \
  -e OO_A327_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
  -e OO_A327_LOGIN_PASSWORD_FILE=/secrets/login.pass \
  -e OO_A327_CA_FILE=/secrets/ca.crt \
  -e OO_A327_EVIDENCE=/evidence/linux/proof \
  -e OO_A327_PIPELINE=/target/a401/debug/oo-pipeline \
  -e OO_A327_BUILDER=/target/a401/debug/oo-builder \
  -e OO_A327_REVIEWED_EFFECTIVE=/target/a401/debug/reviewed-effective-oos \
  -e OO_A327_ROLE_PACK_BUNDLE=/target/a401/debug/role-pack-bundle \
  -e OO_A327_REVIEWED_POLICY=/target/a401/debug/reviewed-policy \
  -e OO_A327_INSTALL=/target/a401/debug/install-column-acl \
  -e OO_A327_MAPPING=/target/a401/debug/provision-mapping-reader \
  -e OO_A327_ANALYZE=/target/a401/debug/analyze-pg \
  -e OO_A327_INIT=/target/a401/debug/init-pg \
  -e OO_A327_QUERY=/target/a401/debug/query-pg \
  --mount "type=bind,source=${PRODUCER_MNT},target=/src,readonly" \
  --mount "type=bind,source=${EXPORT_MNT},target=/export-src,readonly" \
  --mount "type=bind,source=${ANALYSIS_MNT},target=/local-analysis,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${LIVE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/chain-oracle.rs,target=/tools/chain-oracle.rs,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${NATIVE_VOL},target=/native" \
  --label oo.project=open-ontology \
  --label oo.owner=a401 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a401 \
  --label oo.task=sp09-local-analysis-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
if [ "$run_exit" -eq 125 ] && grep -q 'bind source path does not exist' "$LIVE/console.log"; then
  mkdir -p "$LIVE/failed-attempts"
  cp "$LIVE/console.log" "$LIVE/failed-attempts/docker-bind-${bind_try}.log"
  echo "bind_try=$bind_try exit=125" >> "$LIVE/docker-bind-retry.txt"
  docker rm "$NAME" >/dev/null 2>&1 || true
  sleep 3
  continue
fi
break
done
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
