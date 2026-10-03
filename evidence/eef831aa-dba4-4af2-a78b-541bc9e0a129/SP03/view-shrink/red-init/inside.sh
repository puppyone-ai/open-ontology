#!/bin/bash
# Run the init_mut unit test outside the workspace so sqlx-core dev-deps are allowed.
# The copied manifest pins bytes to the workspace lock (=1.12.1). Source is the snapshot.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/sqlx-core-init
cd /src
mkdir -p /evidence
echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
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
if [ "$pin_exit" -ne 0 ]; then
  printf '%s\n' "$pin_exit" > /evidence/unit.exit
  exit "$pin_exit"
fi
mv /tmp/sqlx-core-pkg/Cargo.toml.pinned /tmp/sqlx-core-pkg/Cargo.toml
grep -n 'dependencies.bytes' -A 2 /tmp/sqlx-core-pkg/Cargo.toml
grep -n 'incoming view shrank during advance' /tmp/sqlx-core-pkg/src/net/socket/buffered.rs
echo "=== cargo test init_mut outside workspace ==="
set +e
cargo test --manifest-path /tmp/sqlx-core-pkg/Cargo.toml --lib init_mut_small_frames_shrink_the_receive_view -- --test-threads=1 --nocapture
unit_exit=$?
set -e
echo "unit_exit=$unit_exit"
printf '%s\n' "$unit_exit" > /evidence/unit.exit
# Show which bytes crate was linked.
grep -n 'name = "bytes"' -A 2 /tmp/sqlx-core-pkg/Cargo.lock | head -20 > /evidence/bytes-lock.txt || true
exit "$unit_exit"
