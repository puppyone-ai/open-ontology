#!/bin/bash
# Continuation after rest exit 101. Does not repeat smoke, lib, bounds,
# accounting, portal, view, or the tail integration tests. sqlx-core is not a
# workspace member, so Cargo will not test it from the workspace root. This
# copies the snapshot crate to /tmp, pins bytes to the workspace 1.12.1 crate,
# discards the vendored lock (bytes 1.11.1), and tests that copy as its own
# package. The repo Cargo.toml and Cargo.lock are not modified.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
cd /src
mkdir -p /evidence/proof

echo "=== identity ==="
uname -m
rustc -Vv
cargo -V

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
if ! grep -q 'fn committed_init_len' "$PRODUCT"; then
  echo "tail correction missing from snapshot" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
if [ -f crates/oo-connectors/src/stream.rs ]; then
  echo "snapshot contains stream.rs" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
sha256sum "$PRODUCT" crates/oo-connectors/vendor/PATCH.md crates/oo-connectors/tests/pg_wire_tls_tail.rs > /evidence/proof/embed-product-source.sha256

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/proof/embed-${name}.log" 2>&1
  local phase_exit=$?
  set +e
  cat "/evidence/proof/embed-${name}.log"
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/proof/embed-${name}.exit"
  if [ "$phase_exit" -ne 0 ]; then
    printf '%s\n' "$phase_exit" > /evidence/proof/embed-worst.exit
    echo "worst_phase=$name"
    exit "$phase_exit"
  fi
}

ISO=/tmp/sqlx-core-iso
rm -rf "$ISO"
cp -a /src/crates/oo-connectors/vendor/sqlx-core "$ISO"
rm -f "$ISO/Cargo.lock"
if ! grep -q 'fn committed_init_len' "$ISO/src/net/socket/buffered.rs"; then
  echo "iso copy lost the tail correction" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi

# The full crate lists optional dependencies the production build never
# enables. Offline resolution still looks them up, and crates such as
# async-fs are not in the warm cache. Drop those tables, and the unused
# dev-dependency, from this copy only. bytes stays =1.12.1.
awk '
BEGIN { mode = ""; buf = ""; skip = 0; crate = "" }
function flush() {
  if (mode == "dep" && skip == 0) printf "%s", buf
  mode = ""; buf = ""; skip = 0; crate = ""
}
/^\[/ {
  flush()
  if ($0 == "[features]") {
    print "[features]"
    print "default = []"
    mode = "features"
    next
  }
  if ($0 ~ /^\[dev-dependencies/) {
    mode = "drop"
    next
  }
  if ($0 ~ /^\[dependencies\./) {
    mode = "dep"
    crate = $0
    sub(/^\[dependencies\./, "", crate)
    sub(/\]$/, "", crate)
    buf = $0 "\n"
    next
  }
  print
  next
}
mode == "features" || mode == "drop" { next }
mode == "dep" {
  if ($0 ~ /^optional = true/) skip = 1
  if ($0 ~ /^version = / && crate == "bytes") {
    buf = buf "version = \"=1.12.1\"\n"
  } else if ($0 ~ /^version = / && crate == "base64") {
    buf = buf "version = \"=0.22.1\"\n"
  } else if ($0 ~ /^version = / && crate == "cfg-if") {
    buf = buf "version = \"=1.0.5\"\n"
  } else if ($0 ~ /^version = / && crate == "crossbeam-queue") {
    buf = buf "version = \"=0.3.14\"\n"
  } else if ($0 ~ /^version = / && crate == "either") {
    buf = buf "version = \"=1.18.0\"\n"
  } else if ($0 ~ /^version = / && crate == "event-listener") {
    buf = buf "version = \"=5.4.2\"\n"
  } else if ($0 ~ /^version = / && crate == "futures-core") {
    buf = buf "version = \"=0.3.34\"\n"
  } else if ($0 ~ /^version = / && crate == "futures-intrusive") {
    buf = buf "version = \"=0.5.0\"\n"
  } else if ($0 ~ /^version = / && crate == "futures-io") {
    buf = buf "version = \"=0.3.34\"\n"
  } else if ($0 ~ /^version = / && crate == "futures-util") {
    buf = buf "version = \"=0.3.34\"\n"
  } else if ($0 ~ /^version = / && crate == "hashbrown") {
    buf = buf "version = \"=0.16.1\"\n"
  } else if ($0 ~ /^version = / && crate == "hashlink") {
    buf = buf "version = \"=0.11.1\"\n"
  } else if ($0 ~ /^version = / && crate == "indexmap") {
    buf = buf "version = \"=2.14.2\"\n"
  } else if ($0 ~ /^version = / && crate == "log") {
    buf = buf "version = \"=0.4.34\"\n"
  } else if ($0 ~ /^version = / && crate == "memchr") {
    buf = buf "version = \"=2.8.3\"\n"
  } else if ($0 ~ /^version = / && crate == "percent-encoding") {
    buf = buf "version = \"=2.3.2\"\n"
  } else if ($0 ~ /^version = / && crate == "smallvec") {
    buf = buf "version = \"=1.16.1\"\n"
  } else if ($0 ~ /^version = / && crate == "thiserror") {
    buf = buf "version = \"=2.0.21\"\n"
  } else if ($0 ~ /^version = / && crate == "tracing") {
    buf = buf "version = \"=0.1.44\"\n"
  } else if ($0 ~ /^version = / && crate == "url") {
    buf = buf "version = \"=2.5.8\"\n"
  } else {
    buf = buf $0 "\n"
  }
  next
}
{ print }
END { flush() }
' "$ISO/Cargo.toml" > "$ISO/Cargo.toml.pinned"
mv "$ISO/Cargo.toml.pinned" "$ISO/Cargo.toml"
if grep -q 'async-fs' "$ISO/Cargo.toml"; then
  echo "optional deps were not stripped" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
grep -n 'bytes' -A2 "$ISO/Cargo.toml" | head -20 > /evidence/proof/embed-bytes-pin.txt
if ! grep -q 'version = "=1.12.1"' "$ISO/Cargo.toml"; then
  echo "bytes pin missing" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi

run_phase sqlx-lock cargo generate-lockfile --offline --manifest-path "$ISO/Cargo.toml"
bytes_ver=$(awk '
  $0 ~ /name = "bytes"/ { hit = 1; next }
  hit && $0 ~ /version = / { print; exit }
' "$ISO/Cargo.lock")
echo "resolved_${bytes_ver}" | tee /evidence/proof/embed-bytes-lock.txt
if ! grep -q 'version = "1.12.1"' /evidence/proof/embed-bytes-lock.txt; then
  echo "resolved bytes is not 1.12.1" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
if ! grep -q 'fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04' "$ISO/Cargo.lock"; then
  echo "bytes 1.12.1 checksum missing from iso lock" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
cp "$ISO/Cargo.lock" /evidence/proof/embed-sqlx-core-Cargo.lock

run_phase sqlx-core cargo test --offline --locked --manifest-path "$ISO/Cargo.toml" --target-dir /target/sqlx-iso --lib -- --test-threads=1 --nocapture
if ! grep -q 'committed_init_len_matches_the_read_contract' /evidence/proof/embed-sqlx-core.log; then
  echo "committed_init_len test did not run" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
if ! grep -q 'init_mut_small_frames_shrink_the_receive_view' /evidence/proof/embed-sqlx-core.log; then
  echo "init_mut plateau test did not run" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi
if ! grep -q 'test result: ok' /evidence/proof/embed-sqlx-core.log; then
  echo "sqlx-core test result was not ok" >&2
  printf '%s\n' 4 > /evidence/proof/embed-worst.exit
  exit 4
fi

run_phase clippy cargo clippy --manifest-path /src/Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings

printf '%s\n' 0 > /evidence/proof/embed-worst.exit
echo "embed_worst_exit=0"
