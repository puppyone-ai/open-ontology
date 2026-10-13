#!/bin/bash
# One Linux 1.94 native-import run for sr_w45. Fixed archives. Own volumes only.
set -euo pipefail

echo $$ > /tmp/oo-dl01w1-45-native.pid
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/tools/starrocks-preview"
EVIDENCE="$ROOT/evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product"
PREFLIGHT="$ROOT/evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-driver-preflight"
OUT="$EVIDENCE/raw/native-bind"
PLAN="$OUT/plan"
PIPE="/tmp/oo-dl01w1-45-native-pipeline"
PG="/tmp/oo-dl01w1-45-native-pg"
PREFIX="oo-dl01w1-45"
RUST="${PREFIX}-rust"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$ROOT/testing/dl01-driver-preflight/starrocks/scripts/admit.py"
ARROW="$PREFLIGHT/native/read"
DECODED="$PREFLIGHT/native/decoded-initial"
REPORT="$EVIDENCE/run/out/admission-report.json"
CONFIG="$EVIDENCE/run/out/admission-config.json"
RECEIPT="$ROOT/evidence/2ecaf08e-eb23-4b7a-a720-e07459e6f2a6/DL01/budget-report/native-readonly-completion/receipt.json"
ADMISSION_BIN="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin"
PIN="d3b3abcff32cc85602d7736eb1340b14813c069b"
STATUS="/tmp/oo-dl01w1-45-native.status"

phase() {
  mkdir -p "$EVIDENCE/raw"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/raw/phase.txt"
}

prepare_plan() {
  mkdir -p "$PLAN"
  cp "$REPORT" "$PLAN/report.json"
  cp "$CONFIG" "$PLAN/config.json"
  python3 "$SRC/scripts/classify-reader-audit.py" "$PREFLIGHT" "$EVIDENCE/raw/reader-journey-audit.json"
  python3 - "$PLAN/report.json" "$PLAN/config.json" << 'PY'
import json, sys
report = json.load(open(sys.argv[1]))
config = json.load(open(sys.argv[2]))
assert report["source_id"] == "sr_w45" and report["compat_mode"] == "starrocks-native"
assert report["driver"]["digest"] == "21bbdc81d436bbffb2951b2245068c1667dccd02ed32a6667b3770b4b7bd1aee"
assert config["requested_write_class"] == "R0" and config["r1w"] is None
assert config["source_id"] == "sr_w45"
PY
}

prepare_deps() {
  rm -rf "$PIPE" "$PG"
  mkdir -p "$PIPE" "$PG" "$OUT"
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
  test -f "$ADMISSION_BIN/oo-source-admission"
  test -f "$RECEIPT"
  test -f "$ARROW/pk_accounts.arrow"
  test -f "$ARROW/agg_metrics.arrow"
  prepare_plan
  prepare_deps
  if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
    docker rm -f "$RUST" >/dev/null
  fi
  set +e
  python3 "$ADMIT" acquire-start rust "$OUT/capacity.json" -- \
    docker run -d --name "$RUST" \
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
      -v "$SRC:/work/tools/starrocks-preview:ro" \
      -v "$PIPE:/deps/pipeline:ro" \
      -v "$PG:/deps/pg:ro" \
      -v "$OUT:/evidence" \
      -v "$ARROW:/frozen/read:ro" \
      -v "$DECODED:/frozen/decoded:ro" \
      -v "$RECEIPT:/frozen/native-readonly-receipt.json:ro" \
      -v "$ADMISSION_BIN:/admission:ro" \
      -w /work/tools/starrocks-preview \
      --label project=open-ontology \
      --label owner=2c023948-0862-4dae-ae8f-6fc30241db62 \
      --label session=2c023948-0862-4dae-ae8f-6fc30241db62 \
      --label task=DL01-W1-45 \
      --label kind=rust-builder \
      "$RUST_IMAGE" \
      bash /work/tools/starrocks-preview/scripts/build-native-inside.sh
  start_rc=$?
  set -e
  if [[ "$start_rc" -eq 2 ]]; then
    phase "native capacity denied"
    return 3
  fi
  if [[ "$start_rc" -ne 0 ]]; then
    phase "native docker start failed ${start_rc}"
    return "$start_rc"
  fi
  phase "native container started"
  set +e
  docker wait "$RUST" >"$OUT/docker.exit"
  wait_rc=$?
  docker logs "$RUST" >"$OUT/docker.stdout" 2>"$OUT/docker.stderr"
  set -e
  build_rc="$(cat "$OUT/docker.exit")"
  release_rust
  phase "native docker_exit=${build_rc} wait=${wait_rc}"
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
  if [[ "$rc" -eq 0 || "$rc" -eq 4 ]]; then
    echo "DONE native ${rc}" > "$STATUS"
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
