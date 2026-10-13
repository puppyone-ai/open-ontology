#!/bin/bash
# Install 0009 and one real customer jsonb row. Does not alter the retained table.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-69-jsonb-one"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ELF="/tmp/oo-dl01w1-41-elf-631720c5"
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
python3 "$SRC/scripts/import-one-doc.py"
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
  -v "$ELF:/elf:ro" \
  -v "$SECRETS/admin-password:/run/secrets/admin-password:ro" \
  -v "$SECRETS/ca.crt:/run/secrets/ca.crt:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash -c '/elf/oo-pipeline import-jsonb-document < /evidence/request.json' \
  >"$EVIDENCE/receipt.json" 2>"$EVIDENCE/import.stderr"
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/import.exit"
if [[ "$rc" -ne 0 ]]; then
  printf '%s\n' "FAILED import $rc" >"$EVIDENCE/status.txt"
  exit "$rc"
fi
python3 - << 'PY'
import json
from pathlib import Path
receipt = json.loads(Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-69-jsonb-one/receipt.json").read_text())
need = {
    "outcome": "imported",
    "ready": False,
    "partial_ready": False,
    "rows": 1,
    "atttypid": 3802,
    "atttypmod": -1,
    "type_sql": "jsonb",
    "type_114": "json",
    "typmod_sqlstate": "22023",
    "send_matches": True,
    "catalog_state": "awaiting_publish",
    "model": "not_called",
    "source_engine": "mongodb",
}
for key, value in need.items():
    if receipt.get(key) != value:
        raise SystemExit(f"{key}={receipt.get(key)!r}")
if "weight" in receipt.get("document_text", "") or "notApproved" in receipt.get("document_text", ""):
    raise SystemExit("stored document leaked weight or a hidden field")
print("receipt-ok", receipt.get("install"))
PY
old_rows="$(docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c 'SELECT count(*) FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254')"
old_jsonb="$(docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c "SELECT count(*) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_type t ON t.oid=a.atttypid WHERE c.relname='pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254' AND t.typname='jsonb' AND a.attnum>0 AND NOT a.attisdropped")"
mig="$(docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c "SELECT version FROM _oo.schema_migrations WHERE version='full-import-jsonb/v1'")"
printf '%s\n' "retained_rows=$old_rows" "retained_jsonb_columns=$old_jsonb" "migration=$mig" | tee "$EVIDENCE/retained.txt"
[[ "$old_rows" == "16" && "$old_jsonb" == "0" && "$mig" == "full-import-jsonb/v1" ]]
printf '%s\n' DONE >"$EVIDENCE/status.txt"
