set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
mkdir -p /target/phases
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "clippy_component_exit=$clippy_component_exit"
printf '%s\n' "$clippy_component_exit" > /target/phases/clippy-component.exit
echo "=== clippy oo-connectors ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "=== sdk test ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk --lib --tests
sdk_exit=$?
set -e
echo "sdk_exit=$sdk_exit"
printf '%s\n' "$sdk_exit" > /target/phases/sdk-test.exit
echo "=== sqlx-macros tree ==="
set +e
cargo tree --manifest-path Cargo.toml --locked --offline -p oo-connectors -i sqlx-macros > /target/phases/tree-sqlx-macros.txt
macros_exit=$?
set -e
echo "macros_exit=$macros_exit"
printf '%s\n' "$macros_exit" > /target/phases/tree-sqlx-macros.exit
echo "=== live pg ==="
set +e
OO_PG_FIXTURE=/secrets/pointer.json cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test live_pg -- --nocapture
live_exit=$?
set -e
echo "live_exit=$live_exit"
printf '%s\n' "$live_exit" > /target/phases/live.exit
worst=0
for phase_exit in "$clippy_component_exit" "$clippy_exit" "$sdk_exit" "$live_exit"; do
  if [ "$phase_exit" -ne 0 ]; then worst=$phase_exit; fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/live-worst.exit
exit "$worst"
