#!/bin/bash
# Fixed driver. Same init_mut unit invocation as red-init, then the finite peer
# and the unchanged driver regressions. Workspace tests stay --locked --offline.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /evidence
echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"

if grep -q 'incoming view shrank' src/net/socket/buffered.rs 2>/dev/null \
  || grep -q 'incoming view shrank' crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs; then
  echo "fixed source still contains the panic expect" >&2
  printf '%s\n' 4 > /evidence/unit.exit
  exit 4
fi

echo "=== init_mut unit outside workspace ==="
rm -rf /tmp/sqlx-core-pkg
mkdir -p /tmp/sqlx-core-pkg
cp -a /src/crates/oo-connectors/vendor/sqlx-core/. /tmp/sqlx-core-pkg/
awk '
  BEGIN { pinned = 0 }
  {
    if ($0 == "[dependencies.bytes]") { grab = 1; print; next }
    if (grab == 1 && $0 == "version = \"1.2.0\"") {
      print "version = \"=1.12.1\""
      grab = 0
      pinned = 1
      next
    }
    print
  }
  END { if (pinned != 1) exit 2 }
' /tmp/sqlx-core-pkg/Cargo.toml > /tmp/sqlx-core-pkg/Cargo.toml.pinned
pin_exit=$?
echo "pin_exit=$pin_exit"
printf '%s\n' "$pin_exit" > /evidence/pin.exit
if [ "$pin_exit" -ne 0 ]; then
  printf '%s\n' "$pin_exit" > /evidence/unit.exit
  exit "$pin_exit"
fi
mv /tmp/sqlx-core-pkg/Cargo.toml.pinned /tmp/sqlx-core-pkg/Cargo.toml
grep -n 'dependencies.bytes' -A 2 /tmp/sqlx-core-pkg/Cargo.toml
if grep -q 'incoming view shrank' /tmp/sqlx-core-pkg/src/net/socket/buffered.rs; then
  echo "copied unit source still panics" >&2
  printf '%s\n' 4 > /evidence/unit.exit
  exit 4
fi
set +e
cargo test --manifest-path /tmp/sqlx-core-pkg/Cargo.toml --target-dir /target/sqlx-core-init --lib init_mut_small_frames_shrink_the_receive_view -- --test-threads=1 --nocapture
unit_exit=$?
set -e
echo "unit_exit=$unit_exit"
printf '%s\n' "$unit_exit" > /evidence/unit.exit
grep -n 'name = "bytes"' -A 2 /tmp/sqlx-core-pkg/Cargo.lock | head -20 > /evidence/bytes-lock.txt || true

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@"
  local phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/${name}.exit"
}

run_phase tcp cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_view_shrink -- --test-threads=1 --nocapture
tcp_exit=$(cat /evidence/tcp.exit)
run_phase lib cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$(cat /evidence/lib.exit)
run_phase bounds cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_bounds -- --test-threads=1
bounds_exit=$(cat /evidence/bounds.exit)
run_phase accounting cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_accounting -- --test-threads=1
accounting_exit=$(cat /evidence/accounting.exit)
run_phase portal cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_portal_bounds -- --test-threads=1
portal_exit=$(cat /evidence/portal.exit)

echo "=== clippy component ==="
set +e
rustup component list --toolchain 1.94.0 --installed
installed_exit=$?
set -e
echo "component_list_exit=$installed_exit"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0
  add_exit=$?
  set -e
  echo "clippy_add_exit=$add_exit"
else
  echo "clippy_add_exit=already_installed"
  add_exit=0
fi
printf '%s\n' "$add_exit" > /evidence/clippy-add.exit

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit

worst=0
for phase_exit in "$pin_exit" "$unit_exit" "$tcp_exit" "$lib_exit" "$bounds_exit" "$accounting_exit" "$portal_exit" "$add_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/worst.exit
exit "$worst"
