#!/bin/bash
# Fixed 79515fcf archive, Rust 1.94. Smoke lib and scoped clippy first.
# Then bounds, accounting, portal, then the bounded receive-cap probe.
# Workspace tests stay --locked --offline. The sqlx-core unit copy is not a
# workspace member and is pinned to bytes 1.12.1. No PostgreSQL.
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

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
sha256sum \
  "$PRODUCT" \
  crates/oo-connectors/vendor/PATCH.md \
  crates/oo-connectors/tests/pg_wire_view_shrink.rs \
  > /evidence/product-source.sha256
echo "=== product hash ==="
cat /evidence/product-source.sha256

if grep -q 'incoming view shrank' "$PRODUCT"; then
  echo "archive still has the panic expect" >&2
  printf '%s\n' 4 > /evidence/lib.exit
  exit 4
fi
grep -n 'saturating_sub(view_before)' "$PRODUCT" | head -5 > /evidence/saturating-hits.txt

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

echo "=== fetch workspace ==="
run_phase fetch cargo fetch --locked --manifest-path Cargo.toml
fetch_exit=$(cat /evidence/fetch.exit)
if [ "$fetch_exit" -ne 0 ]; then
  echo "fetch failed; workspace offline tests will record that" >&2
fi

echo "=== SMOKE lib ==="
run_phase lib cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$(cat /evidence/lib.exit)

echo "=== SMOKE clippy ==="
set +e
rustup component list --toolchain 1.94.0 --installed
installed_exit=$?
set -e
echo "component_list_exit=$installed_exit"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  run_phase clippy-add rustup component add clippy --toolchain 1.94.0
  add_exit=$(cat /evidence/clippy-add.exit)
else
  echo "clippy_add_exit=already_installed"
  add_exit=0
  printf '%s\n' 0 > /evidence/clippy-add.exit
fi
run_phase clippy cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$(cat /evidence/clippy.exit)

echo "=== bounds accounting portal ==="
run_phase bounds cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_bounds -- --test-threads=1
bounds_exit=$(cat /evidence/bounds.exit)
run_phase accounting cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_accounting -- --test-threads=1
accounting_exit=$(cat /evidence/accounting.exit)
run_phase portal cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_portal_bounds -- --test-threads=1
portal_exit=$(cat /evidence/portal.exit)

echo "=== probe copy ==="
rm -rf /tmp/sqlx-core-pkg
mkdir -p /tmp/sqlx-core-pkg
cp -a /src/crates/oo-connectors/vendor/sqlx-core/. /tmp/sqlx-core-pkg/
sha256sum "$PRODUCT" > /evidence/buffered-archive.sha256
sha256sum /tmp/sqlx-core-pkg/src/net/socket/buffered.rs > /evidence/buffered-copy-before.sha256
if cmp -s "$PRODUCT" /tmp/sqlx-core-pkg/src/net/socket/buffered.rs; then
  echo "buffered_copy_before=same" | tee /evidence/buffered-copy-before.txt
else
  echo "buffered_copy_before=DIFFER" | tee /evidence/buffered-copy-before.txt
  printf '%s\n' 4 > /evidence/probe.exit
  exit 4
fi
# Record the package lock's bytes version before the workspace pin.
awk 'BEGIN{p=0} /^name = "bytes"$/{p=1} p&&/^version =/{print; exit}' /tmp/sqlx-core-pkg/Cargo.lock \
  > /evidence/bytes-lock-before.txt || true

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
  printf '%s\n' "$pin_exit" > /evidence/probe.exit
  exit "$pin_exit"
fi
mv /tmp/sqlx-core-pkg/Cargo.toml.pinned /tmp/sqlx-core-pkg/Cargo.toml
grep -n 'dependencies.bytes' -A 2 /tmp/sqlx-core-pkg/Cargo.toml > /evidence/bytes-pin-toml.txt
cat >> /tmp/sqlx-core-pkg/Cargo.toml <<'EOF'

[[test]]
name = "a132_receive_cap"
path = "tests/a132_receive_cap.rs"
EOF
mkdir -p /tmp/sqlx-core-pkg/tests
cp /evidence/a132_receive_cap.rs /tmp/sqlx-core-pkg/tests/a132_receive_cap.rs
if cmp -s "$PRODUCT" /tmp/sqlx-core-pkg/src/net/socket/buffered.rs; then
  echo "buffered_copy_after_pin=same" | tee /evidence/buffered-copy-after.txt
else
  echo "buffered_copy_after_pin=DIFFER" | tee /evidence/buffered-copy-after.txt
  printf '%s\n' 4 > /evidence/probe.exit
  exit 4
fi

echo "=== probe ==="
run_phase probe cargo test --manifest-path /tmp/sqlx-core-pkg/Cargo.toml --target-dir /target/sqlx-core-probe --test a132_receive_cap -- --test-threads=1 --nocapture
probe_exit=$(cat /evidence/probe.exit)
awk 'BEGIN{p=0} /^name = "bytes"$/{p=1} p&&/^version =/{print; exit} p&&/^$/{exit}' /tmp/sqlx-core-pkg/Cargo.lock \
  > /evidence/bytes-lock-after.txt || true
if cmp -s "$PRODUCT" /tmp/sqlx-core-pkg/src/net/socket/buffered.rs; then
  echo "buffered_copy_after_test=same" | tee -a /evidence/buffered-copy-after.txt
else
  echo "buffered_copy_after_test=DIFFER" | tee -a /evidence/buffered-copy-after.txt
fi

worst=0
for phase_exit in "$fetch_exit" "$lib_exit" "$add_exit" "$clippy_exit" "$bounds_exit" "$accounting_exit" "$portal_exit" "$pin_exit" "$probe_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/worst.exit
exit "$worst"
