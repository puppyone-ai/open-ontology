#!/bin/bash
# Linux 1.94. One job. Fixed archives only. Does not open another task's database.
set -euo pipefail

cd /work/tools/starrocks-preview
if [[ -n "${HOST_UID:-}" ]]; then
  trap 'chown -R "${HOST_UID}:${HOST_GID}" /evidence || true' EXIT
fi

phase() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a /evidence/phase.txt
}

phase "rustc"
rustc --version | tee /evidence/rustc-version.txt
rustc --version | grep -q '^rustc 1.94.0 '

sha256sum /admission/oo-source-admission | tee /evidence/admission-bin.sha256
grep -q '^a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2  ' /evidence/admission-bin.sha256
sha256sum /frozen/native-readonly-receipt.json | tee /evidence/admission-receipt.sha256
grep -q '^65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef  ' /evidence/admission-receipt.sha256

phase "build pipeline"
export CARGO_TARGET_DIR=/target/pipeline-194
cargo fetch --locked --manifest-path /deps/pipeline/Cargo.toml
cargo build --jobs 1 --locked --offline --manifest-path /deps/pipeline/Cargo.toml -p oo-pipeline --bin oo-pipeline
printf '%s\n' 0 > /evidence/pipeline-build.exit

phase "build pg-builder"
export CARGO_TARGET_DIR=/target/pg-builder-194
cargo fetch --locked --manifest-path /deps/pg/tools/pg-builder/Cargo.toml
cargo build --jobs 1 --locked --offline --manifest-path /deps/pg/tools/pg-builder/Cargo.toml --bin pg-builder
printf '%s\n' 0 > /evidence/pg-builder-build.exit

sha256sum /target/pipeline-194/debug/oo-pipeline | tee /evidence/oo-pipeline.sha256
sha256sum /target/pg-builder-194/debug/pg-builder | tee /evidence/pg-builder.sha256
phase "binaries hashed"

python3 /work/tools/starrocks-preview/scripts/bind-native.py prepare \
  /evidence/plan /frozen/decoded /frozen/read
phase "manifests prepared"

set +e
env -i /admission/oo-source-admission validate \
  --report /evidence/plan/report.json \
  --config /evidence/plan/config.json \
  > /evidence/decision.bin \
  2> /evidence/validate.stderr
validate_rc=$?
set -e
printf '%s\n' "$validate_rc" > /evidence/validate.exit
phase "validator exit ${validate_rc}"
if [[ "$validate_rc" -ne 0 ]]; then
  exit 4
fi

registry_sha="$(python3 /work/tools/starrocks-preview/scripts/bind-native.py bind /evidence/plan /evidence)"
printf '%s\n' "$registry_sha" > /evidence/registry.sha256
phase "registry bound"

python3 - << 'PY'
import json
from pathlib import Path
tables = json.loads(Path("/evidence/plan/tables.json").read_text())["tables"]
Path("/evidence/plan/import-order.txt").write_text("".join(item["name"] + "\n" for item in tables))
PY

while IFS= read -r table; do
  phase "import ${table}"
  set +e
  OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
  OO_NATIVE_BINDING_REGISTRY=/evidence/registry.json \
  OO_NATIVE_BINDING_REGISTRY_SHA256="$registry_sha" \
    /target/pg-builder-194/debug/pg-builder import-native \
    --manifest "/evidence/plan/${table}/manifest.json" \
    --out "/evidence/plan/${table}/envelope.json" \
    --pipeline /target/pipeline-194/debug/oo-pipeline \
    --stderr "/evidence/plan/${table}/import.stderr"
  import_rc=$?
  set -e
  printf '%s\n' "$import_rc" > "/evidence/plan/${table}/import.exit"
  phase "import ${table} exit ${import_rc}"
done < /evidence/plan/import-order.txt

python3 - << 'PY'
import json
from pathlib import Path
rows = []
ready = []
for exit_path in sorted(Path("/evidence/plan").glob("*/import.exit")):
    envelope_path = exit_path.parent / "envelope.json"
    body = {}
    if envelope_path.is_file():
        body = json.loads(envelope_path.read_text()).get("import") or {}
    row = {
        "table": exit_path.parent.name,
        "load_state": body.get("load_state"),
        "reason": body.get("reason"),
        "loader_invoked": body.get("loader_invoked"),
        "run_id": body.get("run_id"),
        "source_id": body.get("source_id"),
        "source_engine": body.get("source_engine"),
        "envelope_present": envelope_path.is_file(),
        "exit": int(exit_path.read_text().strip()),
    }
    rows.append(row)
    if row["reason"] == "target_absent":
        ready.append(row["table"])
Path("/evidence/import-summary.json").write_text(json.dumps({"tables": rows}, indent=2) + "\n")
Path("/evidence/plan-ready-tables.txt").write_text("".join(name + "\n" for name in ready))
called = any(row["load_state"] == "awaiting_publish" for row in rows)
gate = {
    "endpoint": "http://127.0.0.1:8317",
    "token_file": "/tmp/oo-customer-demo-20261007/llm-token",
    "token_read": False,
    "called": False,
    "http_status": None,
    "reason": "awaiting_publish was not reached, so the model was not called",
}
if called:
    gate["reason"] = "awaiting_publish was reached; this step still did not call the model"
Path("/evidence/productmodel-provider-gate.json").write_text(json.dumps(gate, indent=2) + "\n")
PY
phase "import summary written"
