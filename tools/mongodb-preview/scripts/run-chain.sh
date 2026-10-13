#!/bin/bash
# One Linux 1.94 builder. Private dependency archive. No repository root mount.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/tools/mongodb-preview"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$EVIDENCE/raw/chain"
DEPS="/tmp/oo-dl01w1-41-deps"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
ARROW="$EVIDENCE/../mongodb-driver-preflight/batch.arrow"
CENSUS="$EVIDENCE/../mongodb-driver-preflight/raw/probe/field-census.json"
STATUS="/tmp/oo-dl01w1-41-chain.status"

phase() {
  mkdir -p "$EVIDENCE/raw"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/raw/phase.txt"
}

# oo-pipeline is not a pg-builder path dependency. The protocol refusal is inside pg-builder.
prepare_deps() {
  rm -rf "$DEPS"
  mkdir -p "$DEPS" "$OUT"
  # Pins, not HEAD. Later commits changed source-admission for other engines.
  git -C "$ROOT" archive 75e3dcf7aa6e61a8dce411942e6a4d97da0c3bf0 \
    tools/source-admission | tar -C "$DEPS" -xf -
  git -C "$ROOT" archive 39b2c1e17202a089897b3076ee0d71e194f767e7 \
    tools/pg-builder \
    tools/description-pack \
    tools/description-review | tar -C "$DEPS" -xf -
  git -C "$ROOT" archive b1ffb079d7beec4297dbd3deec97219584c6775d \
    crates/oo-builder \
    crates/oo-core \
    crates/oo-jobs \
    crates/oo-connectors/vendor | tar -C "$DEPS" -xf -
  python3 - "$DEPS" "$OUT/archive-note.txt" << 'PY'
import sys
from pathlib import Path
root = Path(sys.argv[1])
replacements = {
    "edition.workspace = true": 'edition = "2024"',
    "license.workspace = true": 'license = "Apache-2.0"',
    "publish.workspace = true": "publish = false",
    "rust-version.workspace = true": 'rust-version = "1.94.0"',
    "version.workspace = true": 'version = "0.1.0"',
}
lints = """[lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"

[lints.clippy]
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
"""
for rel in ("crates/oo-builder/Cargo.toml", "crates/oo-core/Cargo.toml", "crates/oo-jobs/Cargo.toml"):
    path = root / rel
    text = path.read_text()
    for old, new in replacements.items():
        if old not in text:
            raise SystemExit(f"missing {old} in {rel}")
        text = text.replace(old, new, 1)
    old_lints = "[lints]\nworkspace = true\n"
    if old_lints not in text:
        raise SystemExit(f"missing workspace lints in {rel}")
    text = text.replace(old_lints, lints, 1)
    if "workspace = true" in text or ".workspace = true" in text:
        raise SystemExit(f"workspace inheritance remains in {rel}")
    path.write_text(text)
cargo_toml = root / "Cargo.toml"
if cargo_toml.exists():
    cargo_toml.unlink()
Path(sys.argv[2]).write_text(
    "Private archive only. Repository files are unchanged. "
    "source-admission is 75e3dcf7aa6e61a8dce411942e6a4d97da0c3bf0, not HEAD. "
    "pg-builder, description-pack, and description-review are 39b2c1e17202a089897b3076ee0d71e194f767e7. "
    "oo-builder, oo-core, oo-jobs, and the sqlx vendor are b1ffb079d7beec4297dbd3deec97219584c6775d. "
    "rust-toolchain.toml and the root Cargo.toml are absent. "
    "oo-builder, oo-core, and oo-jobs receive workspace.package values "
    "edition 2024, license Apache-2.0, publish false, rust-version 1.94.0, and version 0.1.0 "
    "so pg-builder's own workspace can compile those path dependencies. "
    "import.rs is not edited.\n"
)
PY
  test ! -f "$DEPS/rust-toolchain.toml"
  test ! -f "$DEPS/Cargo.toml"
  test -f "$DEPS/tools/pg-builder/src/import.rs"
  grep -q 'oo.mysql-preview.manifest/v1' "$DEPS/tools/pg-builder/src/import.rs"
  grep -q 'edition = "2024"' "$DEPS/crates/oo-builder/Cargo.toml"
  if grep -R -n 'edition.workspace' "$DEPS/crates/oo-builder" "$DEPS/crates/oo-core" "$DEPS/crates/oo-jobs"; then
    echo "archive still inherits edition" >&2
    return 1
  fi
}

release_rust() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}

run_once() {
  set -euo pipefail
  docker image inspect "$RUST_IMAGE" >/dev/null
  mkdir -p "$OUT"
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  if ! python3 "$ADMIT" acquire rust "$OUT/admission.json"; then
    phase "chain capacity denied"
    return 3
  fi
  trap release_rust EXIT
  phase "chain admitted"
  prepare_deps
  set +e
  docker run --name "$RUST" \
    --platform linux/arm64 --pull never \
    --memory 6g --memory-swap 6g --cpus 1 \
    --pids-limit 1024 \
    -e CARGO_BUILD_JOBS=1 \
    -e CARGO_HOME=/cargo \
    -e CARGO_TERM_COLOR=never \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_TARGET_DIR=/target/mongodb-preview-194 \
    -e CARGO_INCREMENTAL=1 \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -v "${PREFIX}-cargo:/cargo" \
    -v "${PREFIX}-target:/target" \
    -v "$SRC:/work/tools/mongodb-preview" \
    -v "$DEPS/tools/source-admission:/work/tools/source-admission:ro" \
    -v "$DEPS/crates/oo-connectors/vendor:/work/crates/oo-connectors/vendor:ro" \
    -v "$DEPS:/deps:ro" \
    -v "$OUT:/evidence" \
    -v "$ARROW:/frozen/batch.arrow:ro" \
    -v "$CENSUS:/frozen/field-census.json:ro" \
    -w /work/tools/mongodb-preview \
    --label project=open-ontology \
    --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 \
    --label kind=rust-builder \
    "$RUST_IMAGE" \
    bash /work/tools/mongodb-preview/scripts/build-chain-inside.sh >"$OUT/docker.stdout" 2>"$OUT/docker.stderr"
  build_rc=$?
  set -e
  printf '%s\n' "$build_rc" >"$OUT/docker.exit"
  release_rust
  trap - EXIT
  phase "chain docker_exit=${build_rc}"
  return "$build_rc"
}

rm -f "$STATUS"
phase "chain wait start"
for attempt in $(seq 1 30); do
  phase "chain capacity attempt ${attempt}"
  set +e
  ( run_once )
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    echo "DONE chain 0" > "$STATUS"
    exit 0
  fi
  if [[ "$rc" -ne 3 ]]; then
    echo "FAILED chain ${rc}" > "$STATUS"
    exit "$rc"
  fi
  if [[ "$attempt" -eq 30 ]]; then
    phase "chain capacity exhausted"
    echo "FAILED capacity" > "$STATUS"
    exit 3
  fi
  phase "chain sleep 900"
  sleep 900
done
