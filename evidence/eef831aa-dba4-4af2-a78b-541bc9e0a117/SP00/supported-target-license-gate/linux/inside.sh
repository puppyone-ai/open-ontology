#!/bin/bash
# Inside the single a117 Linux arm64 Rust 1.94 builder.
# Reads /src read-only. Writes only /evidence and the private cargo/target volumes.
set -euo pipefail

PHASE="${1:?phase}"
EVID=/evidence
SRC=/src
RAW="$EVID/raw"
LOG="$EVID/linux/progress.txt"
export PATH="/cargo-home/bin:${PATH}"
export CARGO_TERM_COLOR=never

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

count_terms() {
  local file="$1" dest="$2"
  : >"$dest"
  local term
  for term in compile-time-rng runtime-rng tiny-keccak webpki-root-certs rustls-native-certs const-random const-random-macro arrow-array ahash getrandom; do
    local n
    n=$(grep -F -o -c "$term" "$file" || true)
    printf '%s\t%s\n' "$term" "$n" >>"$dest"
  done
}

tree_inv() {
  local dir="$1"
  local label="$2"
  shift 2
  mkdir -p "$dir"
  local code
  code=$(run_capture "$dir/${label}.stdout" "$dir/${label}.stderr" "$dir/${label}.exit" "$@")
  mark "tree $label exit=$code"
}

metadata_one() {
  local phase="$1" name="$2"
  shift 2
  local dir="$RAW/$phase/$name"
  mkdir -p "$dir"
  mark "metadata $phase $name begin"
  local code
  code=$(run_capture "$dir/metadata.json" "$dir/metadata.stderr" "$dir/metadata.exit" \
    cargo metadata --locked --offline --format-version 1 "$@")
  mark "metadata $phase $name exit=$code"
  if [ "$code" != "0" ]; then
    return 0
  fi
  sha256sum "$dir/metadata.json" >"$dir/metadata.sha256"
  set +e
  /cargo-home/meta_facts scan "$dir/metadata.json" "$dir/facts.txt"
  local facts_code=$?
  set -e
  printf '%s\n' "$facts_code" >"$dir/facts.exit"
  mark "facts $phase $name exit=$facts_code"
  wc -c "$dir/metadata.json" >"$dir/metadata.bytes"
}

trees_one() {
  local phase="$1" name="$2"
  shift 2
  local dir="$RAW/$phase/$name"
  mkdir -p "$dir"
  local target_args=("$@")
  mark "trees $phase $name begin"

  local code
  code=$(run_capture "$dir/tree-features.stdout" "$dir/tree-features.stderr" "$dir/tree-features.exit" \
    cargo tree --locked --offline "${target_args[@]}" -e features --charset ascii)
  mark "tree-features $phase $name exit=$code"
  if [ -s "$dir/tree-features.stdout" ]; then
    sha256sum "$dir/tree-features.stdout" >"$dir/tree-features.sha256"
    count_terms "$dir/tree-features.stdout" "$dir/tree-features.counts"
    wc -l "$dir/tree-features.stdout" >"$dir/tree-features.lines"
  fi

  code=$(run_capture "$dir/tree-normal.stdout" "$dir/tree-normal.stderr" "$dir/tree-normal.exit" \
    cargo tree --locked --offline "${target_args[@]}" -e normal --charset ascii --prefix none --format '{p} {f}')
  mark "tree-normal $phase $name exit=$code"
  if [ -s "$dir/tree-normal.stdout" ]; then
    sha256sum "$dir/tree-normal.stdout" >"$dir/tree-normal.sha256"
    sort "$dir/tree-normal.stdout" >"$dir/tree-normal.sorted"
    wc -l "$dir/tree-normal.sorted" >"$dir/tree-normal.lines"
  fi

  code=$(run_capture "$dir/tree-all-edges.stdout" "$dir/tree-all-edges.stderr" "$dir/tree-all-edges.exit" \
    cargo tree --locked --offline "${target_args[@]}" -e all --charset ascii --prefix none --format '{p} {f}')
  mark "tree-all-edges $phase $name exit=$code"

  local inv="$dir/inverse"
  tree_inv "$inv" "tiny-normal" cargo tree --locked --offline "${target_args[@]}" -e normal -i tiny-keccak@2.0.2 --charset ascii
  tree_inv "$inv" "tiny-features" cargo tree --locked --offline "${target_args[@]}" -e features -i tiny-keccak@2.0.2 --charset ascii
  tree_inv "$inv" "tiny-all" cargo tree --locked --offline "${target_args[@]}" -e all -i tiny-keccak@2.0.2 --charset ascii
  tree_inv "$inv" "tiny-oo-connectors-features" cargo tree -p oo-connectors --locked --offline "${target_args[@]}" -e features -i tiny-keccak@2.0.2 --charset ascii
  tree_inv "$inv" "webpki-normal" cargo tree --locked --offline "${target_args[@]}" -e normal -i webpki-root-certs@1.0.9 --charset ascii
  tree_inv "$inv" "webpki-features" cargo tree --locked --offline "${target_args[@]}" -e features -i webpki-root-certs@1.0.9 --charset ascii
  tree_inv "$inv" "webpki-all" cargo tree --locked --offline "${target_args[@]}" -e all -i webpki-root-certs@1.0.9 --charset ascii
  tree_inv "$inv" "webpki-oo-sdk-features" cargo tree -p oo-sdk --locked --offline "${target_args[@]}" -e features -i webpki-root-certs@1.0.9 --charset ascii
  tree_inv "$inv" "ahash-oo-connectors-features" cargo tree -p oo-connectors --locked --offline "${target_args[@]}" -e features -i ahash@0.8.12 --charset ascii
  tree_inv "$inv" "native-oo-sdk-features" cargo tree -p oo-sdk --locked --offline "${target_args[@]}" -e features -i rustls-native-certs@0.8.4 --charset ascii
  tree_inv "$inv" "verifier-oo-sdk-features" cargo tree -p oo-sdk --locked --offline "${target_args[@]}" -e features -i rustls-platform-verifier@0.7.0 --charset ascii
  tree_inv "$inv" "arrow-oo-connectors-features" cargo tree -p oo-connectors --locked --offline "${target_args[@]}" -e features -i arrow-array@60.0.0 --charset ascii
}

capture_graphs() {
  local phase="$1"
  metadata_one "$phase" aarch64-unknown-linux-gnu --filter-platform aarch64-unknown-linux-gnu
  metadata_one "$phase" x86_64-unknown-linux-gnu --filter-platform x86_64-unknown-linux-gnu
  metadata_one "$phase" wasm32-unknown-unknown --filter-platform wasm32-unknown-unknown
  metadata_one "$phase" all-target
  trees_one "$phase" aarch64-unknown-linux-gnu --target aarch64-unknown-linux-gnu
  trees_one "$phase" x86_64-unknown-linux-gnu --target x86_64-unknown-linux-gnu
  trees_one "$phase" wasm32-unknown-unknown --target wasm32-unknown-unknown
  trees_one "$phase" all-target --target all
}

smoke() {
  mark "smoke begin"
  {
    echo "uname_m=$(uname -m)"
    echo "uname_s=$(uname -s)"
    echo "pwd_src=$(pwd)"
  } >"$EVID/linux/uname.txt"
  rustc --version >"$EVID/linux/rustc-version.txt" 2>"$EVID/linux/rustc-version.err"
  rustc -vV >"$EVID/linux/rustc-vv.txt" 2>"$EVID/linux/rustc-vv.err"
  cargo --version >"$EVID/linux/cargo-version.txt" 2>"$EVID/linux/cargo-version.err"
  rustup show >"$EVID/linux/rustup-show.txt" 2>"$EVID/linux/rustup-show.err" || true
  # Toolchain file at /src must not move this builder off 1.94.0.
  local host rel
  host=$(awk -F': ' '$1=="host"{print $2}' "$EVID/linux/rustc-vv.txt")
  rel=$(awk -F': ' '$1=="release"{print $2}' "$EVID/linux/rustc-vv.txt")
  printf 'host=%s\nrelease=%s\n' "$host" "$rel" >"$EVID/linux/smoke-identity.txt"
  if [ "$host" != "aarch64-unknown-linux-gnu" ] || [ "$rel" != "1.94.0" ]; then
    mark "FAILED smoke host=$host release=$rel"
    return 4
  fi
  mark "smoke ok host=$host release=$rel"
}

compile_probe() {
  mark "probe compile"
  local code
  code=$(run_capture "$EVID/linux/probe-rustc.stdout" "$EVID/linux/probe-rustc.stderr" "$EVID/linux/probe-rustc.exit" \
    rustc --edition 2021 -D warnings -o /cargo-home/meta_facts "$EVID/probe/meta_facts.rs")
  mark "probe rustc exit=$code"
  if [ "$code" != "0" ]; then
    return 1
  fi
  code=$(run_capture "$EVID/linux/probe-test.stdout" "$EVID/linux/probe-test.stderr" "$EVID/linux/probe-test.exit" \
    /cargo-home/meta_facts test)
  mark "probe test exit=$code"
  [ "$code" = "0" ]
}

copy_upstream() {
  mark "upstream manifests"
  local dest="$RAW/upstream-manifests"
  mkdir -p "$dest"
  local crate rel src
  for crate in arrow-array-60.0.0 ahash-0.8.12 rustls-platform-verifier-0.7.0 tiny-keccak-2.0.2 webpki-root-certs-1.0.9 const-random-0.1.18 const-random-macro-0.1.16 rustls-native-certs-0.8.4 reqwest-0.13.5; do
    src=$(find /cargo-home/registry/src -type d -name "$crate" | head -n 1 || true)
    if [ -z "$src" ]; then
      echo "MISSING $crate" >>"$dest/index.txt"
      continue
    fi
    mkdir -p "$dest/$crate"
    for rel in Cargo.toml Cargo.toml.orig; do
      if [ -f "$src/$rel" ]; then
        cp "$src/$rel" "$dest/$crate/$rel"
      fi
    done
    echo "$crate $src" >>"$dest/index.txt"
  done
  # checksums of the crate files cargo stored. These are the lock checksums.
  {
    find /cargo-home/registry/cache -type f \
      \( -name 'tiny-keccak-2.0.2.crate' -o -name 'webpki-root-certs-1.0.9.crate' \
         -o -name 'arrow-array-60.0.0.crate' -o -name 'ahash-0.8.12.crate' \
         -o -name 'rustls-platform-verifier-0.7.0.crate' -o -name 'rustls-native-certs-0.8.4.crate' \) \
      -print0
  } | xargs -0 -r sha256sum >"$dest/crate-files.sha256" || true
  # license field lines only
  : >"$dest/license-lines.txt"
  local f
  for f in "$dest"/*/Cargo.toml "$dest"/*/Cargo.toml.orig; do
    [ -f "$f" ] || continue
    grep -n -E '^license' "$f" | while read -r line; do
      printf '%s:%s\n' "${f#"$dest/"}" "$line"
    done >>"$dest/license-lines.txt"
  done
  sha256sum "$dest"/*/Cargo.toml "$dest"/*/Cargo.toml.orig >"$dest/manifests.sha256" 2>"$dest/manifests.sha256.err" || true
}

phase1() {
  cd "$SRC"
  smoke
  compile_probe
  mark "fetch begin"
  local code
  code=$(run_capture "$RAW/fetch.stdout" "$RAW/fetch.stderr" "$RAW/fetch.exit" cargo fetch --locked --manifest-path "$SRC/Cargo.toml")
  mark "fetch exit=$code"
  if [ "$code" != "0" ]; then
    return 1
  fi
  copy_upstream
  mark "install cargo-deny 0.20.2 begin"
  code=$(run_capture "$RAW/install-cargo-deny.stdout" "$RAW/install-cargo-deny.stderr" "$RAW/install-cargo-deny.exit" \
    cargo install cargo-deny --version 0.20.2 --locked --root /cargo-home)
  mark "install cargo-deny exit=$code"
  if [ "$code" != "0" ]; then
    return 1
  fi
  cargo deny --version >"$RAW/cargo-deny-version.stdout" 2>"$RAW/cargo-deny-version.stderr"
  printf '%s\n' $? >"$RAW/cargo-deny-version.exit"
  sha256sum "$(command -v cargo-deny)" >"$RAW/cargo-deny-binary.sha256"
  mark "deny version $(cat "$RAW/cargo-deny-version.stdout")"
  sha256sum "$SRC/ci/deny.toml" "$SRC/Cargo.lock" >"$RAW/before-config-lock.sha256"
  capture_graphs before
  mark "phase1 complete"
}

compare_linux() {
  local ok=0
  local name base after
  : >"$EVID/linux/graph-compare.txt"
  for name in aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
    for base in metadata.sha256 tree-features.sha256 tree-normal.sha256; do
      local b="$RAW/before/$name/$base"
      local a="$RAW/after/$name/$base"
      if [ ! -f "$b" ] || [ ! -f "$a" ]; then
        echo "MISSING $name $base" >>"$EVID/linux/graph-compare.txt"
        ok=1
        continue
      fi
      if cmp -s "$b" "$a"; then
        echo "MATCH $name $base $(awk '{print $1}' "$a")" >>"$EVID/linux/graph-compare.txt"
      else
        echo "DIFFER $name $base" >>"$EVID/linux/graph-compare.txt"
        ok=1
      fi
    done
  done
  sha256sum "$SRC/Cargo.lock" "$SRC/ci/deny.toml" >"$RAW/after-config-lock.sha256"
  local lock_before lock_after
  lock_before=$(awk '$2=="/src/Cargo.lock"{print $1}' "$RAW/before-config-lock.sha256")
  lock_after=$(awk '$2=="/src/Cargo.lock"{print $1}' "$RAW/after-config-lock.sha256")
  if [ "$lock_before" = "$lock_after" ]; then
    echo "MATCH Cargo.lock $lock_after" >>"$EVID/linux/graph-compare.txt"
  else
    echo "DIFFER Cargo.lock before=$lock_before after=$lock_after" >>"$EVID/linux/graph-compare.txt"
    ok=1
  fi
  printf '%s\n' "$ok" >"$EVID/linux/graph-compare.exit"
  return "$ok"
}

record_advisory_db() {
  mark "advisory db locate"
  local hits="$RAW/advisory-db-paths.txt"
  : >"$hits"
  find /cargo-home -type d \( -name 'advisory-db' -o -name 'advisory-dbs' \) >>"$hits" 2>>"$RAW/advisory-db-find.err" || true
  # cargo-deny 0.20 keeps a git checkout under the cargo home.
  find /cargo-home -name HEAD -path '*advisory*' >>"$hits" 2>>"$RAW/advisory-db-find.err" || true
  local gitdir
  gitdir=$(find /cargo-home -type d -name .git | while read -r g; do
    parent=$(dirname "$g")
    case "$parent" in
      *advisory*) printf '%s\n' "$parent" ;;
    esac
  done | head -n 1 || true)
  if [ -n "$gitdir" ]; then
    git -C "$gitdir" rev-parse HEAD >"$RAW/advisory-db.rev" 2>"$RAW/advisory-db.rev.err" || true
    git -C "$gitdir" log -1 --format='%H%n%ci%n%s' >"$RAW/advisory-db.log" 2>>"$RAW/advisory-db.rev.err" || true
    printf '%s\n' "$gitdir" >"$RAW/advisory-db.dir"
  else
    echo "NO_ADVISORY_GIT" >"$RAW/advisory-db.rev"
  fi
}

phase2() {
  cd "$SRC"
  # Re-check the toolchain did not move between phases.
  rustc -vV >"$EVID/linux/rustc-vv-phase2.txt"
  local rel host
  rel=$(awk -F': ' '$1=="release"{print $2}' "$EVID/linux/rustc-vv-phase2.txt")
  host=$(awk -F': ' '$1=="host"{print $2}' "$EVID/linux/rustc-vv-phase2.txt")
  if [ "$rel" != "1.94.0" ] || [ "$host" != "aarch64-unknown-linux-gnu" ]; then
    mark "FAILED phase2 toolchain $rel $host"
    return 4
  fi
  sha256sum "$SRC/ci/deny.toml" >"$RAW/deny-toml-seen-by-phase2.sha256"
  mark "phase2 graphs"
  capture_graphs after
  if compare_linux; then
    mark "linux graphs match"
  else
    mark "LINUX GRAPH MISMATCH"
    echo "mismatch" >"$EVID/linux/GRAPH_MISMATCH"
  fi
  mark "deny check begin"
  # Exact gate command. No --target. Config is ci/deny.toml.
  local code
  code=$(run_capture "$RAW/deny.stdout" "$RAW/deny.stderr" "$RAW/deny.exit" \
    cargo deny --config "$SRC/ci/deny.toml" --locked check)
  mark "deny check exit=$code"
  record_advisory_db
  # Section summary is the stdout line. Keep a copy of the four words.
  if [ -s "$RAW/deny.stdout" ]; then
    cp "$RAW/deny.stdout" "$RAW/deny-sections.txt"
  fi
  mark "phase2 complete"
}

case "$PHASE" in
  phase1) phase1 ;;
  phase2) phase2 ;;
  *) mark "FAILED unknown phase"; exit 2 ;;
esac
