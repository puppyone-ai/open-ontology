#!/bin/bash
# One a119 Linux arm64 Rust 1.94 builder. Reads /src. Writes /evidence and the cargo/target volumes.
set -euo pipefail

EVID=/evidence
SRC=/src
LOG="$EVID/linux/progress.txt"
export PATH="/cargo-home/bin:${PATH}"
export CARGO_TERM_COLOR=never
mkdir -p /work/home /cargo-home /target/a119

mark() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >>"$LOG"; }

run_capture() {
  local out="$1" err="$2" exitf="$3"
  shift 3
  set +e
  "$@" >"$out" 2>"$err"
  local code=$?
  set -e
  printf '%s\n' "$code" >"$exitf"
  printf '%s\n' "$code"
}

smoke() {
  mark "smoke begin"
  cd "$SRC"
  {
    echo "uname_s=$(uname -s)"
    echo "uname_m=$(uname -m)"
  } >"$EVID/linux/uname.txt"
  rustc --version >"$EVID/linux/rustc-version.txt" 2>"$EVID/linux/rustc-version.err"
  rustc -vV >"$EVID/linux/rustc-vv.txt" 2>"$EVID/linux/rustc-vv.err"
  cargo --version >"$EVID/linux/cargo-version.txt" 2>"$EVID/linux/cargo-version.err"
  local host rel
  host=$(awk -F': ' '$1=="host"{print $2}' "$EVID/linux/rustc-vv.txt")
  rel=$(awk -F': ' '$1=="release"{print $2}' "$EVID/linux/rustc-vv.txt")
  printf 'host=%s\nrelease=%s\n' "$host" "$rel" >"$EVID/linux/smoke-identity.txt"
  if [ "$host" != "aarch64-unknown-linux-gnu" ] || [ "$rel" != "1.94.0" ]; then
    mark "FAILED smoke host=$host release=$rel"
    return 4
  fi
  mkdir -p /target/a119
  local code
  code=$(run_capture "$EVID/linux/probe-rustc.stdout" "$EVID/linux/probe-rustc.stderr" "$EVID/linux/probe-rustc.exit" \
    rustc --edition 2021 --deny warnings -o /target/a119/graph_oracle "$EVID/probe/graph_oracle.rs")
  mark "probe rustc exit=$code"
  if [ "$code" != "0" ]; then
    return "$code"
  fi
  code=$(run_capture "$EVID/linux/probe-test.stdout" "$EVID/linux/probe-test.stderr" "$EVID/linux/probe-test.exit" \
    /target/a119/graph_oracle self-test)
  mark "probe self-test exit=$code"
  if [ "$code" != "0" ]; then
    return "$code"
  fi
  mark "smoke ok host=$host release=$rel"
}

install_deny() {
  mark "install cargo-deny 0.20.2"
  local code
  code=$(run_capture "$EVID/raw/install-cargo-deny.stdout" "$EVID/raw/install-cargo-deny.stderr" "$EVID/raw/install-cargo-deny.exit" \
    cargo install cargo-deny --version 0.20.2 --locked --root /cargo-home)
  mark "install exit=$code"
  if [ "$code" != "0" ]; then
    return "$code"
  fi
  sha256sum /cargo-home/bin/cargo-deny >"$EVID/raw/cargo-deny-binary.sha256"
  code=$(run_capture "$EVID/raw/cargo-deny-version.stdout" "$EVID/raw/cargo-deny-version.stderr" "$EVID/raw/cargo-deny-version.exit" \
    cargo deny --version)
  mark "cargo deny version exit=$code"
  [ "$code" = "0" ]
}

fetch_locked() {
  mark "fetch locked"
  local code
  code=$(run_capture "$EVID/raw/fetch.stdout" "$EVID/raw/fetch.stderr" "$EVID/raw/fetch.exit" \
    cargo fetch --locked --manifest-path /src/Cargo.toml)
  mark "fetch exit=$code"
  [ "$code" = "0" ]
}

deny_one() {
  local name="$1" config="$2" debug="$3"
  local dir="$EVID/raw/deny/$name"
  mkdir -p "$dir"
  sha256sum "$config" >"$dir/config.sha256"
  if [ "$debug" = "debug" ]; then
    printf '%s\n' "cargo deny --log-level debug --config $config --locked check" >"$dir/command.txt"
    mark "deny $name debug begin"
    run_capture "$dir/stdout" "$dir/stderr" "$dir/exit" \
      cargo deny --log-level debug --config "$config" --locked check >/dev/null
  else
    printf '%s\n' "cargo deny --config $config --locked check" >"$dir/command.txt"
    mark "deny $name exact begin"
    run_capture "$dir/stdout" "$dir/stderr" "$dir/exit" \
      cargo deny --config "$config" --locked check >/dev/null
  fi
  local code
  code=$(cat "$dir/exit")
  mark "deny $name exit=$code"
  {
    echo "exit=$code"
    echo "stdout=$(tr '\n' ' ' <"$dir/stdout" | head -c 400)"
    echo "stderr_bytes=$(wc -c <"$dir/stderr" | tr -d ' ')"
    echo "rejected=$(grep -c 'error\[rejected\]' "$dir/stderr" || true)"
    echo "cc0=$(grep -c 'CC0-1.0' "$dir/stderr" || true)"
    echo "cdla=$(grep -c 'CDLA-Permissive-2.0' "$dir/stderr" || true)"
    echo "duplicate=$(grep -c 'warning\[duplicate\]' "$dir/stderr" || true)"
    echo "tiny=$(grep -c 'tiny-keccak' "$dir/stderr" || true)"
    echo "webpki=$(grep -c 'webpki-root-certs' "$dir/stderr" || true)"
  } >"$dir/sections.txt"
}

metadata_one() {
  local name="$1"
  shift
  local dir="$EVID/raw/metadata/$name"
  mkdir -p "$dir"
  mark "metadata $name begin"
  local code
  code=$(run_capture "$dir/metadata.json" "$dir/metadata.stderr" "$dir/exit" \
    cargo metadata --locked --offline --format-version 1 "$@")
  mark "metadata $name exit=$code"
  if [ "$code" = "0" ]; then
    sha256sum "$dir/metadata.json" >"$dir/metadata.sha256"
    wc -c "$dir/metadata.json" >"$dir/metadata.bytes"
  fi
}

tree_one() {
  local name="$1"
  shift
  local dir="$EVID/raw/tree/$name"
  mkdir -p "$dir"
  mark "tree $name begin"
  local code
  code=$(run_capture "$dir/normal.stdout" "$dir/normal.stderr" "$dir/normal.exit" \
    cargo tree --locked --offline "$@" -e normal --charset ascii --prefix none --format '{p}')
  mark "tree normal $name exit=$code"
  code=$(run_capture "$dir/features.stdout" "$dir/features.stderr" "$dir/features.exit" \
    cargo tree --locked --offline "$@" -e features --charset ascii)
  mark "tree features $name exit=$code"
  if [ -s "$dir/features.stdout" ]; then
    : >"$dir/feature.counts"
    local term n
    for term in compile-time-rng runtime-rng tiny-keccak webpki-root-certs rustls-native-certs const-random const-random-macro arrow-array core_detect multiversion multiversion-macros rustversion; do
      n=$(grep -F -o -c "$term" "$dir/features.stdout" || true)
      printf '%s\t%s\n' "$term" "$n" >>"$dir/feature.counts"
    done
  fi
  if [ -s "$dir/normal.stdout" ]; then
    for term in core_detect multiversion multiversion-macros rustversion tiny-keccak webpki-root-certs rustls-native-certs arrow-array; do
      if grep -F -q "$term" "$dir/normal.stdout"; then
        printf 'present\t%s\n' "$term"
      else
        printf 'absent\t%s\n' "$term"
      fi
    done >"$dir/normal.markers"
  fi
}

inverse_one() {
  local dir="$1" label="$2"
  shift 2
  run_capture "$dir/${label}.stdout" "$dir/${label}.stderr" "$dir/${label}.exit" "$@" >/dev/null || true
  wc -c "$dir/${label}.stdout" "$dir/${label}.stderr" >"$dir/${label}.bytes" || true
}

advisory_id() {
  mark "advisory identity"
  local found=0
  : >"$EVID/raw/advisory-db.rev"
  if [ -d /cargo-home/advisory-dbs ]; then
    find /cargo-home/advisory-dbs -name HEAD -type f >"$EVID/raw/advisory-db.paths" || true
    while IFS= read -r head; do
      local repo
      repo=$(dirname "$head")
      if [ "$(basename "$repo")" = ".git" ]; then
        repo=$(dirname "$repo")
      fi
      if git -C "$repo" rev-parse HEAD >>"$EVID/raw/advisory-db.rev" 2>>"$EVID/raw/advisory-db.rev.err"; then
        git -C "$repo" log -1 --format='%H%x09%ci%x09%s' >>"$EVID/raw/advisory-db.log" 2>>"$EVID/raw/advisory-db.rev.err" || true
        found=1
      fi
    done <"$EVID/raw/advisory-db.paths"
  fi
  mark "advisory found=$found"
}

oracle_pair() {
  local name="$1" meta="$2" log="$3"
  shift 3
  local dir="$EVID/raw/oracle/$name"
  mkdir -p "$dir"
  mark "oracle $name begin"
  local code
  code=$(run_capture "$dir/eval.stdout" "$dir/eval.stderr" "$dir/eval.exit" \
    /target/a119/graph_oracle eval "$meta" "$dir/oracle.txt" "$@")
  mark "oracle eval $name exit=$code"
  if [ -n "$log" ]; then
    code=$(run_capture "$dir/subtract.stdout" "$dir/subtract.stderr" "$dir/subtract.exit" \
      /target/a119/graph_oracle subtract "$meta" "$log" "$dir/deny.txt")
    mark "oracle subtract $name exit=$code"
    code=$(run_capture "$dir/diff.stdout" "$dir/diff.stderr" "$dir/diff.exit" \
      /target/a119/graph_oracle diff "$dir/oracle.txt" "$dir/deny.txt" "$dir/diff.txt")
    mark "oracle diff $name exit=$code"
  fi
}

smoke
install_deny
fetch_locked

sha256sum /src/ci/deny.toml /src/Cargo.lock /src/crates/oo-sdk/Cargo.toml /src/crates/oo-connectors/Cargo.toml /src/rust-toolchain.toml \
  >"$EVID/raw/archive-hashes.txt"

deny_one canonical /src/ci/deny.toml exact
deny_one canonical-debug /src/ci/deny.toml debug
deny_one aarch64-only "$EVID/controls/aarch64-only.toml" debug
deny_one x86_64-only "$EVID/controls/x86_64-only.toml" debug
deny_one linux-plus-wasm "$EVID/controls/linux-plus-wasm.toml" debug
deny_one unfiltered "$EVID/controls/unfiltered.toml" debug
advisory_id

metadata_one unfiltered
metadata_one aarch64-unknown-linux-gnu --filter-platform aarch64-unknown-linux-gnu
metadata_one x86_64-unknown-linux-gnu --filter-platform x86_64-unknown-linux-gnu
metadata_one wasm32-unknown-unknown --filter-platform wasm32-unknown-unknown

tree_one aarch64-unknown-linux-gnu --target aarch64-unknown-linux-gnu
tree_one x86_64-unknown-linux-gnu --target x86_64-unknown-linux-gnu
tree_one wasm32-unknown-unknown --target wasm32-unknown-unknown
tree_one all-target --target all

for triple in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu wasm32-unknown-unknown; do
  inv="$EVID/raw/tree/$triple/inverse"
  mkdir -p "$inv"
  inverse_one "$inv" tiny cargo tree --locked --offline --target "$triple" -e features -i tiny-keccak@2.0.2 --charset ascii
  inverse_one "$inv" webpki cargo tree --locked --offline --target "$triple" -e features -i webpki-root-certs@1.0.9 --charset ascii
  inverse_one "$inv" native cargo tree -p oo-sdk --locked --offline --target "$triple" -e features -i rustls-native-certs@0.8.4 --charset ascii
  inverse_one "$inv" arrow cargo tree -p oo-connectors --locked --offline --target "$triple" -e features -i arrow-array@60.0.0 --charset ascii
done
inv="$EVID/raw/tree/all-target/inverse"
mkdir -p "$inv"
inverse_one "$inv" tiny cargo tree --locked --offline --target all -e features -i tiny-keccak@2.0.2 --charset ascii
inverse_one "$inv" webpki cargo tree --locked --offline --target all -e features -i webpki-root-certs@1.0.9 --charset ascii

oracle_pair canonical "$EVID/raw/metadata/unfiltered/metadata.json" "$EVID/raw/deny/canonical-debug/stderr" \
  --targets aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu
oracle_pair aarch64-only "$EVID/raw/metadata/aarch64-unknown-linux-gnu/metadata.json" "$EVID/raw/deny/aarch64-only/stderr" \
  --targets aarch64-unknown-linux-gnu
oracle_pair aarch64-from-unfiltered "$EVID/raw/metadata/unfiltered/metadata.json" "" \
  --targets aarch64-unknown-linux-gnu
oracle_pair x86_64-only "$EVID/raw/metadata/x86_64-unknown-linux-gnu/metadata.json" "$EVID/raw/deny/x86_64-only/stderr" \
  --targets x86_64-unknown-linux-gnu
oracle_pair linux-plus-wasm "$EVID/raw/metadata/unfiltered/metadata.json" "$EVID/raw/deny/linux-plus-wasm/stderr" \
  --targets aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu,wasm32-unknown-unknown
oracle_pair unfiltered "$EVID/raw/metadata/unfiltered/metadata.json" "$EVID/raw/deny/unfiltered/stderr" \
  --all-targets
oracle_pair wasm-metadata "$EVID/raw/metadata/wasm32-unknown-unknown/metadata.json" "" \
  --targets wasm32-unknown-unknown

mark "inside done"
