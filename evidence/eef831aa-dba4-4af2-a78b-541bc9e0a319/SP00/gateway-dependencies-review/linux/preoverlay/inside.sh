#!/bin/bash
# Independent pre-overlay proof of the frozen gateway HTTP baseline.
# Archive commit is mounted read-only at /src. No root lock rewrite.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
mkdir -p /target/tmp
export TMPDIR=/target/tmp

EV=/evidence
UP=$EV/upstream
LIC=$EV/licenses
PROBE=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies/probe
AS_OF=2026-10-06T12:47:28Z
UA='open-ontology-a319-review'

phase() {
  printf '%s\n' "$2" > "$EV/$1.exit"
  echo "PHASE $1=$2"
}

hash_one() {
  local file="$1"
  local out="$2"
  local stage=/target/hash-one
  rm -rf "$stage"
  mkdir -p "$stage"
  cp "$file" "$stage/$(basename "$file")"
  /target/sha256_manifest "$stage" "$out" >/tmp/hash-one.log
}

cached_crate() {
  local name="$1"
  local ver="$2"
  local found count
  found=$(find /cargo-home/registry/cache -type f -name "${name}-${ver}.crate" | sort)
  count=$(printf '%s\n' "$found" | sed '/^$/d' | wc -l | tr -d ' ')
  if [ "$count" -ne 1 ]; then
    printf '%s\n' "$found" > "$UP/${name}-${ver}.find"
    echo "expected one cached ${name}-${ver}.crate, found ${count}" >&2
    return 1
  fi
  printf '%s\n' "$found"
}

verify_crate() {
  local name="$1"
  local ver="$2"
  local lock="$3"
  local json="$UP/${name}-${ver}.json"
  local crate="$UP/crates/${name}-${ver}.crate"
  local cached
  mkdir -p "$UP/crates"
  curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -A "$UA" \
    -o "$json" "https://crates.io/api/v1/crates/${name}/${ver}"
  curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -A "$UA" \
    -o "$crate" "https://static.crates.io/crates/${name}/${name}-${ver}.crate"
  hash_one "$crate" "$UP/${name}-${ver}.official.sha256"
  cached=$(cached_crate "$name" "$ver")
  hash_one "$cached" "$UP/${name}-${ver}.cached.sha256"
  /target/crate_verify meta "$json" "$name" "$ver" "$AS_OF" \
    "$UP/${name}-${ver}.official.sha256" "$UP/${name}-${ver}.cached.sha256" \
    > "$UP/${name}-${ver}.meta"
  local extract="$LIC/${name}-${ver}"
  rm -rf "$extract"
  mkdir -p "$extract"
  tar -xzf "$crate" -C "$extract"
  /target/crate_verify licenses "$extract" "$UP/${name}-${ver}.meta" > "$LIC/${name}-${ver}.licenses"
  /target/crate_verify manifest "$extract/${name}-${ver}/Cargo.toml" "$UP/${name}-${ver}.meta" \
    > "$LIC/${name}-${ver}.manifest"
  /target/crate_verify lock-sha "$lock" "$name" "$ver" "$UP/${name}-${ver}.official.sha256" \
    > "$UP/${name}-${ver}.lock-sha"
}

cd /src
echo "=== identity ==="
rustc -vV | tee "$EV/rustc-version.txt"
cargo -V | tee "$EV/cargo-version.txt"
rustup show | tee "$EV/rustup-show.txt"
if ! grep -q 'release: 1.94' "$EV/rustc-version.txt"; then
  echo "compiler is not rustc 1.94" >&2
  phase check 2
  exit 2
fi

echo "=== frozen blobs ==="
cmp -s /src/Cargo.toml /blobs/Cargo.toml || { echo "Cargo.toml drifted" >&2; phase blobs 2; exit 2; }
cmp -s /src/Cargo.lock /blobs/Cargo.lock || { echo "Cargo.lock drifted" >&2; phase blobs 2; exit 2; }
cmp -s "$PROBE/Cargo.toml" /blobs/probe-Cargo.toml || { echo "probe manifest drifted" >&2; phase blobs 2; exit 2; }
cmp -s "$PROBE/Cargo.lock" /blobs/probe-Cargo.lock || { echo "probe lock drifted" >&2; phase blobs 2; exit 2; }
cmp -s "$PROBE/src/lib.rs" /blobs/probe-lib.rs || { echo "probe lib drifted" >&2; phase blobs 2; exit 2; }
cmp -s "$PROBE/tests/loopback.rs" /blobs/probe-loopback.rs || { echo "probe test drifted" >&2; phase blobs 2; exit 2; }
cmp -s /src/Cargo.lock "$EV/expected-lock.bytes" || { echo "expected lock drifted" >&2; phase blobs 2; exit 2; }
cp "$PROBE/Cargo.lock" "$EV/probe-lock-before.bytes"
phase blobs 0

echo "=== oracles ==="
set +e
rustc --edition 2024 -D warnings -o /target/lockdiff /tools/lockdiff.rs \
  > "$EV/lockdiff-build.stdout" 2> "$EV/lockdiff-build.stderr"
lockdiff_build=$?
rustc --edition 2021 -D warnings -o /target/sha256_manifest /tools/sha256_manifest.rs \
  > "$EV/sha256-build.stdout" 2> "$EV/sha256-build.stderr"
sha_build=$?
rustc --edition 2024 -D warnings -o /target/crate_verify /tools/crate_verify.rs \
  > "$EV/crate-verify-build.stdout" 2> "$EV/crate-verify-build.stderr"
verify_build=$?
set -e
phase lockdiff_build "$lockdiff_build"
phase sha256_build "$sha_build"
phase crate_verify_build "$verify_build"
if [ "$lockdiff_build" -ne 0 ] || [ "$sha_build" -ne 0 ] || [ "$verify_build" -ne 0 ]; then
  exit 1
fi
set +e
/target/lockdiff selfcheck > "$EV/lockdiff-selfcheck.stdout" 2> "$EV/lockdiff-selfcheck.stderr"
self_exit=$?
/target/crate_verify selfcheck > "$EV/crate-verify-selfcheck.stdout" 2> "$EV/crate-verify-selfcheck.stderr"
verify_self=$?
set -e
phase lockdiff_selfcheck "$self_exit"
phase crate_verify_selfcheck "$verify_self"
if [ "$self_exit" -ne 0 ] || [ "$verify_self" -ne 0 ]; then
  exit 1
fi

echo "=== root pins and unused members ==="
/target/crate_verify root-pins /src/Cargo.toml > "$EV/root-pins.txt"
phase root_pins $?
if [ "$(cat "$EV/root_pins.exit")" != "0" ]; then
  exit 1
fi
/target/crate_verify members /src/crates > "$EV/members.txt"
phase members $?
if [ "$(cat "$EV/members.exit")" != "0" ]; then
  exit 1
fi
/target/crate_verify probe-manifest "$PROBE/Cargo.toml" > "$EV/probe-manifest.txt"
phase probe_manifest $?
if [ "$(cat "$EV/probe_manifest.exit")" != "0" ]; then
  exit 1
fi

echo "=== smoke: metadata then scoped arch-check ==="
set +e
cargo metadata --manifest-path /src/Cargo.toml --locked --offline --format-version 1 --no-deps \
  > "$EV/metadata.stdout" 2> "$EV/metadata.stderr"
metadata_exit=$?
set -e
phase metadata "$metadata_exit"
if [ "$metadata_exit" -ne 0 ]; then
  phase check not_run
  exit "$metadata_exit"
fi

unset CARGO_NET_OFFLINE
set +e
cargo check --manifest-path /src/Cargo.toml --locked -p oo-arch-check \
  > "$EV/check.stdout" 2> "$EV/check.stderr"
check_exit=$?
set -e
export CARGO_NET_OFFLINE=true
phase check "$check_exit"
if [ "$check_exit" -ne 0 ]; then
  exit "$check_exit"
fi
cmp -s /src/Cargo.toml /blobs/Cargo.toml
cmp -s /src/Cargo.lock /blobs/Cargo.lock
cmp -s /src/Cargo.lock "$EV/expected-lock.bytes"
phase manifest_unchanged 0

echo "=== probe fetch ==="
unset CARGO_NET_OFFLINE
set +e
cargo fetch --manifest-path "$PROBE/Cargo.toml" --locked \
  > "$EV/fetch.stdout" 2> "$EV/fetch.stderr"
fetch_exit=$?
set -e
export CARGO_NET_OFFLINE=true
phase fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
cmp -s "$PROBE/Cargo.lock" "$EV/probe-lock-before.bytes"
cmp -s /src/Cargo.lock "$EV/expected-lock.bytes"

echo "=== official crate bytes ==="
mkdir -p "$UP" "$LIC"
set +e
verify_crate axum 0.8.9 "$PROBE/Cargo.lock" \
  && verify_crate axum-core 0.5.6 "$PROBE/Cargo.lock" \
  && verify_crate tower 0.5.3 "$PROBE/Cargo.lock" \
  && verify_crate tower-http 0.6.11 "$PROBE/Cargo.lock" \
  && verify_crate httpdate 1.0.3 "$PROBE/Cargo.lock" \
  && verify_crate matchit 0.8.4 "$PROBE/Cargo.lock" \
  && verify_crate mime 0.3.17 "$PROBE/Cargo.lock"
crates_exit=$?
set -e
phase crates "$crates_exit"
if [ "$crates_exit" -ne 0 ]; then
  exit "$crates_exit"
fi
/target/crate_verify lock-sha /src/Cargo.lock tower 0.5.3 "$UP/tower-0.5.3.official.sha256" \
  > "$UP/tower-0.5.3.root-lock-sha"
/target/crate_verify lock-sha /src/Cargo.lock tower-http 0.6.11 "$UP/tower-http-0.6.11.official.sha256" \
  > "$UP/tower-http-0.6.11.root-lock-sha"

verify_support() {
  local name="$1"
  local ver="$2"
  local crate="$UP/crates/${name}-${ver}.crate"
  local cached
  curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -A "$UA" \
    -o "$crate" "https://static.crates.io/crates/${name}/${name}-${ver}.crate"
  hash_one "$crate" "$UP/${name}-${ver}.official.sha256"
  cached=$(cached_crate "$name" "$ver")
  hash_one "$cached" "$UP/${name}-${ver}.cached.sha256"
  cmp -s "$UP/${name}-${ver}.official.sha256" "$UP/${name}-${ver}.cached.sha256"
  /target/crate_verify lock-sha "$PROBE/Cargo.lock" "$name" "$ver" "$UP/${name}-${ver}.official.sha256" \
    > "$UP/${name}-${ver}.lock-sha"
  local extract="$LIC/${name}-${ver}"
  rm -rf "$extract"
  mkdir -p "$extract"
  tar -xzf "$crate" -C "$extract"
}

set +e
verify_support hyper 1.11.1 && verify_support hyper-util 0.1.20
support_exit=$?
set -e
phase support_crates "$support_exit"
if [ "$support_exit" -ne 0 ]; then
  exit "$support_exit"
fi
/target/crate_verify axum-edges "$LIC/axum-0.8.9/axum-0.8.9/Cargo.toml" > "$EV/axum-edges.txt"
phase axum_edges $?
/target/crate_verify hyper-httpdate "$LIC/hyper-1.11.1/hyper-1.11.1/Cargo.toml" > "$EV/hyper-httpdate.txt"
phase hyper_httpdate $?
/target/crate_verify hyper-util-server "$LIC/hyper-util-0.1.20/hyper-util-0.1.20/Cargo.toml" > "$EV/hyper-util-server.txt"
phase hyper_util_server $?
if [ "$(cat "$EV/axum_edges.exit")" != "0" ] || [ "$(cat "$EV/hyper_httpdate.exit")" != "0" ] || [ "$(cat "$EV/hyper_util_server.exit")" != "0" ]; then
  exit 1
fi

echo "=== newest published versions, recorded, not a bump ==="
mkdir -p "$UP/index"
set +e
index_ok=0
for name in axum axum-core tower tower-http httpdate matchit mime; do
  curl -fsSL --retry 5 --retry-all-errors --retry-delay 2 -A "$UA" \
    -o "$UP/index/${name}.json" "https://crates.io/api/v1/crates/${name}" \
    && /target/crate_verify index-newest "$UP/index/${name}.json" "$name" > "$UP/index/${name}.txt" \
    || index_ok=1
done
set -e
phase index_newest "$index_ok"
if [ "$index_ok" -ne 0 ]; then
  exit 1
fi

echo "=== lock diff ==="
/target/crate_verify collect-pins \
  "$UP/axum-0.8.9.meta" \
  "$UP/axum-core-0.5.6.meta" \
  "$UP/httpdate-1.0.3.meta" \
  "$UP/matchit-0.8.4.meta" \
  "$UP/mime-0.3.17.meta" > "$UP/new-crate-pins.txt"
phase collect_pins $?
if [ "$(cat "$EV/collect_pins.exit")" != "0" ]; then
  exit 1
fi
set +e
/target/lockdiff subset /src/Cargo.lock "$PROBE/Cargo.lock" "$UP/new-crate-pins.txt" "$AS_OF" \
  > "$EV/lockdiff-subset.stdout" 2> "$EV/lockdiff-subset.stderr"
subset_exit=$?
set -e
phase lockdiff_subset "$subset_exit"
if [ "$subset_exit" -ne 0 ]; then
  exit "$subset_exit"
fi
/target/crate_verify subset-report "$EV/lockdiff-subset.stdout" "$UP/new-crate-pins.txt" \
  > "$EV/subset-report.txt"
phase subset_report $?
/target/crate_verify edges /src/Cargo.lock "$PROBE/Cargo.lock" "$UP/new-crate-pins.txt" \
  > "$EV/edges.txt"
phase edges $?
set +e
/target/lockdiff exact "$EV/expected-lock.bytes" /src/Cargo.lock \
  > "$EV/lockdiff-root.stdout" 2> "$EV/lockdiff-root.stderr"
root_exit=$?
set -e
phase lockdiff_root "$root_exit"
if [ "$(cat "$EV/subset_report.exit")" != "0" ] || [ "$(cat "$EV/edges.exit")" != "0" ] || [ "$root_exit" -ne 0 ]; then
  exit 1
fi

echo "=== archived probe tests and clippy ==="
unset CARGO_NET_OFFLINE
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add --toolchain 1.94.0 clippy > "$EV/clippy-install.stdout" 2> "$EV/clippy-install.stderr"
  phase clippy_install $?
else
  printf '%s\n' already_installed > "$EV/clippy-install.stdout"
  phase clippy_install 0
fi
if [ "$(cat "$EV/clippy_install.exit")" != "0" ]; then
  exit 1
fi
export CARGO_NET_OFFLINE=true
set +e
cargo test --manifest-path "$PROBE/Cargo.toml" --locked --offline -- --test-threads=1 \
  > "$EV/test.stdout" 2> "$EV/test.stderr"
test_exit=$?
set -e
phase test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
/target/crate_verify test-report "$EV/test.stdout" > "$EV/test-report.txt"
phase test_report $?
if [ "$(cat "$EV/test_report.exit")" != "0" ]; then
  exit 1
fi
set +e
cargo clippy --manifest-path "$PROBE/Cargo.toml" --locked --offline --all-targets -- -D warnings \
  > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
clippy_exit=$?
set -e
phase clippy "$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

cmp -s /src/Cargo.lock "$EV/expected-lock.bytes"
cmp -s "$PROBE/Cargo.lock" "$EV/probe-lock-before.bytes"
cmp -s /src/Cargo.toml /blobs/Cargo.toml
phase lock 0
printf '%s\n' "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "$EV/inside-ok.txt"
exit 0
