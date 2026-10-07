#!/bin/bash
# Independent review tests overlaid on the fixed archive. Product sources stay unchanged.
set -eu
echo "oo_task=sp05-filehost-retention-complete-review"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "postgres=none"
echo "overlay=tools/artifact-retention/tests/review_independent.rs"
echo "lock_policy=prebuilt child lock must stay a checksum subset; no rewrite"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
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
if [ ! -f tools/artifact-retention/tests/review_independent.rs ]; then
  echo "review test missing from snap" >&2
  exit 4
fi
cp Cargo.lock /tmp/Cargo.lock.before
cp crates/oo-control/src/lib.rs /tmp/lib.rs.before
cp crates/oo-control/Cargo.toml /tmp/oo-control-cargo.toml.before
cp crates/oo-control/src/file_hosting.rs /tmp/file_hosting.rs.before
cp tools/artifact-retention/Cargo.lock /tmp/child-lock.before
cp tools/artifact-retention/src/lib.rs /tmp/retention-lib.rs.before
cp tools/artifact-retention/src/reaper.rs /tmp/reaper.rs.before
cp tools/artifact-retention/tests/live.rs /tmp/live.rs.before
cp tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md /tmp/delete-contract.before
cp tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md /tmp/addendum.before
cp tools/artifact-retention/CONTRACT.md /tmp/contract.before
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

echo "=== rustfmt review test --check ==="
set +e
rustfmt --edition 2024 --check tests/review_independent.rs
fmt_check=$?
set -e
echo "phase_exit=$fmt_check phase=cargo_fmt_check"
printf '%s\n' "$fmt_check" > /evidence/phases/fmt-check.exit
if [ "$fmt_check" -ne 0 ]; then
  exit "$fmt_check"
fi

echo "=== cargo test review_independent ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --test review_independent -- --nocapture --test-threads=1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=review_independent"
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
unchanged() {
  local before="$1"
  local after="$2"
  local phase="$3"
  if ! cmp -s "$before" "$after"; then
    echo "$after changed" >&2
    printf '%s\n' 4 > "/evidence/phases/${phase}.exit"
    exit 4
  fi
  printf '%s\n' 0 > "/evidence/phases/${phase}.exit"
}
unchanged /tmp/Cargo.lock.before Cargo.lock root-lock-unchanged
unchanged /tmp/lib.rs.before crates/oo-control/src/lib.rs lib-unchanged
unchanged /tmp/file_hosting.rs.before crates/oo-control/src/file_hosting.rs filehost-unchanged
unchanged /tmp/foreign-builder-lib.before crates/oo-builder/src/lib.rs foreign-unchanged
unchanged /tmp/oo-control-cargo.toml.before crates/oo-control/Cargo.toml control-manifest-unchanged
unchanged /tmp/child-lock.before tools/artifact-retention/Cargo.lock child-lock-unchanged
unchanged /tmp/retention-lib.rs.before tools/artifact-retention/src/lib.rs retention-lib-unchanged
unchanged /tmp/reaper.rs.before tools/artifact-retention/src/reaper.rs reaper-unchanged
unchanged /tmp/live.rs.before tools/artifact-retention/tests/live.rs author-live-unchanged
unchanged /tmp/delete-contract.before tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md delete-contract-unchanged
unchanged /tmp/addendum.before tools/artifact-retention/FILEHOST-COMPLETION-ADDENDUM.md addendum-unchanged
unchanged /tmp/contract.before tools/artifact-retention/CONTRACT.md contract-unchanged
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "review=pass"
