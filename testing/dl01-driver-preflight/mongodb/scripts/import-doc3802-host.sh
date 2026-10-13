#!/bin/bash
# Generation-2 native import of the 16-row doc Arrow. Does not alter the retained table.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
ELF="/tmp/oo-dl01w1-41-elf-631720c5"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMISSION="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission"
RECEIPT="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain/import-batch/native-readonly-receipt.json"
PIPELINE_SHA="f2894da67ec5877c2c6322f17bcb8377e4ca1f03c3f99d4b39236d22b34a9631"
DECISION_SHA="56eb2fd4348bedaadcc9d82fc1d2688f80a6d7600b8a8e18c9441c7271967e84"
RETAINED="pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254"
EXPECTED="pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
  --label kind=rust-builder
)

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence dir already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
  echo "named container $RUST still present" >&2
  exit 2
fi
got_elf="$(shasum -a 256 "$ELF/oo-pipeline" | awk '{print $1}')"
[[ "$got_elf" == "$PIPELINE_SHA" ]]
got_bin="$(shasum -a 256 "$ADMISSION" | awk '{print $1}')"
[[ "$got_bin" == "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2" ]]
got_receipt="$(shasum -a 256 "$RECEIPT" | awk '{print $1}')"
[[ "$got_receipt" == "65b7f22fe26686f8a1e61d705a78d5a1a69f2a123de0b9dfce827cce2ffb59ef" ]]
python3 "$SRC/scripts/plan-doc3802.py" >/tmp/oo-w141-registry-sha.txt
registry_sha="$(cat /tmp/oo-w141-registry-sha.txt)"
rm -f /tmp/oo-w141-registry-sha.txt
[[ "$(cat "$EVIDENCE/registry.sha256")" == "$registry_sha" ]]

psql_at() {
  docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c "$1"
}
retained_rows="$(psql_at "SELECT count(*) FROM _oo.${RETAINED}")"
retained_jsonb="$(psql_at "SELECT count(*) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid WHERE n.nspname='_oo' AND c.relname='${RETAINED}' AND t.typname='jsonb' AND a.attnum>0 AND NOT a.attisdropped")"
new_tables="$(psql_at "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname='${EXPECTED}'")"
[[ "$retained_rows" == "16" && "$retained_jsonb" == "0" && "$new_tables" == "0" ]]
printf '%s\n' "retained_rows=$retained_rows" "retained_jsonb_columns=$retained_jsonb" "new_table_exists=$new_tables" >"$EVIDENCE/pre-pg.txt"

docker image inspect "$RUST_IMAGE" >/dev/null
python3 "$ADMIT" acquire rust "$EVIDENCE/admission.json"
cleanup() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

set +e
docker run --name "$RUST" \
  --network oo-dl01w1-41-net \
  --platform linux/arm64 --pull never \
  --memory 2g --memory-swap 2g --cpus 1 \
  --pids-limit 256 \
  -e OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
  -e OO_NATIVE_BINDING_REGISTRY=/evidence/registry.json \
  -e OO_NATIVE_BINDING_REGISTRY_SHA256="$registry_sha" \
  -e DECISION_SHA="$DECISION_SHA" \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -v "$ELF:/elf:ro" \
  -v "$SRC/scripts/import-doc3802-inside.sh:/import-inside.sh:ro" \
  -v "$ADMISSION:/admission/oo-source-admission:ro" \
  -v "$RECEIPT:/frozen/native-readonly-receipt.json:ro" \
  -v "$SECRETS/admin-password:/run/secrets/admin-password:ro" \
  -v "$SECRETS/ca.crt:/run/secrets/ca.crt:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /import-inside.sh
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
decision_sha="$(shasum -a 256 "$EVIDENCE/decision.bin" | awk '{print $1}')"
printf '%s\n' "$decision_sha" >"$EVIDENCE/decision.sha256"
if [[ "$decision_sha" != "$DECISION_SHA" ]]; then
  printf 'FAILED decision %s\n' "$decision_sha" >"$EVIDENCE/status.txt"
  exit 3
fi
import_rc="$(cat "$EVIDENCE/import.exit")"
if [[ "$import_rc" != "0" ]]; then
  printf 'FAILED import %s\n' "$import_rc" >"$EVIDENCE/status.txt"
  exit "$import_rc"
fi
python3 - << 'PY'
import json
from pathlib import Path
root = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import")
receipt = json.loads((root / "receipt.json").read_text())
if receipt.get("load_state") != "awaiting_publish":
    raise SystemExit(f"load_state {receipt.get('load_state')} reason {receipt.get('reason')}")
if receipt.get("loader_invoked") is not True:
    raise SystemExit("loader was not invoked")
tables = receipt.get("tables") or []
if len(tables) != 1:
    raise SystemExit("table count")
table = tables[0]
if table.get("physical_name") != "pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898":
    raise SystemExit(f"physical {table.get('physical_name')}")
if table.get("row_count") != 16:
    raise SystemExit(f"rows {table.get('row_count')}")
doc = [item for item in table.get("columns", []) if item.get("logical_name") == "doc"]
if len(doc) != 1 or doc[0].get("oid") != 3802 or doc[0].get("typmod") != -1:
    raise SystemExit(f"doc column {doc}")
print("receipt-ok", receipt.get("run_id"))
PY
retained_rows="$(psql_at "SELECT count(*) FROM _oo.${RETAINED}")"
retained_jsonb="$(psql_at "SELECT count(*) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid WHERE n.nspname='_oo' AND c.relname='${RETAINED}' AND t.typname='jsonb' AND a.attnum>0 AND NOT a.attisdropped")"
new_rows="$(psql_at "SELECT count(*) FROM _oo.${EXPECTED}")"
doc_type="$(psql_at "SELECT t.typname || ':' || a.atttypmod::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid WHERE n.nspname='_oo' AND c.relname='${EXPECTED}' AND a.attnum>0 AND NOT a.attisdropped AND t.typname='jsonb'")"
printf '%s\n' "retained_rows=$retained_rows" "retained_jsonb_columns=$retained_jsonb" "new_rows=$new_rows" "doc_type=$doc_type" | tee "$EVIDENCE/post-pg.txt"
[[ "$retained_rows" == "16" && "$retained_jsonb" == "0" && "$new_rows" == "16" && "$doc_type" == "jsonb:-1" ]]
printf '%s\n' DONE >"$EVIDENCE/status.txt"
