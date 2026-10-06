#!/bin/bash
# Independent review build of business-calendar. Fresh target directory.
# Same fixed archive and the lock that impl-4 generated. No cargo update.
# No PostgreSQL. Does not regenerate the child lock.
# Origin: evidence/.../a270/SP08/enum-history/linux/review-1/inside.sh
set -u
echo "oo_task=sp08-business-calendar-review-1"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a269"
echo "oo_fixed=0bcd494457091d4b55c82fc6b9922b5bc5dc44c4"
echo "oo_platform=linux/arm64"
echo "phase=review-1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/business-calendar-review
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) record version 0 ;;
  *) echo "refusing toolchain: $got" >&2; record version 2 ;;
esac

rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?

if [ -e tools/business-calendar ]; then
  echo "business-calendar is already on the fixed archive" >&2
  record absence 2
fi
printf '%s\n' "business-calendar absent" > /evidence/absence.txt
record absence 0

example_hash=$(sha256sum examples/schema/calendar.yaml | awk '{print $1}')
if [ "$example_hash" != "2ad3845ec287c24391168ce1e7271d743202b0b2d5cef7d6ad05537e02fcc378" ]; then
  echo "calendar example hash drifted: $example_hash" >&2
  record example-hash 2
fi
record example-hash 0
sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-before.sha256

rm -rf tools/business-calendar
mkdir -p tools/business-calendar
cp -R /tool/. tools/business-calendar/
find tools/business-calendar -name '._*' -delete
find tools/business-calendar -name '.DS_Store' -delete
rm -rf tools/business-calendar/target
if [ ! -f tools/business-calendar/Cargo.lock ]; then
  echo "review overlay has no Cargo.lock" >&2
  record overlay 2
fi
if [ ! -f tools/business-calendar/src/lib.rs ]; then
  echo "review overlay is missing src/lib.rs" >&2
  record overlay 2
fi
find tools/business-calendar -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
record overlay 0

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/business-calendar/Cargo.lock \
  > /evidence/lock-subset.txt 2> /evidence/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"

echo "=== cargo fetch business-calendar locked offline ==="
set +e
cargo fetch --manifest-path tools/business-calendar/Cargo.toml --locked --offline \
  > /evidence/fetch-child.stdout 2> /evidence/fetch-child.stderr
fetch_child=$?
set -e
record fetch-child "$fetch_child"

echo "=== business-calendar tests ==="
set +e
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
record test "$test_exit"

echo "=== rustup component add clippy ==="
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  printf '%s\n' "already-installed" > /evidence/clippy-component.stdout
  record clippy-component 0
else
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
fi

echo "=== cargo clippy business-calendar ==="
set +e
cargo clippy --manifest-path tools/business-calendar/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
record clippy "$clippy_exit"

sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(cat /evidence/lock-before.sha256)
after=$(cat /evidence/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed before=$before after=$after" > /evidence/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
record lock-hash 0
sha256sum tools/business-calendar/Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
printf '%s\n' 0 > /evidence/smoke.exit
echo "review=ok"
exit 0
