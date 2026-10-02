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
uname -m
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "clippy_component_exit=$clippy_component_exit"
printf '%s\n' "$clippy_component_exit" > /target/phases/clippy-component.exit
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "fetch_exit=$fetch_exit"
printf '%s\n' "$fetch_exit" > /target/phases/fetch.exit
echo "=== cargo test --locked --offline -p oo-connectors ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors -- --test-threads=8
test_exit=$?
set -e
echo "test_exit=$test_exit"
printf '%s\n' "$test_exit" > /target/phases/test.exit
echo "=== cargo test --lib --locked --offline -p oo-connectors ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib -- --test-threads=8
lib_exit=$?
set -e
echo "lib_exit=$lib_exit"
printf '%s\n' "$lib_exit" > /target/phases/lib.exit
echo "=== cargo clippy --locked --offline -p oo-connectors --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "live_fixture=not-run"
echo "live_fixture_reason=OO_PG_FIXTURE and OO_PG_TYPES_FIXTURE are absent; package live tests are not green"
printf '%s\n' "not-run" > /target/phases/live-fixture.status
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$lib_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
