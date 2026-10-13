#!/bin/bash
# Compile the document profile for the generation-2 sealed catalog, then ask
# source-semantics for the role page. The 20-column document-profile pack path
# is recorded separately and is not treated as the role page.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
CATALOG="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-74-sealed-catalog/sealed-catalog.json"
SEAL="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-67-canonical/envelopes/seal-00.json"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-76-profile"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
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
if [[ ! -x "$ELF/pg-builder" || ! -x "$ELF/source-semantics" ]]; then
  echo "fixed profile elf is absent" >&2
  exit 2
fi

mkdir "$EVIDENCE"
cp "$SEAL" "$EVIDENCE/document.json"
python3 - << 'PY'
import json
from pathlib import Path
out = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-76-profile")
doc = json.loads((out / "document.json").read_text())
if doc.get("dataset") != "oo_w141.shipments" or doc.get("column") != "doc":
    raise SystemExit("seal envelope is not the shipments document")
if doc.get("oid") != 3802 or doc.get("typmod") != -1:
    raise SystemExit("seal envelope is not jsonb 3802")
text = doc.get("pg_jsonb_text", "")
if "weight" in text or "notApproved" in text or "price_odd" in text or "mixed_marks" in text:
    raise SystemExit("seal text contains a hidden or float field")
print("document-ready", doc["raw_bson_sha256"])
PY

docker image inspect "$RUST_IMAGE" >/dev/null
python3 "$ADMIT" acquire rust "$EVIDENCE/admission.json"
cleanup() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

run_elf() {
  local name="$1"
  shift
  set +e
  docker run --name "$RUST" \
    --platform linux/arm64 --pull never \
    --memory 2g --memory-swap 2g --cpus 1 \
    --pids-limit 256 \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -v "$ELF:/elf:ro" \
    -v "$CATALOG:/catalog/sealed-catalog.json:ro" \
    -v "$EVIDENCE:/request" \
    "${LABELS[@]}" \
    "$RUST_IMAGE" \
    "$@"
  local rc=$?
  set -e
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  printf '%s\n' "$rc" >"$EVIDENCE/docker-$name.exit"
  return 0
}

run_elf profile bash -c 'set +e
/elf/pg-builder document-profile --catalog /catalog/sealed-catalog.json --document /request/document.json --out /request/profile.json > /request/profile.stdout 2> /request/profile.stderr
printf "%s\n" "$?" > /request/profile.exit
if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi
test "$(tr -d "\n" < /request/profile.exit)" = 0'
if [[ "$(tr -d '\n' < "$EVIDENCE/profile.exit")" != "0" ]]; then
  printf '%s\n' 'FAILED profile' >"$EVIDENCE/status.txt"
  exit 1
fi

python3 - << 'PY'
import json
from pathlib import Path
import hashlib
out = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-76-profile")
profile = json.loads((out / "profile.json").read_text())
doc = json.loads((out / "document.json").read_text())
pg_hash = hashlib.sha256(bytes.fromhex(doc["pg_jsonb_hex"])).hexdigest()
catalog = json.loads(Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-74-sealed-catalog/sealed-catalog.json").read_text())
if profile.get("outcome") != "compiled" or profile.get("ready") is not False or profile.get("partial_ready") is not False:
    raise SystemExit("profile is not a non-ready compile")
if profile.get("model") != "not_called":
    raise SystemExit("profile called a model")
if profile.get("dataset") != "oo_w141.shipments" or profile.get("column") != "doc":
    raise SystemExit("profile dataset")
if profile.get("raw_bson_sha256") != doc["raw_bson_sha256"] or profile.get("pg_jsonb_sha256") != pg_hash:
    raise SystemExit("profile hash does not match the seal envelope")
local = profile.get("document")
if not isinstance(local, str) or "weight" in local or "notApproved" in local or "price_odd" in local or "mixed_marks" in local:
    raise SystemExit("profile document leaks a hidden field")
identity = profile.get("catalog_identity_hash")
if not isinstance(identity, str) or len(identity) != 64:
    raise SystemExit("catalog identity missing")
columns = catalog["tables"][0]["columns"]
semantic = {
    "_id": "identifier",
    "code": "code",
    "flag": "quantity",
    "units": "quantity",
    "price": "quantity",
    "created": "event_time",
    "uid": "identifier",
    "blob": "system",
    "blob_presence": "system",
    "weight": "quantity",
    "warehouse": "code",
    "q_7768436f6465": "code",
    "lane": "json",
    "memo": "free_text",
    "memo_presence": "system",
    "marks": "json",
    "marks_presence": "system",
    "pack": "json",
    "pack_presence": "system",
    "doc": "json",
}
names = [item["logical_name"] for item in columns]
if set(names) != set(semantic):
    raise SystemExit(f"catalog columns changed: {names}")
decisions = []
for name in names:
    decisions.append({
        "dataset": "oo_w141.shipments",
        "column": name,
        "pii": "none",
        "semantic_type": semantic[name],
        "reviewer": {"by": "dl01-w1-41", "at": "2026-10-12"},
    })
settings = {
    "project": "oo_w141",
    "ontology_version": "ont-1",
    "catalog_version": identity,
    "policy": {"id": "pol_w141", "version": "1"},
    "knowledge": {"id": "know_w141", "version": "1"},
    "ontology": {
        "spec_version": "oos/v1",
        "name": "oo_w141",
        "locale": "en-US",
        "include_schemas": ["oo_w141"],
        "default_tier_preference": ["raw"],
        "publish": {"include_unreviewed": []},
        "classification": {"auto_accept": "rule_high_confidence"},
        "lifecycle": {"deprecation_min_releases": 2},
        "compile": {
            "per_role": True,
            "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
            "sample_values": {"default": "masked", "max_per_column": 0},
        },
    },
}
visibility = {
    "protocol": "oo.builder.role-visibility/v1",
    "project": "oo_w141",
    "principal": "oo_w141",
    "role": "oo_r_w141_reader",
    "ontology_version": "ont-1",
    "catalog_version": identity,
    "policy": {"id": "pol_w141", "version": "1"},
    "knowledge": {"id": "know_w141", "version": "1"},
    "issued_for_compiler": "oo-builder/role-pack/v1",
    "datasets": [{
        "dataset": "oo_w141.shipments",
        "row_security": "none",
        "global_stats": "denied",
        "columns": names,
    }],
    "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128},
}
command = {
    "protocol": "oo.source-semantics.command/v1",
    "catalog": catalog,
    "target": {
        "family": "mongodb",
        "product": "mongodb",
        "version": "8.0.32",
        "capture": "sealed_import",
        "source_consistency": "B",
        "delete_detection": "reconcile",
        "source_kind": "mongodb_collection",
        "cursor": None,
        "chunk": {"key_kind": "binary", "binary_order": None, "hash_chunking": False},
        "flags": {
            "synthetic_binlog": False,
            "append_only": False,
            "eventual_consistency": False,
            "default_for_missing": False,
            "accent_insensitive": False,
            "soft_delete_hint": False,
        },
        "columns": [],
        "constraints": [],
    },
    "binding": {
        "settings": settings,
        "decisions": decisions,
        "visibility": visibility,
        "examples": [],
    },
    "document": doc,
}
(out / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")
(out / "visibility.json").write_text(json.dumps(visibility, indent=2) + "\n")
(out / "command.json").write_text(json.dumps(command) + "\n")
print("command-ready", identity)
PY

run_elf semantics bash -c 'set +e
/elf/source-semantics < /request/command.json > /request/semantics.json 2> /request/semantics.stderr
printf "%s\n" "$?" > /request/semantics.exit
/elf/pg-builder document-profile --catalog /catalog/sealed-catalog.json --document /request/document.json --settings /request/settings.json --visibility /request/visibility.json --reviewer dl01-w1-41 --reviewed-at 2026-10-12 --out /request/profile-pack.json --out-pack /request/profile-pack-page.md > /request/profile-pack.stdout 2> /request/profile-pack.stderr
printf "%s\n" "$?" > /request/profile-pack.exit
if [ -n "${HOST_UID:-}" ]; then chown -R "${HOST_UID}:${HOST_GID}" /request || true; fi
exit 0'

python3 - << 'PY'
import json
from pathlib import Path
out = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-76-profile")
sem_exit = (out / "semantics.exit").read_text().strip()
pack_exit = (out / "profile-pack.exit").read_text().strip()
pack_err = (out / "profile-pack.stderr").read_text().strip()
result = json.loads((out / "semantics.json").read_text())
status = result.get("status")
proof = (result.get("catalog_proof") or {}).get("status")
page = ""
pages = (result.get("pack") or {}).get("pages") if isinstance(result.get("pack"), dict) else None
if isinstance(pages, dict):
    page = pages.get("tables/oo_w141.shipments.md") or ""
doc = json.loads((out / "document.json").read_text())
profile = json.loads((out / "profile.json").read_text())
profile_pg = profile["pg_jsonb_sha256"]
lines = [
    f"semantics_exit={sem_exit}",
    f"semantics_status={status}",
    f"catalog_proof={proof}",
    f"profile_pack_exit={pack_exit}",
    f"profile_pack_stderr={pack_err}",
    f"page_has_document_storage={'document_storage: jsonb' in page}",
    f"page_has_oid={'document_oid: 3802' in page}",
    f"page_has_raw={doc['raw_bson_sha256'] in page}",
    f"page_has_pg={profile_pg in page}",
]
(out / "check.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
if status not in {"packed", "blocked"} or proof not in {"accepted", "refused"}:
    raise SystemExit("semantics result has no closed status")
if "notApproved" in page or "price_odd" in page or "mixed_marks" in page:
    raise SystemExit("role page leaks a hidden field")
PY
printf '%s\n' RECORDED >"$EVIDENCE/status.txt"
