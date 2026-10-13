#!/bin/bash
# Linux 1.94. One job. Fixed archives only. Does not open oo-dl01w1-06-pg.
set -euo pipefail

cd /work/tools/mongodb-preview
rustc --version | tee /evidence/rustc-version.txt
rustc --version | grep -q '^rustc 1.94.0 '

sha256sum /admission/oo-source-admission | tee /evidence/admission-bin.sha256
grep -q '^a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2  ' /evidence/admission-bin.sha256
sha256sum /frozen/native-readonly-receipt.json | tee /evidence/admission-receipt.sha256
grep -q '^65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef  ' /evidence/admission-receipt.sha256
sha256sum /frozen/batch.arrow | tee /evidence/arrow.sha256
grep -q '^4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8  ' /evidence/arrow.sha256

# Product crate first. A broken manifest writer stops before the pipeline build.
export CARGO_TARGET_DIR=/target/mongodb-preview-194
cargo fetch --locked --manifest-path /work/tools/mongodb-preview/Cargo.toml
cargo test --jobs 1 --locked --offline --manifest-path /work/tools/mongodb-preview/Cargo.toml -- --test-threads=1
printf '%s\n' 0 > /evidence/cargo-test.exit

export CARGO_TARGET_DIR=/target/pipeline-194
cargo fetch --locked --manifest-path /deps/pipeline/Cargo.toml
cargo build --jobs 1 --locked --offline --manifest-path /deps/pipeline/Cargo.toml -p oo-pipeline --bin oo-pipeline
printf '%s\n' 0 > /evidence/pipeline-build.exit

export CARGO_TARGET_DIR=/target/pg-builder-194
cargo fetch --locked --manifest-path /deps/pg/tools/pg-builder/Cargo.toml
cargo build --jobs 1 --locked --offline --manifest-path /deps/pg/tools/pg-builder/Cargo.toml --bin pg-builder
printf '%s\n' 0 > /evidence/pg-builder-build.exit

sha256sum /target/pipeline-194/debug/oo-pipeline | tee /evidence/oo-pipeline.sha256
sha256sum /target/pg-builder-194/debug/pg-builder | tee /evidence/pg-builder.sha256

python3 - << 'PY'
import hashlib, json
from pathlib import Path
plan = Path("/evidence/plan")
schema = json.loads((plan / "schema.json").read_text())["schema"]
schema_preimage = "".join(
    f"{item['name']}\t{item['type']}\t{str(item['nullable']).lower()}\n" for item in schema
)
type_preimage = "".join(f"{item['type']}\n" for item in schema)
config = (plan / "config.json").read_bytes()
report = (plan / "report.json").read_bytes()
arrow = Path("/frozen/batch.arrow").read_bytes()
capture = (plan / "capture-receipt.json").read_bytes()
digest = lambda data: hashlib.sha256(data).hexdigest()
digests = {
    "config_digest": digest(config),
    "schema_digest": digest(schema_preimage.encode()),
    "type_digest": digest(type_preimage.encode()),
    "input_digest": digest(arrow),
    "report_sha256": digest(report),
    "config_sha256": digest(config),
    "capture_receipt_sha256": digest(capture),
    "arrow_sha256": digest(arrow),
}
columns = []
for item in schema:
    precision = None
    scale = None
    if item["type"] == "decimal128(34,7)":
        precision = 34
        scale = 7
    columns.append({
        "name": item["name"],
        "arrow_type": item["type"],
        "nullable": item["nullable"],
        "precision": precision,
        "scale": scale,
    })
(plan / "digests.json").write_text(json.dumps(digests, indent=2) + "\n")
(plan / "columns.json").write_text(json.dumps(columns, indent=2) + "\n")
catalog = {
    "protocol": "oo.pipeline.sealed-import-catalog/v1",
    "engine": "mongodb",
    "source_id": "oo_w141",
    "source_generation": 1,
    "config_digest": digests["config_digest"],
    "schema_digest": digests["schema_digest"],
    "type_digest": digests["type_digest"],
    "input_digest": digests["input_digest"],
    "state": "not_sealed",
    "capture": "identity_only",
}
(plan / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
digests["catalog_sha256"] = digest((plan / "catalog.json").read_bytes())
(plan / "digests.json").write_text(json.dumps(digests, indent=2) + "\n")
Path("/evidence/digest-preimage.txt").write_text(
    "config_digest sha256 of config.json bytes\n"
    "schema_digest sha256 of name, type, nullable lines from the frozen decode schema\n"
    "type_digest sha256 of the type tokens, one per line\n"
    "input_digest sha256 of batch.arrow bytes\n"
    "catalog state is not_sealed. This file is the identity lock, not a physical seal.\n"
)
PY

export CARGO_TARGET_DIR=/target/mongodb-preview-194
cargo run --jobs 1 --locked --offline --quiet --manifest-path /work/tools/mongodb-preview/Cargo.toml -- \
  --emit-native \
  --config-digest "$(python3 -c 'import json; print(json.load(open("/evidence/plan/digests.json"))["config_digest"])')" \
  --schema-digest "$(python3 -c 'import json; print(json.load(open("/evidence/plan/digests.json"))["schema_digest"])')" \
  --type-digest "$(python3 -c 'import json; print(json.load(open("/evidence/plan/digests.json"))["type_digest"])')" \
  --input-digest "$(python3 -c 'import json; print(json.load(open("/evidence/plan/digests.json"))["input_digest"])')" \
  --arrow /frozen/batch.arrow \
  --batch-sha 4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8 \
  --out /evidence/plan/manifest.json

python3 - << 'PY'
import json
document = json.load(open("/evidence/plan/manifest.json"))
forbidden = {"admitted", "admission_bin", "registry_path", "registry_sha256", "approval"}
if forbidden & set(document) or document.get("source_engine") != "mongodb":
    raise SystemExit("manifest lost the mongodb identity")
if "clickhouse" in json.dumps(document):
    raise SystemExit("manifest names clickhouse")
PY

set +e
env -i /admission/oo-source-admission validate --report /evidence/plan/report.json --config /evidence/plan/config.json > /evidence/decision.bin
validate_rc=$?
set -e
printf '%s\n' "$validate_rc" > /evidence/validate.exit
if [[ "$validate_rc" -ne 0 ]]; then
  echo "validator exit ${validate_rc}" >&2
  exit 4
fi

: > /evidence/plan/password-not-read
printf '%s\n' 'not-a-ca' > /evidence/plan/ca-not-read
registry_sha="$(python3 /work/tools/mongodb-preview/scripts/bind-native.py /evidence/plan /evidence)"
printf '%s\n' "$registry_sha" > /evidence/registry.sha256

set +e
OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
OO_NATIVE_BINDING_REGISTRY=/evidence/registry.json \
OO_NATIVE_BINDING_REGISTRY_SHA256="$registry_sha" \
  /target/pg-builder-194/debug/pg-builder import-native \
  --manifest /evidence/plan/manifest.json \
  --out /evidence/envelope.json \
  --pipeline /target/pipeline-194/debug/oo-pipeline \
  --stderr /evidence/import.stderr
import_rc=$?
set -e
printf '%s\n' "$import_rc" > /evidence/import.exit

python3 - << 'PY'
import json
from pathlib import Path
envelope = json.loads(Path("/evidence/envelope.json").read_text())
import_body = envelope.get("import") or {}
Path("/evidence/load-state.txt").write_text(str(import_body.get("load_state")) + "\n")
Path("/evidence/import-reason.txt").write_text(str(import_body.get("reason")) + "\n")
gate = {
    "endpoint": "http://127.0.0.1:8317",
    "token_file": "/tmp/oo-customer-demo-20261007/llm-token",
    "token_read": False,
    "called": False,
    "http_status": None,
    "reason": "native import did not return a sealed catalog in load_state awaiting_publish",
}
if import_body.get("load_state") == "awaiting_publish":
    gate["reason"] = "awaiting_publish was reached; model still not called in this step"
Path("/evidence/productmodel-provider-gate.json").write_text(json.dumps(gate, indent=2) + "\n")
PY

if [[ -n "${HOST_UID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence /evidence/plan /work/tools/mongodb-preview || true
fi
