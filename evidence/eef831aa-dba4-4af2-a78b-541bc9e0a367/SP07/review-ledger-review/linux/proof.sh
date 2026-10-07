#!/bin/bash
# Fresh archive of the fixed product, one private PostgreSQL, one Rust builder.
# Does not pull, prune, or modify the git product tree.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a367
REPO=/Users/redux/puppy-one/open-ontology
EVID=${REPO}/evidence/${RUN_ID}/SP07/review-ledger-review
ROOT=/tmp/oo-a367-${RUN_ID}
SEC=${ROOT}/secrets
COMMIT=6b3a92c2e06a2e91a74393b739ec8f1c565ae17b
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a367-eef831aa-net
PG_NAME=oo-a367-pg
RUST_NAME=oo-a367-rust
CARGO_VOL=oo-a367-eef831aa-cargo
TARGET_VOL=oo-a367-eef831aa-target
STATE_VOL=oo-a367-eef831aa-state
HOST_UID=$(id -u)
HOST_GID=$(id -g)

mkdir -p "$EVID/linux/author" "$EVID/linux/oracle" "$EVID/origin" "$EVID/failed-attempts" "$EVID/oracle"
rm -f "$EVID/linux/proof.exit"
finish() {
  code=$?
  printf '%s\n' "$code" > "$EVID/linux/proof.exit"
}
trap finish EXIT

capacity() {
  local global own host n c
  global=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
  own=$(docker ps -q --filter label=oo.owner=a367 --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
  host=0
  for n in cargo rustc rustdoc rustup; do
    c=$( (pgrep -x "$n" || true) | wc -l | tr -d ' ')
    host=$((host + c))
  done
  printf 'own=%s global=%s host_tools=%s\n' "$own" "$global" "$host" | tee "$EVID/linux/author/builders-before.txt"
  if [ "$global" -ge 10 ] || [ "$own" -ge 1 ] || [ "$host" -ge 1 ]; then
    echo "builder capacity exceeded" >&2
    exit 3
  fi
}

archive() {
  mkdir -p "$ROOT/archive" "$ROOT/build"
  if [ ! -f "$EVID/origin/archive.sha256" ]; then
    rm -rf "$ROOT/archive"
    mkdir -p "$ROOT/archive"
    git -C "$REPO" archive --format=tar "$COMMIT" \
      Cargo.toml rust-toolchain.toml \
      crates/oo-builder crates/oo-core crates/oo-jobs \
      tools/review-ledger tools/description-pack tools/description-review \
      | tar -x -C "$ROOT/archive"
    tar -C "$ROOT/archive" -cf - . | shasum -a 256 | awk '{print $1}' > "$EVID/origin/archive.sha256"
    {
      echo "commit=$COMMIT"
      echo "archive_sha256=$(cat "$EVID/origin/archive.sha256")"
      mismatch=0
      while IFS= read -r path; do
        blob=$(git -C "$REPO" rev-parse "$COMMIT:$path")
        file=$(git -C "$REPO" hash-object "$ROOT/archive/$path")
        if [ "$blob" = "$file" ]; then
          printf 'match %s %s\n' "$blob" "$path"
        else
          printf 'MISMATCH %s %s %s\n' "$blob" "$file" "$path"
          mismatch=1
        fi
      done < <(git -C "$REPO" ls-tree -r --name-only "$COMMIT" -- \
        Cargo.toml rust-toolchain.toml \
        crates/oo-builder crates/oo-core crates/oo-jobs \
        tools/review-ledger tools/description-pack tools/description-review)
      printf 'mismatches=%s\n' "$mismatch"
    } > "$EVID/origin/source-inventory.txt"
    grep -q 'mismatches=0' "$EVID/origin/source-inventory.txt"
  fi
  if [ ! -f "$ROOT/build/tools/review-ledger/Cargo.lock" ]; then
    rm -rf "$ROOT/build"
    mkdir -p "$ROOT/build"
    tar -C "$ROOT/archive" -cf - crates tools | tar -x -C "$ROOT/build"
    cat > "$ROOT/build/Cargo.toml" <<'EOF'
[workspace]
resolver = "3"
members = [
    "crates/oo-builder",
    "crates/oo-core",
    "crates/oo-jobs",
]
[workspace.package]
edition = "2024"
license = "Apache-2.0"
publish = false
rust-version = "1.94.0"
version = "0.1.0"
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
[workspace.lints.clippy]
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
EOF
  fi
  test ! -f "$ROOT/build/rust-toolchain.toml"
  rm -f "$ROOT/build/tools/review-ledger/tests/oracle_a367.rs"
}

hash_build() {
  local out=$1
  (
    cd "$ROOT/build"
    find crates tools -type f ! -name oracle_a367.rs -print | sort | xargs shasum -a 256
  ) > "$out"
}

ensure_pg() {
  if docker ps -a --filter "name=^/${PG_NAME}$" --format '{{.Names}}' | grep -qx "$PG_NAME"; then
    return 0
  fi
  "$EVID/pg/up.sh"
}

ensure_volumes() {
  docker volume create --label oo.owner=a367 --label oo.session="${RUN_ID}" --label oo.kind=cargo "$CARGO_VOL" >/dev/null
  docker volume create --label oo.owner=a367 --label oo.session="${RUN_ID}" --label oo.kind=target "$TARGET_VOL" >/dev/null
  docker volume create --label oo.owner=a367 --label oo.session="${RUN_ID}" --label oo.kind=state "$STATE_VOL" >/dev/null
}

run_builder() {
  local phase=$1
  local script=$2
  local log=$3
  local marker_phase=$4
  local runtime_file=$5
  capacity
  ensure_volumes
  docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVID/linux/author/rust-image.txt"
  if docker ps -a --format '{{.Names}}' | grep -qx "$RUST_NAME"; then
    local owner
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$RUST_NAME" 2>/dev/null || true)
    if [ "$owner" != "a367" ]; then
      echo "foreign container $RUST_NAME" >&2
      exit 1
    fi
    docker rm -f "$RUST_NAME" >/dev/null
  fi
  set +e
  docker run --name "$RUST_NAME" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --security-opt no-new-privileges:true \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 512 \
    --network "$NET" \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS=1 \
    -e CARGO_PROFILE_DEV_DEBUG=0 \
    -e CARGO_PROFILE_TEST_DEBUG=0 \
    -e RUSTUP_HOME=/usr/local/rustup \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -e REVIEW_LEDGER_HOST=pg.a367.internal \
    -e REVIEW_LEDGER_PORT=5432 \
    -e REVIEW_LEDGER_CA=/secrets/ca.crt \
    -e REVIEW_LEDGER_ADMIN_PASSWORD_FILE=/secrets/ledger-admin.pass \
    -e REVIEW_LEDGER_RUNTIME_PASSWORD_FILE="$runtime_file" \
    -e REVIEW_LEDGER_SUPER_PASSWORD_FILE=/secrets/super.pass \
    -e REVIEW_LEDGER_DATABASE=oo_a367_oracle \
    -e REVIEW_LEDGER_STATE=/state \
    -e REVIEW_LEDGER_MARKER=/evidence/linux/author/history.sha256 \
    -e REVIEW_LEDGER_ORACLE_OUT=/evidence/oracle/summary.json \
    -e REVIEW_LEDGER_PHASE="$marker_phase" \
    -e HOST_UID="$HOST_UID" \
    -e HOST_GID="$HOST_GID" \
    --mount "type=bind,source=${ROOT}/build,target=/build" \
    --mount "type=bind,source=${EVID},target=/evidence" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --mount "type=volume,source=${STATE_VOL},target=/state" \
    --label oo.project=open-ontology \
    --label oo.owner=a367 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=rust-builder \
    --label oo.task=review-ledger-review \
    -w /build/tools/review-ledger \
    "$RUST_IMAGE" \
    bash "/evidence/linux/${script}" > "$log" 2>&1
  local code=$?
  set -e
  docker inspect "$RUST_NAME" --format 'exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVID/linux/author/${phase}-inspect.txt"
  docker inspect "$RUST_NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVID/linux/author/${phase}-caps.json"
  docker rm "$RUST_NAME" >/dev/null
  printf '%s\n' "$code" > "$EVID/linux/author/${phase}.exit"
  if [ "$code" -ne 0 ]; then
    cp "$log" "$EVID/failed-attempts/${phase}.log"
    echo "phase $phase failed" >&2
    exit "$code"
  fi
}

reset_author_database() {
  docker exec -u postgres "$PG_NAME" psql -U oo_a367_admin -d postgres -v ON_ERROR_STOP=1 \
    -c 'DROP DATABASE IF EXISTS oo_a358_review' \
    -c 'CREATE DATABASE oo_a358_review OWNER oo_review_ledger_admin' \
    > "$EVID/linux/author/database-reset.out"
}

run_phases() {
    hash_build "$EVID/linux/author/tree-before.txt"
    run_builder test inside-test.sh "$EVID/linux/author/test.log" write /secrets/runtime-a358.pass
    hash_build "$EVID/linux/author/tree-after-test.txt"
    diff -q "$EVID/linux/author/tree-before.txt" "$EVID/linux/author/tree-after-test.txt"
    run_builder clippy inside-clippy.sh "$EVID/linux/author/clippy.log" clippy /secrets/runtime-a358.pass
    docker restart "$PG_NAME" >/dev/null
    ready=0
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
      if docker exec -u postgres "$PG_NAME" pg_isready -U oo_a367_admin -d postgres >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 2
    done
    [ "$ready" -eq 1 ]
    run_builder restart inside-restart.sh "$EVID/linux/author/restart.log" restart /secrets/runtime-a358.pass
    cp "$EVID/oracle/oracle.rs" "$ROOT/build/tools/review-ledger/tests/oracle_a367.rs"
    run_builder oracle inside-oracle.sh "$EVID/linux/oracle/oracle.log" oracle /secrets/runtime-a367.pass
    hash_build "$EVID/linux/oracle/tree-after-oracle.txt"
    diff -q "$EVID/linux/author/tree-before.txt" "$EVID/linux/oracle/tree-after-oracle.txt"
    rm -f "$ROOT/build/tools/review-ledger/tests/oracle_a367.rs"
    if [ ! -f "$EVID/failed-attempts/attempt-1-why.txt" ]; then
      printf 'none\n' > "$EVID/failed-attempts/none.txt"
    fi
}

phase="${1:-all}"
archive
ensure_pg
case "$phase" in
  all)
    run_phases
    ;;
  resume)
    reset_author_database
    run_phases
    ;;
  oracle)
    docker exec -u postgres "$PG_NAME" psql -U oo_a367_admin -d postgres -v ON_ERROR_STOP=1 \
      -c 'DROP DATABASE IF EXISTS oo_a367_oracle' \
      -c 'CREATE DATABASE oo_a367_oracle OWNER oo_review_ledger_admin' \
      > "$EVID/linux/oracle/database-reset.out"
    cp "$EVID/oracle/oracle.rs" "$ROOT/build/tools/review-ledger/tests/oracle_a367.rs"
    run_builder oracle inside-oracle.sh "$EVID/linux/oracle/oracle.log" oracle /secrets/runtime-a367.pass
    hash_build "$EVID/linux/oracle/tree-after-oracle.txt"
    diff -q "$EVID/linux/author/tree-before.txt" "$EVID/linux/oracle/tree-after-oracle.txt"
    rm -f "$ROOT/build/tools/review-ledger/tests/oracle_a367.rs"
    ;;
  *)
    echo "usage: proof.sh all|resume" >&2
    exit 2
    ;;
esac
