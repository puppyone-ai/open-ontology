#!/bin/bash
# Generate, pin, test, and release-build the MariaDB driver inside the one admitted builder.
set -u
ROOT=/Users/redux/puppy-one/open-ontology
WORK=${ROOT}/testing/dl01-driver-preflight/mariadb
EVIDENCE=${ROOT}/evidence/0d8d559a-7e46-4859-838e-21ffccd62772/DL01/mariadb-driver-preflight
BUILD=${EVIDENCE}/build
NAME=oo-dl01w1-39-rust
mkdir -p "$BUILD"
if ! docker container inspect "$NAME" >/dev/null 2>&1; then
  bash "$WORK/scripts/rust-launch.sh"
  printf '%s\n' "$?" > "$BUILD/launch.exit"
  if [ "$(cat "$BUILD/launch.exit")" != "0" ]; then
    exit "$(cat "$BUILD/launch.exit")"
  fi
fi

exec_cargo() {
  docker exec \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo \
    -e CARGO_TARGET_DIR=/target \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_BUILD_JOBS=1 \
    -w /work \
    "$NAME" \
    "$@"
}

{
  echo "rustc=$(exec_cargo rustc --version)"
  echo "cargo=$(exec_cargo cargo --version)"
  echo "jobs=1"
} | tee "$BUILD/toolchain.txt"

if [ ! -f "$WORK/Cargo.lock" ]; then
  exec_cargo cargo generate-lockfile > "$BUILD/generate.stdout" 2> "$BUILD/generate.stderr"
  printf '%s\n' "$?" > "$BUILD/generate.exit"
  if [ "$(cat "$BUILD/generate.exit")" != "0" ]; then
    exit "$(cat "$BUILD/generate.exit")"
  fi
else
  printf '%s\n' "skipped" > "$BUILD/generate.exit"
fi

exec_cargo bash /work/scripts/pin-lock.sh
printf '%s\n' "$?" > "$BUILD/pin.exit"
if [ "$(cat "$BUILD/pin.exit")" != "0" ]; then
  exit "$(cat "$BUILD/pin.exit")"
fi

python3 "$WORK/scripts/check_lock.py" "$ROOT/tools/mysql-preview/Cargo.lock" "$WORK/Cargo.lock" | tee "$BUILD/check-lock.txt"
printf '%s\n' "${PIPESTATUS[0]}" > "$BUILD/check-lock.exit"
if [ "$(cat "$BUILD/check-lock.exit")" != "0" ]; then
  exit "$(cat "$BUILD/check-lock.exit")"
fi

exec_cargo cargo fetch --locked > "$BUILD/fetch.stdout" 2> "$BUILD/fetch.stderr"
printf '%s\n' "$?" > "$BUILD/fetch.exit"
if [ "$(cat "$BUILD/fetch.exit")" != "0" ]; then
  exit "$(cat "$BUILD/fetch.exit")"
fi

exec_cargo cargo test --offline --locked --jobs 1 -- --test-threads=1 > "$BUILD/unit-test.stdout" 2> "$BUILD/unit-test.stderr"
printf '%s\n' "$?" > "$BUILD/unit-test.exit"
if [ "$(cat "$BUILD/unit-test.exit")" != "0" ]; then
  exit "$(cat "$BUILD/unit-test.exit")"
fi

exec_cargo cargo build --release --offline --locked --jobs 1 > "$BUILD/release.stdout" 2> "$BUILD/release.stderr"
printf '%s\n' "$?" > "$BUILD/release.exit"
exit "$(cat "$BUILD/release.exit")"
