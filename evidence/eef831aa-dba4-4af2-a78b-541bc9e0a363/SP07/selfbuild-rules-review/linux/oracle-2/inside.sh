#!/bin/bash
# Oracle-only retry for the low-card foreign-key bridge.
# Does not rerun toolchain install, fetch, tests, Clippy, lock subset, or the sealed CLI.
# Origin posture: evidence/.../a363/.../linux/proof/inside.sh
set -u
echo "oo_task=sp07-selfbuild-rules-review-oracle-2"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a363"
echo "oo_fixed=e5427c1a18e2ddbb480791d214c76888eb7ca998"
echo "oo_platform=linux/arm64"
echo "phase=oracle-2"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/oracle-2/phases /cargo-home /target
cd /work

record() {
  printf '%s\n' "$2" > "/evidence/linux/oracle-2/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/oracle-2/oracle.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/oracle-2/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

printf '%s\n' 'not-rerun; proof phases clippy-component.exit 0 and clippy.exit 0 already recorded' \
  > /evidence/linux/oracle-2/clippy-not-rerun.txt

shopt -s nullglob
libs=(/target/debug/deps/libselfbuild_rules-*.rlib)
sjs=(/target/debug/deps/libserde_json-*.rlib)
printf '%s\n' "${libs[@]}" > /evidence/linux/oracle-2/rlib-selfbuild.txt
printf '%s\n' "${sjs[@]}" > /evidence/linux/oracle-2/rlib-serde-json.txt
if [ "${#libs[@]}" -ne 1 ] || [ "${#sjs[@]}" -ne 1 ]; then
  echo "rlib count self=${#libs[@]} serde_json=${#sjs[@]}" >&2
  record rlib 1
fi
record rlib 0
lib=${libs[0]}
sj=${sjs[0]}

rustc --edition 2024 -C debuginfo=0 -L dependency=/target/debug/deps \
  --extern selfbuild_rules="$lib" \
  --extern serde_json="$sj" \
  -o /target/a363-oracle2 /evidence/oracle/main.rs > /evidence/linux/oracle-2/oracle-build.log 2>&1
record oracle-build $?

/target/a363-oracle2 > /evidence/linux/oracle-2/oracle.log 2> /evidence/linux/oracle-2/oracle.stderr
record oracle $?

if cmp -s <(printf '%s\n' "$(cat /evidence/linux/oracle-2/oracle-shop.json)") /evidence/linux/proof/shop-review.json; then
  record cli-oracle-bytes 0
else
  record cli-oracle-bytes 1
fi

printf '%s\n' 0 > /evidence/linux/oracle-2/oracle.exit
echo oracle_done
