#!/bin/bash
# One linux/arm64 Rust 1.94 builder. Tests only oo-product-stack.
# Fixed services are release builds. Source and release-control stay unmounted.
# No cargo update, no generate-lockfile, no root manifest edit.
set -u
EVIDENCE=/evidence
OUT=/out
mkdir -p "$EVIDENCE"
exec > >(tee -a "$EVIDENCE/console.log") 2>&1
echo "inside_begin $(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc -vV > "$EVIDENCE/rustc.txt" 2>"$EVIDENCE/rustc.err"
printf '%s\n' $? > "$EVIDENCE/rustc.exit"
cargo -vV > "$EVIDENCE/cargo.txt" 2>"$EVIDENCE/cargo.err" || true
uname -m > "$EVIDENCE/uname.txt"
{
  command -v curl || true
  command -v openssl || true
  command -v wget || true
  command -v python3 || true
} > "$EVIDENCE/tools.txt"
if ! cargo clippy --version > "$EVIDENCE/clippy-version.txt" 2>&1; then
  rustup component add clippy --toolchain 1.94.0 > "$EVIDENCE/clippy-component.stdout" 2>"$EVIDENCE/clippy-component.stderr"
  printf '%s\n' $? > "$EVIDENCE/clippy-component.exit"
  cargo clippy --version > "$EVIDENCE/clippy-version.txt" 2>&1 || true
fi

cd /src/deploy/product-stack
set +e
cargo test --locked --offline --all-targets -- --test-threads=1 > "$EVIDENCE/test.stdout" 2>"$EVIDENCE/test.stderr"
printf '%s\n' $? > "$EVIDENCE/test.exit"
cargo clippy --locked --offline --all-targets --keep-going -- -D warnings > "$EVIDENCE/clippy.stdout" 2>"$EVIDENCE/clippy.stderr"
printf '%s\n' $? > "$EVIDENCE/clippy.exit"
cargo build --release --locked --offline --bin oo-product-stack > "$EVIDENCE/oracle-build.stdout" 2>"$EVIDENCE/oracle-build.stderr"
printf '%s\n' $? > "$EVIDENCE/oracle-build.exit"
set -e
if [ "$(cat "$EVIDENCE/test.exit")" != 0 ] || [ "$(cat "$EVIDENCE/clippy.exit")" != 0 ] || [ "$(cat "$EVIDENCE/oracle-build.exit")" != 0 ]; then
  echo "oracle_failed"
  exit 1
fi
cp /target/release/oo-product-stack "$OUT/oo-product-stack"
chmod 755 "$OUT/oo-product-stack"

build_one() {
  local name="$1" manifest="$2" bin="$3"
  echo "build_begin ${name}"
  set +e
  cargo fetch --locked --manifest-path "$manifest" > "$EVIDENCE/${name}-fetch.stdout" 2>"$EVIDENCE/${name}-fetch.stderr"
  printf '%s\n' $? > "$EVIDENCE/${name}-fetch.exit"
  if [ "$(cat "$EVIDENCE/${name}-fetch.exit")" != 0 ]; then
    echo "fetch_failed ${name}"
    return 0
  fi
  cargo build --release --locked --offline --manifest-path "$manifest" --bin "$bin" > "$EVIDENCE/${name}-build.stdout" 2>"$EVIDENCE/${name}-build.stderr"
  printf '%s\n' $? > "$EVIDENCE/${name}-build.exit"
  set -e
  if [ "$(cat "$EVIDENCE/${name}-build.exit")" = 0 ]; then
    cp "/target/release/${bin}" "$OUT/${bin}"
    chmod 755 "$OUT/${bin}"
    echo "build_ok ${name}"
  else
    echo "build_failed ${name}"
  fi
}

if [ "${OO_BUILD_IDENTITY:-0}" = 1 ]; then
  build_one control-identity /src/tools/control-identity/Cargo.toml control-identity
fi
if [ "${OO_BUILD_AUDIT:-0}" = 1 ]; then
  build_one audit-ingest /src/tools/audit-ingest/Cargo.toml audit-ingest
fi
if [ "${OO_BUILD_SESSION:-0}" = 1 ]; then
  build_one session-control-api /src/tools/session-control-api/Cargo.toml session-control-api
fi
if [ "${OO_BUILD_SELFBUILD:-0}" = 1 ]; then
  build_one selfbuild-control-api /archive/tools/selfbuild-control-api/Cargo.toml selfbuild-control-api
fi
echo "inside_end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
