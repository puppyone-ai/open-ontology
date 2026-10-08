#!/bin/bash
# Build and prove mcp-gateway inside the private arm64 Rust 1.94 container.
# stdout of this script is phase lines. Cargo output stays in files under /evidence.
set -euo pipefail

EVID=/evidence
phase() {
  printf '%s %s\n' "$(date -u +%H:%M:%S)" "$1" | tee -a "$EVID/phase.txt" >&2
}

scrub() {
  perl - "$EVID" <<'PERL'
use strict;
use warnings;
my $dir = $ARGV[0];
sub slurp {
  my ($path) = @_;
  open my $fh, '<', $path or return '';
  local $/;
  my $text = <$fh> // '';
  $text =~ s/\s+\z//;
  return $text;
}
my @needles = grep { length $_ } map { slurp($_) } (
  '/run/secrets/token-admin.pass',
  '/run/secrets/token-runtime.pass',
);
opendir my $dh, $dir or exit 0;
while (my $name = readdir $dh) {
  next if $name !~ /\.(txt|log|stdout|stderr)$/;
  my $path = "$dir/$name";
  next unless -f $path;
  open my $in, '<', $path or next;
  local $/;
  my $text = <$in> // '';
  close $in;
  for my $needle (@needles) {
    $text =~ s/\Q$needle\E/[redacted]/g;
  }
  $text =~ s/ooct_\S+/ooct_redacted/g;
  $text =~ s/oost_\S+/oost_redacted/g;
  open my $out, '>', $path or next;
  print {$out} $text;
  close $out;
}
PERL
}

trap 'status=$?; scrub || true; exit "$status"' EXIT

: > "$EVID/phase.txt"
phase "start"
rustc -vV > "$EVID/rustc.txt"
grep -q 'release: 1.94.0' "$EVID/rustc.txt"
uname -m | grep -qx aarch64
phase "rustc_1.94_arm64"
phase "clippy_component_begin"
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "clippy_component=already_installed" > "$EVID/clippy-component.stdout"
  printf '%s\n' 0 > "$EVID/clippy-component.exit"
  phase "clippy_component_exit=0"
else
  echo "=== rustup component add clippy --toolchain 1.94.0 ===" > "$EVID/clippy-component.stdout"
  set +e
  rustup component add clippy --toolchain 1.94.0 >> "$EVID/clippy-component.stdout" 2> "$EVID/clippy-component.stderr"
  clippy_component=$?
  set -e
  printf '%s\n' "$clippy_component" > "$EVID/clippy-component.exit"
  phase "clippy_component_exit=$clippy_component"
  test "$clippy_component" -eq 0
fi

sha256sum /src/Cargo.lock /src/Cargo.toml > "$EVID/root-inside-before.txt"
mkdir -p /cargo-home/bin /target/root /target/child
export CARGO_HOME=/cargo-home
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/target/child
export PATH="/usr/local/cargo/bin:/cargo-home/bin:${PATH}"
export RUSTUP_TOOLCHAIN=1.94.0

phase "helpers"
rustc --edition 2021 -O /tools/select-debs.rs -o /cargo-home/bin/select-debs
rustc --edition 2021 -O /tools/sha256.rs -o /cargo-home/bin/sha256
phase "helpers_ready"

phase "libclang_begin"
set +e
bash /extract-libclang.sh > "$EVID/libclang-extract.log" 2>&1
libclang_exit=$?
set -e
printf '%s\n' "$libclang_exit" > "$EVID/libclang.exit"
phase "libclang_exit=$libclang_exit"
test "$libclang_exit" -eq 0
libdir=$(find /cargo-home/a345-clang-root/usr/lib -name 'libclang.so' -print -quit)
test -n "$libdir"
libdir=$(dirname "$libdir")
multiarch=/cargo-home/a345-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a345-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
test -f "$llvm_inc/stdbool.h"
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a345-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a345-clang-root/usr/share/pkgconfig"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > "$EVID/libclang.txt"
phase "libclang_ready"

if [ ! -s /src/tools/mcp-gateway/Cargo.lock ]; then
  phase "lock_generate_begin"
  set +e
  cargo generate-lockfile --manifest-path /src/tools/mcp-gateway/Cargo.toml > "$EVID/lock-generate.stdout" 2> "$EVID/lock-generate.stderr"
  lock_exit=$?
  set -e
  printf '%s\n' "$lock_exit" > "$EVID/lock-generate.exit"
  phase "lock_generate_exit=$lock_exit"
  test "$lock_exit" -eq 0
  date -u +%Y-%m-%dT%H:%M:%SZ > "$EVID/lock-generated.txt"
  printf 'generated once with cargo generate-lockfile in tools/mcp-gateway\n' >> "$EVID/lock-generated.txt"
else
  printf 'child lock already present; generate-lockfile was not run again\n' > "$EVID/lock-generated.txt"
  phase "lock_present"
fi

awk '
  $0 == "name = \"rmcp\"" { found = 1 }
  found && $1 == "version" { version = $3 }
  found && $1 == "checksum" { checksum = $3; exit }
  END {
    gsub(/"/, "", version);
    gsub(/"/, "", checksum);
    printf "version=%s\nchecksum=%s\n", version, checksum;
  }
' /src/tools/mcp-gateway/Cargo.lock > "$EVID/rmcp-checksum.txt"
grep -q 'version=3.5.0' "$EVID/rmcp-checksum.txt"
phase "rmcp_checksum_recorded"

phase "fetch_child_begin"
set +e
cargo fetch --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked > "$EVID/fetch-child.stdout" 2> "$EVID/fetch-child.stderr"
fetch_child=$?
set -e
printf '%s\n' "$fetch_child" > "$EVID/fetch-child.exit"
phase "fetch_child_exit=$fetch_child"
test "$fetch_child" -eq 0

phase "fetch_root_begin"
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > "$EVID/fetch-root.stdout" 2> "$EVID/fetch-root.stderr"
fetch_root=$?
set -e
printf '%s\n' "$fetch_root" > "$EVID/fetch-root.exit"
phase "fetch_root_exit=$fetch_root"
test "$fetch_root" -eq 0
if ! cmp -s <(sha256sum /src/Cargo.lock /src/Cargo.toml) "$EVID/root-inside-before.txt"; then
  echo "root lock changed during fetch" >&2
  exit 6
fi
phase "root_lock_unchanged_after_fetch"

phase "tree_begin"
cargo tree --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked --offline -p mcp-gateway --edges normal > "$EVID/cargo-tree-normal.txt"
if grep -E 'oo-gateway|pg_query' "$EVID/cargo-tree-normal.txt"; then
  echo "production tree links the gateway" >&2
  exit 4
fi
phase "tree_clean"

run_logged() {
  local name=$1
  shift
  phase "${name}_begin"
  set +e
  "$@" > "$EVID/${name}.stdout" 2> "$EVID/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "$EVID/${name}.exit"
  phase "${name}_exit=$code"
  test "$code" -eq 0
}

run_logged clippy-protocol \
  cargo clippy --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked --offline --all-targets -- -D warnings
run_logged test-protocol \
  cargo test --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked --offline --all-targets

if [ "${OO_A388_STOP_AFTER_PROTOCOL:-}" = "1" ]; then
  phase "protocol_only_done"
  exit 0
fi

export CARGO_TARGET_DIR=/target/root
run_logged build-gateway \
  cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --bin oo-gateway -p oo-builder --bin role-pack-bundle
phase "fixture_begin"
rustc --edition 2021 -O /src/testing/real-db/gateway-compiled-pack/src/fixture.rs -o /target/root/oo-a388-fixture
phase "fixture_ready"
export CARGO_TARGET_DIR=/target/child
export OO_A388_GATEWAY_BIN=/target/root/debug/oo-gateway
export OO_A388_ROLE_PACK_BIN=/target/root/debug/role-pack-bundle
export OO_A388_FIXTURE_BIN=/target/root/oo-a388-fixture
export OO_A388_FIXTURE_DIR=/src/testing/real-db/gateway-compiled-pack/fixtures

run_logged clippy-live \
  cargo clippy --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked --offline --all-targets --features live -- -D warnings
run_logged test-live \
  cargo test --manifest-path /src/tools/mcp-gateway/Cargo.toml --locked --offline --features live --test live -- --ignored --nocapture

sha256sum /src/Cargo.lock /src/Cargo.toml > "$EVID/root-inside-after.txt"
if ! cmp -s "$EVID/root-inside-before.txt" "$EVID/root-inside-after.txt"; then
  echo "root lock changed during the proof" >&2
  exit 6
fi
phase "done"
