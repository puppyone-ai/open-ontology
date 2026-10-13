#!/bin/bash
# Install sealed-import-catalog/v1 (0006), then native-sealed-catalog/v1 (0008),
# then read the generation-2 import. repair-73 stays the refused 0008 attempt.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
IMPORT="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-74-sealed-catalog"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
ELF="/tmp/oo-dl01w1-41-elf-631720c5"
SECRETS="/tmp/oo-dl01w1-41-secrets"
ADMISSION="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/admission-1/compile-5df/bin/oo-source-admission"
RECEIPT="$ROOT/evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/live-chain/import-batch/native-readonly-receipt.json"
PG="oo-dl01w1-41-pg"
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
if [[ ! -x "$ELF/oo-pipeline" ]]; then
  echo "fixed pipeline elf is absent" >&2
  exit 2
fi

python3 - << 'PY'
import json
from pathlib import Path
root = Path("/Users/redux/puppy-one/open-ontology")
imp = root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-71-native-import"
out = root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-74-sealed-catalog"
digests = json.loads((imp / "plan/digests.json").read_text())
receipt = json.loads((imp / "receipt.json").read_text())
if receipt.get("load_state") != "awaiting_publish" or receipt.get("run_id") != "ad8b3022-7730-4d5c-9218-ca68bd26b561":
    raise SystemExit("import receipt is not the generation-2 run")
out.mkdir()
target_admin = {
    "host": "oo-dl01w1-41-pg",
    "port": 5432,
    "database": "oo_w141",
    "administrator": "oo_w141_admin",
    "password_file": "/run/secrets/admin-password",
    "ca_file": "/run/secrets/ca.crt",
}
sealed_install = {
    "protocol": "oo.pipeline.install-sealed-catalog/v1",
    "target": target_admin,
}
native_install = {
    "protocol": "oo.pipeline.install-native-sealed-catalog/v1",
    "target": target_admin,
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
(out / "sealed-install-request.json").write_text(json.dumps(sealed_install, indent=2) + "\n")
(out / "install-request.json").write_text(json.dumps(native_install, indent=2) + "\n")
(out / "catalog-request.json").write_text(json.dumps(catalog, indent=2) + "\n")
(out / "registry.sha256").write_text((imp / "registry.sha256").read_text())
(out / "cause.txt").write_text(
    "repair-73 install-native-sealed-catalog returned target catalog refused.\n"
    "0008 requires an exact sealed-import-catalog/v1 row. That version was absent.\n"
    "This run applies 0006 with install-sealed-catalog, then 0008, then catalog-import-native.\n"
)
print("request-ready")
PY

{
  printf '%s\n' 'migrations:'
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT version FROM _oo.schema_migrations ORDER BY version"
  printf '%s\n' 'counts:'
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'retained=' || count(*)::text FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254"
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'retained_jsonb=' || count(*)::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname='pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254' AND a.atttypid=3802 AND a.attnum>0 AND NOT a.attisdropped"
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'new=' || count(*)::text FROM _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898"
} >"$EVIDENCE/pre-pg.txt"

if grep -qx 'sealed-import-catalog/v1' "$EVIDENCE/pre-pg.txt"; then
  echo "sealed catalog migration already present; refusing to continue" >&2
  printf '%s\n' 'FAILED sealed version already present' >"$EVIDENCE/status.txt"
  exit 2
fi
grep -qx 'retained=16' "$EVIDENCE/pre-pg.txt"
grep -qx 'retained_jsonb=0' "$EVIDENCE/pre-pg.txt"
grep -qx 'new=16' "$EVIDENCE/pre-pg.txt"

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
  bash -c 'set +e
/elf/oo-pipeline install-sealed-catalog < /request/sealed-install-request.json > /request/sealed-install-receipt.json 2>/request/sealed-install.stderr
printf "%s\n" "$?" > /request/sealed-install.exit
if [ "$(tr -d "\n" < /request/sealed-install.exit)" != "0" ]; then
  if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi
  exit 1
fi
/elf/oo-pipeline install-native-sealed-catalog < /request/install-request.json > /request/install-receipt.json 2>/request/install.stderr
printf "%s\n" "$?" > /request/install.exit
if [ "$(tr -d "\n" < /request/install.exit)" != "0" ]; then
  if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi
  exit 1
fi
/elf/oo-pipeline catalog-import-native < /request/catalog-request.json > /request/sealed-catalog.json 2>/request/catalog.stderr
printf "%s\n" "$?" > /request/catalog.exit
if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi
test "$(tr -d "\n" < /request/catalog.exit)" = 0'
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
out = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-74-sealed-catalog")
sealed = json.loads((out / "sealed-install-receipt.json").read_text())
native = json.loads((out / "install-receipt.json").read_text())
catalog = json.loads((out / "sealed-catalog.json").read_text())
if sealed.get("protocol") != "oo.pipeline.install-sealed-catalog-result/v1" or sealed.get("outcome") != "installed":
    raise SystemExit(f"sealed install {sealed}")
if native.get("protocol") != "oo.pipeline.install-native-sealed-catalog-result/v1" or native.get("outcome") != "installed":
    raise SystemExit(f"native install {native}")
if catalog.get("protocol") != "oo.pipeline.sealed-import-catalog/v1":
    raise SystemExit("catalog protocol")
if catalog.get("state") != "awaiting_publish" or catalog.get("capture") != "sealed_import":
    raise SystemExit(f"catalog state {catalog.get('state')} capture {catalog.get('capture')}")
if catalog.get("engine") != "mongodb":
    raise SystemExit(f"engine {catalog.get('engine')}")
if catalog.get("ready") is True or catalog.get("partial_ready") is True:
    raise SystemExit("catalog claims ready")
text = (out / "sealed-catalog.json").read_text()
if "secret://" in text:
    raise SystemExit("secret ref in catalog")
tables = catalog.get("tables") or []
if len(tables) != 1 or tables[0].get("accepted_rows") != 16:
    raise SystemExit("catalog rows")
doc = [item for item in tables[0].get("columns", []) if item.get("logical_name") == "doc"]
if len(doc) != 1 or doc[0].get("oid") != 3802 or doc[0].get("typmod") != -1 or doc[0].get("source_type") != "jsonb":
    raise SystemExit(f"doc column {doc}")
print("catalog-ok", sealed.get("outcome"), native.get("outcome"), catalog.get("run_id"), tables[0].get("stored_physical"))
PY

{
  printf '%s\n' 'migrations:'
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT version FROM _oo.schema_migrations ORDER BY version"
  printf '%s\n' 'counts:'
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'retained=' || count(*)::text FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254"
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'retained_jsonb=' || count(*)::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname='pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254' AND a.atttypid=3802 AND a.attnum>0 AND NOT a.attisdropped"
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'new=' || count(*)::text FROM _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898"
  docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
    "SELECT 'new_doc=' || a.atttypid::text || ':' || a.atttypmod::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname='pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898' AND a.attname='pc_ef107119f2cd14e89c719da1d0f8d86dcb2483e797325259750ab16b7d1f'"
} >"$EVIDENCE/post-pg.txt"
grep -qx 'sealed-import-catalog/v1' "$EVIDENCE/post-pg.txt"
grep -qx 'native-sealed-catalog/v1' "$EVIDENCE/post-pg.txt"
grep -qx 'retained=16' "$EVIDENCE/post-pg.txt"
grep -qx 'retained_jsonb=0' "$EVIDENCE/post-pg.txt"
grep -qx 'new=16' "$EVIDENCE/post-pg.txt"
grep -qx 'new_doc=3802:-1' "$EVIDENCE/post-pg.txt"
printf '%s\n' DONE >"$EVIDENCE/status.txt"
