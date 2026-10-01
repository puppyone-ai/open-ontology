set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
echo "=== identity ==="
rustc -Vv
rustup show
uname -a
cat /etc/os-release
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "clippy_component_exit=$clippy_component_exit"
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --locked
fetch_exit=$?
set -e
echo "fetch_exit=$fetch_exit"
echo "=== cargo test --locked --offline -p oo-builder --all-targets ==="
set +e
cargo test --locked --offline -p oo-builder --all-targets -- --test-threads=8
test_exit=$?
set -e
echo "test_exit=$test_exit"
echo "=== cargo clippy --locked --offline -p oo-builder --all-targets ==="
set +e
cargo clippy --locked --offline -p oo-builder --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
echo "=== done ==="
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
