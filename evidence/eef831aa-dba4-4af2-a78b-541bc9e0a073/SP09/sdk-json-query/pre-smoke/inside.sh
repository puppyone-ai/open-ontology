set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
mkdir -p /target/phases
echo "=== identity ==="
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "clippy_component_exit=$clippy_component_exit"
printf '%s\n' "$clippy_component_exit" > /target/phases/clippy-component.exit
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "fetch_exit=$fetch_exit"
printf '%s\n' "$fetch_exit" > /target/phases/fetch.exit
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk
test_exit=$?
set -e
echo "test_exit=$test_exit"
printf '%s\n' "$test_exit" > /target/phases/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "=== done ==="
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
