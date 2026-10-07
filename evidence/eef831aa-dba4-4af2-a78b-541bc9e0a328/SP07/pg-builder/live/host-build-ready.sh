#!/bin/bash
# Host pg-builder for the readiness run. Run only after the focused-6 container is gone.
# The tree is the focused-6 archive, not the moving workspace.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
ROOT=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328
ARCHIVE=${ROOT}/focused-6/archive
HOST=${ROOT}/ready-host
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
if docker ps -q --filter name=oo-a328-focused-6 --filter status=running | grep -q .; then
  echo "linux builder is still running" >&2
  exit 3
fi
if docker ps -q --filter status=running --filter label=oo.owner=a328 --filter label=oo.kind=rust-builder | grep -q .; then
  echo "an a328 rust builder is still running" >&2
  exit 3
fi
if [ ! -f "$ARCHIVE/tools/pg-builder/src/lib.rs" ]; then
  echo "focused-6 archive is missing" >&2
  exit 2
fi
mkdir -p "$HOST/cargo" "$HOST/target" "$HOST/meta"
cp "$EVIDENCE/linux/focused-6/locks/pg-builder-Cargo.lock" "$ARCHIVE/tools/pg-builder/Cargo.lock"
# Docker Desktop can miss a freshly created /tmp directory. Probe with the pinned
# Postgres image and no server, the same check focused-6/launch.sh uses.
visible=0
attempt=0
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if docker run --rm --pull never --network none --entrypoint bash \
    --mount "type=bind,source=${HOST}/cargo,target=/out" \
    "$PG_IMAGE" -c 'test -d /out' >/dev/null 2>&1; then
    visible=1
    break
  fi
  sleep 1
done
printf 'visible=%s attempts=%s\n' "$visible" "$attempt" > "$HOST/meta/cargo-visible.txt"
if [ "$visible" != 1 ]; then
  echo "host cargo directory is not visible to docker" >&2
  printf '%s\n' 125 > "$HOST/meta/build.exit"
  exit 125
fi
docker run --rm --pull never --network none --entrypoint bash \
  --mount "type=volume,source=oo-a328-eef831aa-cargo,target=/cargo,readonly" \
  --mount "type=bind,source=${HOST}/cargo,target=/out" \
  "$PG_IMAGE" \
  -c 'mkdir -p /out && cp -a /cargo/registry /out/registry && if [ -d /cargo/git ]; then cp -a /cargo/git /out/git; fi'
export CARGO_HOME="$HOST/cargo"
export CARGO_TARGET_DIR="$HOST/target"
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
cd "$ARCHIVE"
rustc -vV > "$HOST/meta/rustc-version.txt"
cargo build --manifest-path tools/pg-builder/Cargo.toml --locked --offline --bin pg-builder \
  > "$HOST/meta/build.log" 2>&1
code=$?
printf '%s\n' "$code" > "$HOST/meta/build.exit"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi
shasum -a 256 "$HOST/target/debug/pg-builder" > "$HOST/meta/pg-builder.sha256"
exit 0
