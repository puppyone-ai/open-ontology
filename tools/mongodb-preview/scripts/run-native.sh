#!/bin/bash
# One Linux 1.94 native-import run. Fixed archives. Does not join the MySQL06 network.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/tools/mongodb-preview"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$EVIDENCE/raw/native-bind"
PLAN="$OUT/plan"
PIPE="/tmp/oo-dl01w1-41-native-pipeline"
PG="/tmp/oo-dl01w1-41-native-pg"
ADMIT_SRC="/tmp/oo-dl01w1-41-native-admit"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-native"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
ARROW="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/batch.arrow"
PREFLIGHT="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
DECODED="$PREFLIGHT/raw/probe/decoded.json"
RECEIPT="$ROOT/evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json"
ADMISSION_BIN="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin"
PIN="d3b3abcff32cc85602d7736eb1340b14813c069b"
STATUS="/tmp/oo-dl01w1-41-native.status"

phase() {
  mkdir -p "$EVIDENCE/raw"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/raw/phase.txt"
}

prepare_plan() {
  mkdir -p "$PLAN"
  git -C "$ROOT" show 5df40d24eefa4fbcfa00c4415cfcfde5d368db89:tools/source-admission/fixtures/native/mongodb-product-report.json > "$PLAN/report.json"
  git -C "$ROOT" show 5df40d24eefa4fbcfa00c4415cfcfde5d368db89:tools/source-admission/fixtures/native/mongodb-product-config.json > "$PLAN/config.json"
  cp "$DECODED" "$PLAN/capture-receipt.json"
  python3 - "$DECODED" "$PLAN/schema.json" << 'PY'
import json, sys
document = json.load(open(sys.argv[1]))
json.dump({"schema": document["schema"], "rows": document["rows"], "decoder": document["decoder"]}, open(sys.argv[2], "w"), indent=2)
open(sys.argv[2], "a").write("\n")
PY
  python3 - "$PLAN/report.json" "$PLAN/config.json" << 'PY'
import json, sys
report = json.load(open(sys.argv[1]))
config = json.load(open(sys.argv[2]))
assert report["source_id"] == "oo_w141" and report["compat_mode"] == "mongodb-native"
assert report["driver"]["digest"].startswith("622f272c")
assert config["requested_write_class"] == "R0" and config["r1w"] is None
assert config["source_id"] == "oo_w141"
PY
}

prepare_deps() {
  rm -rf "$PIPE" "$PG" "$ADMIT_SRC"
  mkdir -p "$PIPE" "$PG" "$ADMIT_SRC" "$OUT"
  git -C "$ROOT" archive "$PIN" \
    Cargo.toml Cargo.lock \
    crates/oo-core crates/oo-jobs crates/oo-control crates/oo-connectors \
    crates/oo-pipeline crates/oo-builder crates/oo-gateway crates/oo-sdk \
    crates/oo-cli crates/oo-mcp crates/oo-proxy crates/oo-sandbox \
    crates/oo-entry crates/oo-eval crates/oo-analysis crates/oo-arch-check \
    crates/xtask \
    | tar -C "$PIPE" -xf -
  git -C "$ROOT" archive "$PIN" \
    tools/pg-builder tools/description-pack tools/description-review \
    crates/oo-builder crates/oo-core crates/oo-jobs \
    | tar -C "$PG" -xf -
  git -C "$ROOT" archive 75e3dcf7aa6e61a8dce411942e6a4d97da0c3bf0 tools/source-admission \
    | tar -C "$ADMIT_SRC" -xf -
  python3 - "$PIPE" "$PG" "$OUT/archive-note.txt" "$PIN" << 'PY'
import sys
from pathlib import Path
pipe = Path(sys.argv[1])
pg = Path(sys.argv[2])
note = Path(sys.argv[3])
pin = sys.argv[4]
if (pipe / "rust-toolchain.toml").exists():
    raise SystemExit("pipeline archive contains a toolchain file")
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
    path = pg / rel
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
root_cargo = pg / "Cargo.toml"
if root_cargo.exists():
    root_cargo.unlink()
if not (pg / "tools/pg-builder/src/native_import.rs").is_file():
    raise SystemExit("pg archive has no native_import")
if not (pipe / "crates/oo-pipeline/src/native_binding.rs").is_file():
    raise SystemExit("pipeline archive has no native_binding")
note.write_text(
    "Private archive of " + pin + " for oo-pipeline and pg-builder. "
    "source-admission product tests use 75e3dcf7aa6e61a8dce411942e6a4d97da0c3bf0. "
    "Live workspace sources were not mounted. "
    "The admission binary is the frozen a205eb57 file, mounted read-only.\n"
)
PY
}

release_rust() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}

run_once() {
  set -euo pipefail
  docker image inspect "$RUST_IMAGE" >/dev/null
  prepare_plan
  prepare_deps
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  if ! python3 "$ADMIT" acquire rust "$OUT/admission.json"; then
    phase "native capacity denied"
    return 3
  fi
  trap release_rust EXIT
  phase "native admitted"
  set +e
  docker run --name "$RUST" \
    --platform linux/arm64 --pull never \
    --memory 6g --memory-swap 6g --cpus 1 \
    --pids-limit 1024 \
    -e CARGO_BUILD_JOBS=1 \
    -e CARGO_HOME=/cargo \
    -e CARGO_TERM_COLOR=never \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_INCREMENTAL=1 \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -v "${PREFIX}-cargo:/cargo" \
    -v "${PREFIX}-target:/target" \
    -v "$SRC:/work/tools/mongodb-preview" \
    -v "$ADMIT_SRC/tools/source-admission:/work/tools/source-admission:ro" \
    -v "$PIPE/crates/oo-connectors/vendor:/work/crates/oo-connectors/vendor:ro" \
    -v "$PIPE:/deps/pipeline:ro" \
    -v "$PG:/deps/pg:ro" \
    -v "$OUT:/evidence" \
    -v "$ARROW:/frozen/batch.arrow:ro" \
    -v "$RECEIPT:/frozen/native-readonly-receipt.json:ro" \
    -v "$ADMISSION_BIN:/admission:ro" \
    -v "$PREFLIGHT:/work/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight:ro" \
    -w /work/tools/mongodb-preview \
    --label project=open-ontology \
    --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 \
    --label kind=rust-builder \
    "$RUST_IMAGE" \
    bash /work/tools/mongodb-preview/scripts/build-native-inside.sh >"$OUT/docker.stdout" 2>"$OUT/docker.stderr"
  build_rc=$?
  set -e
  printf '%s\n' "$build_rc" >"$OUT/docker.exit"
  release_rust
  trap - EXIT
  phase "native docker_exit=${build_rc}"
  return "$build_rc"
}

rm -f "$STATUS"
phase "native wait start"
for attempt in $(seq 1 30); do
  phase "native capacity attempt ${attempt}"
  set +e
  ( run_once )
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    echo "DONE native 0" > "$STATUS"
    exit 0
  fi
  if [[ "$rc" -ne 3 ]]; then
    echo "FAILED native ${rc}" > "$STATUS"
    exit "$rc"
  fi
  phase "native sleep 900"
  sleep 900
done
echo "FAILED native capacity" > "$STATUS"
exit 3
