#!/bin/bash
# Post-edit focused suite. Fixed archive plus the owned completion overlay.
# No lock rewrite. file_hosting.rs and the delete contract are expected to differ.
set -eu
echo "oo_task=sp05-filehost-retention-complete-postedit4"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "postgres=none"
echo "overlay=none"
echo "lock_policy=prebuilt child lock must stay a checksum subset; no rewrite"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file must stay aside so 1.94.0 is not replaced" >&2
  exit 2
fi
for path in \
  crates/oo-control/src/file_hosting.rs \
  crates/oo-control/src/lib.rs \
  crates/oo-control/Cargo.toml \
  Cargo.lock \
  tools/artifact-retention/Cargo.toml \
  tools/artifact-retention/Cargo.lock \
  tools/artifact-retention/src/lib.rs \
  tools/artifact-retention/src/reaper.rs \
  tools/artifact-retention/tests/live.rs \
  tools/artifact-retention/CONTRACT.md \
  tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md \
  tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md
do
  if [ ! -f "$path" ]; then
    echo "missing $path" >&2
    exit 2
  fi
done
if [ -f tools/artifact-retention/tests/review_independent.rs ]; then
  echo "preoverlay snap contains the review test" >&2
  exit 4
fi
cp Cargo.lock /tmp/Cargo.lock.before
cp crates/oo-control/src/lib.rs /tmp/lib.rs.before
cp crates/oo-control/Cargo.toml /tmp/oo-control-cargo.toml.before
cp tools/artifact-retention/Cargo.lock /tmp/child-lock.before
cp crates/oo-builder/src/lib.rs /tmp/foreign-builder-lib.before
mkdir -p /evidence/phases

ensure_component() {
  local component="$1"
  local phase="$2"
  if rustup component list --toolchain 1.94.0 --installed | grep -q "^${component}"; then
    echo "${component}_component=already_installed"
    printf '%s\n' 0 > "/evidence/phases/${phase}.exit"
    return 0
  fi
  echo "=== rustup component add ${component} --toolchain 1.94.0 ==="
  set +e
  rustup component add "$component" --toolchain 1.94.0
  local add_exit=$?
  set -e
  echo "phase_exit=${add_exit} phase=rustup_component_add_${component}"
  printf '%s\n' "$add_exit" > "/evidence/phases/${phase}.exit"
  if [ "$add_exit" -ne 0 ]; then
    exit "$add_exit"
  fi
}

ensure_component rustfmt rustfmt-component
ensure_component clippy clippy-component

cd /src/tools/artifact-retention
echo "=== rustc lock_from_baseline ==="
set +e
rustc --edition 2021 -D warnings -o /target/lock_from_baseline /evidence/lock_from_baseline.rs
rustc_exit=$?
set -e
echo "phase_exit=$rustc_exit phase=rustc_lock_from_baseline"
printf '%s\n' "$rustc_exit" > /evidence/phases/rustc-emitter.exit
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi

echo "=== check prebuilt child lock ==="
set +e
/target/lock_from_baseline --check /src/Cargo.lock Cargo.lock
pre_check=$?
set -e
echo "phase_exit=$pre_check phase=check_prebuilt"
printf '%s\n' "$pre_check" > /evidence/phases/check-prebuilt.exit
if [ "$pre_check" -ne 0 ]; then
  exit "$pre_check"
fi
printf '%s\n' prebuilt-locked > /evidence/lock-method.txt
echo "lock_method=prebuilt-locked"

cd /src
echo "=== cargo fetch root --locked ==="
set +e
cargo fetch --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_root"
printf '%s\n' "$fetch_exit" > /evidence/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo fetch child --locked ==="
set +e
cargo fetch --manifest-path tools/artifact-retention/Cargo.toml --locked
child_fetch_exit=$?
set -e
echo "phase_exit=$child_fetch_exit phase=cargo_fetch_child"
printf '%s\n' "$child_fetch_exit" > /evidence/phases/fetch-child.exit
if [ "$child_fetch_exit" -ne 0 ]; then
  exit "$child_fetch_exit"
fi

cd /src/tools/artifact-retention
echo "=== confirm locked metadata ==="
cp Cargo.lock /tmp/Cargo.lock.child.before
set +e
cargo metadata --manifest-path Cargo.toml --locked --offline --format-version 1 > /evidence/metadata-final.json
final_meta_exit=$?
set -e
echo "phase_exit=$final_meta_exit phase=cargo_metadata_final_locked"
printf '%s\n' "$final_meta_exit" > /evidence/phases/metadata-final.exit
if [ "$final_meta_exit" -ne 0 ]; then
  exit "$final_meta_exit"
fi
if ! cmp -s /tmp/Cargo.lock.child.before Cargo.lock; then
  echo "final locked metadata rewrote the child lock" >&2
  printf '%s\n' 4 > /evidence/phases/child-lock-stable.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/child-lock-stable.exit

cd /src
owned_rust() {
  find crates/oo-control/src/file_hosting.rs tools/artifact-retention -name '*.rs' -type f | sort
}
echo "=== rustfmt owned sources --check ==="
set +e
owned_rust | xargs rustfmt --edition 2024 --check
fmt_check=$?
set -e
echo "phase_exit=$fmt_check phase=cargo_fmt_check"
printf '%s\n' "$fmt_check" > /evidence/phases/fmt-check.exit
if [ "$fmt_check" -ne 0 ]; then
  exit "$fmt_check"
fi

echo "=== cargo test file_hosting ==="
set +e
cargo test --locked --offline -p oo-control --test file_hosting -- --nocapture --test-threads=1
host_test_exit=$?
set -e
echo "phase_exit=$host_test_exit phase=file_hosting_test"
printf '%s\n' "$host_test_exit" > /evidence/phases/file-hosting-test.exit
if [ "$host_test_exit" -ne 0 ]; then
  exit "$host_test_exit"
fi

echo "=== cargo test oo-control lib (delete_without_lock and retention_completion) ==="
set +e
cargo test --locked --offline -p oo-control --lib -- --nocapture --test-threads=1
lib_test_exit=$?
set -e
echo "phase_exit=$lib_test_exit phase=oo_control_lib"
printf '%s\n' "$lib_test_exit" > /evidence/phases/oo-control-lib.exit
if [ "$lib_test_exit" -ne 0 ]; then
  exit "$lib_test_exit"
fi

echo "=== cargo clippy oo-control ==="
set +e
cargo clippy --locked --offline -p oo-control --all-targets -- -D warnings
host_clippy_exit=$?
set -e
echo "phase_exit=$host_clippy_exit phase=oo_control_clippy"
printf '%s\n' "$host_clippy_exit" > /evidence/phases/oo-control-clippy.exit
if [ "$host_clippy_exit" -ne 0 ]; then
  exit "$host_clippy_exit"
fi

cd /src/tools/artifact-retention
echo "=== cargo test artifact-retention ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --nocapture --test-threads=1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=artifact_retention_test"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo clippy artifact-retention ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=artifact_retention_clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

cd /src
if ! cmp -s /tmp/Cargo.lock.before Cargo.lock; then
  echo "root Cargo.lock changed" >&2
  printf '%s\n' 4 > /evidence/phases/root-lock-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/root-lock-unchanged.exit
if ! cmp -s /tmp/lib.rs.before crates/oo-control/src/lib.rs; then
  echo "lib.rs changed" >&2
  printf '%s\n' 4 > /evidence/phases/lib-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/lib-unchanged.exit
if ! cmp -s /tmp/foreign-builder-lib.before crates/oo-builder/src/lib.rs; then
  echo "foreign oo-builder lib.rs changed" >&2
  printf '%s\n' 4 > /evidence/phases/foreign-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/foreign-unchanged.exit
if ! cmp -s /tmp/oo-control-cargo.toml.before crates/oo-control/Cargo.toml; then
  echo "oo-control Cargo.toml changed" >&2
  printf '%s\n' 4 > /evidence/phases/control-manifest-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/control-manifest-unchanged.exit
if ! cmp -s /tmp/child-lock.before tools/artifact-retention/Cargo.lock; then
  echo "child Cargo.lock changed" >&2
  printf '%s\n' 4 > /evidence/phases/child-lock-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/child-lock-unchanged.exit
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "postedit=pass"
