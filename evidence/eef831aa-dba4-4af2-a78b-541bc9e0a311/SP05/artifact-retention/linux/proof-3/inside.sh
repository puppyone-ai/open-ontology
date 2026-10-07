#!/bin/bash
# Proof of tools/artifact-retention on the fixed archive plus this overlay.
# One offline metadata rewrite is allowed only after --locked rejects the walked lock.
# No cargo update, no cargo generate-lockfile, no root lock rewrite, no network fetch.
set -eu
echo "oo_task=sp05-artifact-retention-proof-3"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "postgres=none"
echo "overlay=tools/artifact-retention"
echo "lock_policy=walk fixed lock; one cargo metadata --offline only if --locked fails"
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
  tools/artifact-retention/src/lib.rs \
  tools/artifact-retention/tests/live.rs \
  tools/artifact-retention/CONTRACT.md
do
  if [ ! -f "$path" ]; then
    echo "missing $path" >&2
    exit 2
  fi
done
if [ -f tools/artifact-retention/Cargo.lock ]; then
  echo "overlay_lock=present"
else
  echo "overlay_lock=absent"
fi
cp Cargo.lock /tmp/Cargo.lock.before
cp crates/oo-control/src/file_hosting.rs /tmp/file_hosting.rs.before
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

if [ -f Cargo.lock ]; then
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
  method=prebuilt-locked
  printf '%s\n' "$method" > /evidence/lock-method.txt
  echo "lock_method=$method"
else
echo "=== walk child lock ==="
set +e
/target/lock_from_baseline --walk /src/Cargo.lock Cargo.lock
walk_exit=$?
set -e
echo "phase_exit=$walk_exit phase=walk_lock"
printf '%s\n' "$walk_exit" > /evidence/phases/walk-lock.exit
if [ -f Cargo.lock ]; then
  cp Cargo.lock /evidence/Cargo.lock.walk
fi

method=unset
if [ "$walk_exit" -eq 0 ]; then
  echo "=== check walked lock ==="
  set +e
  /target/lock_from_baseline --check /src/Cargo.lock Cargo.lock
  walk_check_exit=$?
  set -e
  echo "phase_exit=$walk_check_exit phase=check_walk"
  printf '%s\n' "$walk_check_exit" > /evidence/phases/check-walk.exit
  if [ "$walk_check_exit" -eq 0 ]; then
    echo "=== cargo metadata --locked --offline ==="
    set +e
    cargo metadata --manifest-path Cargo.toml --locked --offline --format-version 1 > /evidence/metadata-locked.json
    locked_exit=$?
    set -e
    echo "phase_exit=$locked_exit phase=cargo_metadata_locked"
    printf '%s\n' "$locked_exit" > /evidence/phases/metadata-locked.exit
    if [ "$locked_exit" -eq 0 ]; then
      method=walk-locked
    fi
  fi
else
  printf '%s\n' 1 > /evidence/phases/check-walk.exit
  printf '%s\n' 1 > /evidence/phases/metadata-locked.exit
fi

if [ "$method" = unset ]; then
  echo "=== superset pins then one cargo metadata --offline ==="
  if [ -f Cargo.lock ]; then
    cp Cargo.lock /evidence/Cargo.lock.handbuilt
  elif [ -f /evidence/Cargo.lock.walk ]; then
    cp /evidence/Cargo.lock.walk /evidence/Cargo.lock.handbuilt
  fi
  set +e
  /target/lock_from_baseline --superset /src/Cargo.lock Cargo.lock
  super_exit=$?
  set -e
  echo "phase_exit=$super_exit phase=superset_lock"
  printf '%s\n' "$super_exit" > /evidence/phases/superset-lock.exit
  if [ "$super_exit" -ne 0 ]; then
    exit "$super_exit"
  fi
  cp Cargo.lock /evidence/Cargo.lock.superset
  echo "=== cargo metadata --offline (single edge rewrite) ==="
  set +e
  cargo metadata --manifest-path Cargo.toml --offline --format-version 1 > /evidence/metadata-offline.json
  meta_exit=$?
  set -e
  echo "phase_exit=$meta_exit phase=cargo_metadata_offline"
  printf '%s\n' "$meta_exit" > /evidence/phases/metadata-offline.exit
  if [ "$meta_exit" -ne 0 ]; then
    if [ -f /evidence/Cargo.lock.handbuilt ]; then
      cp /evidence/Cargo.lock.handbuilt Cargo.lock
    fi
    exit "$meta_exit"
  fi
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  echo "=== checksum gate ==="
  set +e
  /target/lock_from_baseline --check /src/Cargo.lock Cargo.lock
  gate_exit=$?
  set -e
  echo "phase_exit=$gate_exit phase=checksum_gate"
  printf '%s\n' "$gate_exit" > /evidence/phases/checksum-gate.exit
  if [ "$gate_exit" -ne 0 ]; then
    echo "checksum gate failed; restored hand-built lock" >&2
    if [ -f /evidence/Cargo.lock.handbuilt ]; then
      cp /evidence/Cargo.lock.handbuilt Cargo.lock
    fi
    exit "$gate_exit"
  fi
  method=metadata-offline
else
  echo "superset_rewrite=not_needed"
fi
printf '%s\n' "$method" > /evidence/lock-method.txt
echo "lock_method=$method"
fi

echo "=== confirm locked metadata on the final lock ==="
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
cp Cargo.lock /evidence/Cargo.lock.final

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "root Cargo.lock changed" >&2
  printf '%s\n' 4 > /evidence/phases/root-lock-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/root-lock-unchanged.exit
if ! cmp -s /tmp/file_hosting.rs.before /src/crates/oo-control/src/file_hosting.rs; then
  echo "file_hosting.rs changed" >&2
  printf '%s\n' 4 > /evidence/phases/filehost-unchanged.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/filehost-unchanged.exit

echo "=== cargo fmt -p artifact-retention --check ==="
set +e
cargo fmt -p artifact-retention -- --check
fmt_check_exit=$?
set -e
echo "phase_exit=$fmt_check_exit phase=cargo_fmt_check"
printf '%s\n' "$fmt_check_exit" > /evidence/phases/fmt-check.exit
if [ "$fmt_check_exit" -ne 0 ]; then
  echo "=== cargo fmt -p artifact-retention ==="
  set +e
  cargo fmt -p artifact-retention
  fmt_exit=$?
  set -e
  echo "phase_exit=$fmt_exit phase=cargo_fmt"
  printf '%s\n' "$fmt_exit" > /evidence/phases/fmt.exit
  if [ "$fmt_exit" -ne 0 ]; then
    exit "$fmt_exit"
  fi
else
  printf '%s\n' 0 > /evidence/phases/fmt.exit
fi

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --nocapture --test-threads=1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "root Cargo.lock changed after test" >&2
  printf '%s\n' 4 > /evidence/phases/root-lock-after.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/root-lock-after.exit
if ! cmp -s /tmp/file_hosting.rs.before /src/crates/oo-control/src/file_hosting.rs; then
  echo "file_hosting.rs changed after test" >&2
  printf '%s\n' 4 > /evidence/phases/filehost-after.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/phases/filehost-after.exit
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "proof=pass"
