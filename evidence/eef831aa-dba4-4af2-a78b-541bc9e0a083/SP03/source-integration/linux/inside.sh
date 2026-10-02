set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
mkdir -p /target/phases
cp Cargo.lock /target/phases/Cargo.lock.before
echo "=== identity ==="
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
echo "=== fetch unlocked ==="
set +e
cargo fetch --manifest-path Cargo.toml
fetch_exit=$?
set -e
echo "fetch_exit=$fetch_exit"
printf '%s\n' "$fetch_exit" > /target/phases/fetch.exit
cp Cargo.lock /target/phases/Cargo.lock.after || true
echo "=== test lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
lib_exit=$?
set -e
echo "lib_exit=$lib_exit"
printf '%s\n' "$lib_exit" > /target/phases/test-lib.exit
echo "=== test doc ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --doc
doc_exit=$?
set -e
echo "doc_exit=$doc_exit"
printf '%s\n' "$doc_exit" > /target/phases/test-doc.exit
echo "=== clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "=== feature tree ==="
set +e
cargo tree --manifest-path Cargo.toml --locked --offline -e features -i sqlx-core > /target/phases/tree-sqlx-core.txt
tree_core_exit=$?
cargo tree --manifest-path Cargo.toml --locked --offline -i webpki-roots > /target/phases/tree-webpki-roots.txt
tree_webpki_exit=$?
cargo tree --manifest-path Cargo.toml --locked --offline -i rustls-native-certs > /target/phases/tree-rustls-native-certs.txt
tree_native_exit=$?
cargo tree --manifest-path Cargo.toml --locked --offline -p oo-connectors -i webpki-roots > /target/phases/tree-connectors-webpki.txt
tree_conn_webpki_exit=$?
cargo tree --manifest-path Cargo.toml --locked --offline -p oo-connectors -e features -i sqlx-core > /target/phases/tree-connectors-sqlx-core.txt
tree_conn_core_exit=$?
set -e
echo "tree_core_exit=$tree_core_exit tree_webpki_exit=$tree_webpki_exit tree_native_exit=$tree_native_exit tree_conn_webpki_exit=$tree_conn_webpki_exit tree_conn_core_exit=$tree_conn_core_exit"
printf '%s\n' "$tree_core_exit" > /target/phases/tree-core.exit
printf '%s\n' "$tree_webpki_exit" > /target/phases/tree-webpki.exit
printf '%s\n' "$tree_native_exit" > /target/phases/tree-native.exit
printf '%s\n' "$tree_conn_webpki_exit" > /target/phases/tree-conn-webpki.exit
forbid=0
if grep -E 'feature "(tls-rustls-ring-native-roots|tls-rustls-ring-webpki|tls-rustls-aws-lc-rs|tls-rustls-ring|tls-rustls|tls-native-tls|_tls-rustls-ring-native-roots|_tls-rustls-ring-webpki|_tls-rustls-aws-lc-rs|_tls-native-tls|webpki-roots|rustls-native-certs|native-tls)"' /target/phases/tree-sqlx-core.txt; then
  forbid=1
fi
if grep -E '(^| )webpki-roots v|(^| )rustls-native-certs v' /target/phases/tree-sqlx-core.txt; then
  forbid=1
fi
echo "forbid_exit=$forbid"
printf '%s\n' "$forbid" > /target/phases/forbid.exit
echo "=== negative union probe ==="
mkdir -p /tmp/guard-union/src /target/guard-union
cat > /tmp/guard-union/Cargo.toml << 'TOML'
[package]
name = "oo-a083-guard-union"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
sqlx-core = { path = "/src/crates/oo-connectors/vendor/sqlx-core", default-features = false, features = ["_tls-rustls-ring-no-roots", "_tls-rustls-ring-webpki"] }
TOML
echo 'fn main() {}' > /tmp/guard-union/src/main.rs
set +e
CARGO_TARGET_DIR=/target/guard-union cargo check --manifest-path /tmp/guard-union/Cargo.toml --offline
union_exit=$?
if [ "$union_exit" -ne 0 ]; then
  CARGO_TARGET_DIR=/target/guard-union cargo check --manifest-path /tmp/guard-union/Cargo.toml
  union_exit=$?
fi
set -e
echo "union_exit=$union_exit"
printf '%s\n' "$union_exit" > /target/phases/guard-union.exit
echo "=== negative facade probe ==="
mkdir -p /tmp/guard-facade/src /target/guard-facade
cat > /tmp/guard-facade/Cargo.toml << 'TOML'
[package]
name = "oo-a083-guard-facade"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
sqlx = { version = "=0.9.0", default-features = false, features = ["runtime-tokio", "postgres", "tls-rustls"] }

[patch.crates-io]
sqlx-core = { path = "/src/crates/oo-connectors/vendor/sqlx-core" }
sqlx-postgres = { path = "/src/crates/oo-connectors/vendor/sqlx-postgres" }
TOML
echo 'fn main() {}' > /tmp/guard-facade/src/main.rs
set +e
CARGO_TARGET_DIR=/target/guard-facade cargo check --manifest-path /tmp/guard-facade/Cargo.toml
facade_exit=$?
set -e
echo "facade_exit=$facade_exit"
printf '%s\n' "$facade_exit" > /target/phases/guard-facade.exit
echo "=== sdk test ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk --lib --tests
sdk_exit=$?
set -e
echo "sdk_exit=$sdk_exit"
printf '%s\n' "$sdk_exit" > /target/phases/sdk-test.exit
echo "=== done ==="
worst=0
for phase_exit in "$fetch_exit" "$lib_exit" "$doc_exit" "$clippy_exit" "$forbid" "$sdk_exit"; do
  if [ "$phase_exit" -ne 0 ]; then worst=$phase_exit; fi
done
# The negative probes must fail the compiler. Exit 0 from either probe is a failed guard.
if [ "$union_exit" -eq 0 ] || [ "$facade_exit" -eq 0 ]; then
  echo "guard labeled pass; that is a failure"
  worst=1
fi
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
