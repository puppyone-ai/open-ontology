set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
mkdir -p /target/phases
echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
lib_exit=$?
set -e
echo "lib_exit=$lib_exit"
printf '%s\n' "$lib_exit" > /target/phases/test-lib.exit
echo "=== cargo test --doc ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --doc
doc_exit=$?
set -e
echo "doc_exit=$doc_exit"
printf '%s\n' "$doc_exit" > /target/phases/test-doc.exit
worst=0
for phase_exit in "$lib_exit" "$doc_exit"; do
  if [ "$phase_exit" -ne 0 ]; then worst=$phase_exit; fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/test-fix.exit
exit "$worst"
