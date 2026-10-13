#!/bin/bash
# Install native sealed-catalog migration 0008, then read the generation-2 import back.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
IMPORT="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-73-sealed-catalog"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
ELF="/tmp/oo-dl01w1-41-elf-631720c5"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMISSION="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission"
RECEIPT="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain/import-batch/native-readonly-receipt.json"
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
python3 - << 'PY'
import json
from pathlib import Path
root = Path("/Users/redux/puppy-one/open-ontology")
imp = root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
out = root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-73-sealed-catalog"
digests = json.loads((imp / "plan/digests.json").read_text())
receipt = json.loads((imp / "receipt.json").read_text())
if receipt.get("load_state") != "awaiting_publish" or receipt.get("run_id") != "ad8b3022-7730-4d5c-9218-ca68bd26b561":
    raise SystemExit("import receipt is not the generation-2 run")
out.mkdir()
install = {
    "protocol": "oo.pipeline.install-native-sealed-catalog/v1",
    "target": {
        "host": "oo-dl01w1-41-pg",
        "port": 5432,
        "database": "oo_w141",
        "administrator": "oo_w141_admin",
        "password_file": "/run/secrets/admin-password",
        "ca_file": "/run/secrets/ca.crt",
    },
}
catalog = {
    "protocol": "oo.pipeline.native-sealed-catalog/v1",
    "run_id": receipt["run_id"],
    "source_id": "oo_w141",
    "owner": "23df6145-4592-4dc0-82df-fd226dd020fd",
    "source_generation": 2,
    "target_generation": 2,
    "config_digest": digests["config_digest"],
    "schema_digest": digests["schema_digest"],
    "type_digest": digests["type_digest"],
    "input_digest": digests["input_digest"],
    "snapshot": {"kind": "token", "value": "oo-dl01w1-41-doc3802"},
    "target": {
        "host": "oo-dl01w1-41-pg",
        "port": 5432,
        "database": "oo_w141",
        "runtime_principal": "oo_w141_admin",
        "password_file": "/run/secrets/admin-password",
        "ca_file": "/run/secrets/ca.crt",
    },
}
(out / "install-request.json").write_text(json.dumps(install, indent=2) + "\n")
(out / "catalog-request.json").write_text(json.dumps(catalog, indent=2) + "\n")
(out / "registry.sha256").write_text((imp / "registry.sha256").read_text())
print("request-ready")
PY
registry_sha="$(tr -d '\n' < "$IMPORT/registry.sha256")"
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
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -v "$ELF:/elf:ro" \
  -v "$ADMISSION:/admission/oo-source-admission:ro" \
  -v "$RECEIPT:/frozen/native-readonly-receipt.json:ro" \
  -v "$SECRETS/admin-password:/run/secrets/admin-password:ro" \
  -v "$SECRETS/ca.crt:/run/secrets/ca.crt:ro" \
  -v "$IMPORT:/evidence:ro" \
  -v "$EVIDENCE:/request" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash -c 'set -eu; /elf/oo-pipeline install-native-sealed-catalog < /request/install-request.json > /request/install-receipt.json 2>/request/install.stderr; printf "%s\n" "$?" > /request/install.exit; /elf/oo-pipeline catalog-import-native < /request/catalog-request.json > /request/sealed-catalog.json 2>/request/catalog.stderr; printf "%s\n" "$?" > /request/catalog.exit; if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi; test "$(cat /request/install.exit)" = 0; test "$(cat /request/catalog.exit)" = 0'
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
if [[ "$rc" -ne 0 ]]; then
  printf 'FAILED catalog %s\n' "$rc" >"$EVIDENCE/status.txt"
  exit "$rc"
fi
python3 - << 'PY'
import json
from pathlib import Path
out = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-73-sealed-catalog")
install = json.loads((out / "install-receipt.json").read_text())
catalog = json.loads((out / "sealed-catalog.json").read_text())
if catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1":
    raise SystemExit("catalog protocol")
if catalog.get("state") != "awaiting_publish" or catalog.get("capture") != "sealed_import":
    raise SystemExit(f"catalog state {catalog.get('state')} capture {catalog.get('capture')}")
if catalog.get("ready") is True or catalog.get("partial_ready") is True:
    raise SystemExit("catalog claims ready")
tables = catalog.get("tables") or []
if len(tables) != 1 or tables[0].get("accepted_rows") != 16:
    raise SystemExit("catalog rows")
doc = [item for item in tables[0].get("columns", []) if item.get("logical_name") == "doc"]
if len(doc) != 1 or doc[0].get("oid") != 3802 or doc[0].get("typmod") != -1 or doc[0].get("source_type") != "jsonb":
    raise SystemExit(f"doc column {doc}")
print("catalog-ok", install.get("outcome"), catalog.get("run_id"), tables[0].get("stored_physical"))
PY
retained="$(docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c 'SELECT count(*) FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254')"
newrows="$(docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c 'SELECT count(*) FROM _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898')"
printf '%s\n' "retained_rows=$retained" "new_rows=$newrows" >"$EVIDENCE/post-pg.txt"
[[ "$retained" == "16" && "$newrows" == "16" ]]
printf '%s\n' DONE >"$EVIDENCE/status.txt"
