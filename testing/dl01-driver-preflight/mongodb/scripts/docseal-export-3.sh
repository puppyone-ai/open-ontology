#!/bin/bash
# Third read of the copied source. mongodump writes the server BSON.
# Does not start oo-dl01w1-41-mongodb and does not remove the earlier export failures.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/docseal-80"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
MONGO_IMAGE="mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c"
MONGO_NAME="oo-dl01w1-41-mongodb-docseal3"
PG_NAME="oo-dl01w1-41-pg-docseal3"
VOLUME="oo-dl01w1-41-data-docseal3"
ELF_VOLUME="oo-dl01w1-41-docseal-target"
CYCLE_B="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79/cycle-b.json"

if [[ ! -f "$EVIDENCE/cycle-b.export.txt" || ! -f "$EVIDENCE/cycle-b.export-2.txt" ]]; then
  echo "earlier export receipts are missing" >&2
  exit 2
fi
if [[ -f "$EVIDENCE/cycle-b.compare.json" ]]; then
  echo "cycle-b compare already exists" >&2
  exit 2
fi
for name in "$MONGO_NAME" "$PG_NAME" oo-dl01w1-41-docseal-copy3; do
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    echo "named container already present: $name" >&2
    exit 2
  fi
done
if [[ "$(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)" != "exited 0" ]]; then
  echo "retained mongod is not exited 0" >&2
  exit 2
fi

cleanup() {
  docker rm -f "$MONGO_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker volume create "$VOLUME" >/dev/null
docker run --rm --name oo-dl01w1-41-docseal-copy3 --entrypoint cp \
  --platform linux/arm64 --pull never \
  -v oo-dl01w1-41-data:/from:ro \
  -v "$VOLUME:/to" \
  "$MONGO_IMAGE" -a /from/. /to/
printf '%s\n' 0 > "$EVIDENCE/copy3.exit"
docker run -d --name "$MONGO_NAME" \
  --platform linux/arm64 --pull never \
  --memory 2g --memory-swap 2g \
  -v "$VOLUME:/data/db" \
  -v /tmp/oo-dl01w1-41-secrets:/run/secrets:ro \
  "$MONGO_IMAGE" \
  mongod --bind_ip 127.0.0.1 --port 27017 --auth --wiredTigerCacheSizeGB 0.25 --setParameter diagnosticDataCollectionEnabled=false >/dev/null
ready=1
for _ in $(seq 1 60); do
  if docker exec "$MONGO_NAME" mongosh --quiet --eval 'quit(db.adminCommand({ping:1}).ok===1?0:1)' >/dev/null 2>&1; then
    ready=0
    break
  fi
  sleep 1
done
printf '%s\n' "$ready" > "$EVIDENCE/mongo3-ready.exit"
docker exec "$MONGO_NAME" cat /sys/fs/cgroup/cpu.max > "$EVIDENCE/mongo3.cpu.max" || true
docker exec "$MONGO_NAME" mongodump --version > "$EVIDENCE/mongodump.version.txt" 2>&1 || true
if [[ "$ready" -ne 0 ]]; then
  echo "copy mongod did not answer ping" >&2
  exit 1
fi

docker cp "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/docseal-export-3.js" "$MONGO_NAME:/tmp/docseal-export-3.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO_NAME" \
  mongosh --quiet --norc "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-docseal-reader&maxPoolSize=1" \
  --file /tmp/docseal-export-3.js \
  > "$EVIDENCE/cycle-b.export-3.txt" 2> "$EVIDENCE/cycle-b.export-3.stderr"
printf '%s\n' "$?" > "$EVIDENCE/cycle-b.export-3.exit"
set -e
if [[ "$(tr -d '[:space:]' < "$EVIDENCE/cycle-b.export-3.exit")" != "0" ]]; then
  docker stop "$MONGO_NAME" >/dev/null
  echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)" > "$EVIDENCE/resource-export3.txt"
  echo "export-3 failed" >&2
  exit 1
fi

docker cp "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/docseal-dump-inside.sh" "$MONGO_NAME:/tmp/docseal-dump-inside.sh"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO_NAME" bash /tmp/docseal-dump-inside.sh \
  > "$EVIDENCE/cycle-b.dump-invoke.txt" 2> "$EVIDENCE/cycle-b.dump-invoke.stderr"
printf '%s\n' "$?" > "$EVIDENCE/cycle-b.dump.exit"
set -e
docker cp "$MONGO_NAME:/tmp/docseal-dump.stdout.redacted" "$EVIDENCE/cycle-b.dump.stdout" || true
docker cp "$MONGO_NAME:/tmp/docseal-dump.stderr.redacted" "$EVIDENCE/cycle-b.dump.stderr" || true
bson_inside=$(docker exec "$MONGO_NAME" bash -c 'find /tmp/docseal-dump -name shipments.bson -print -quit' | tr -d '\r\n' || true)
if [[ -n "$bson_inside" ]]; then
  docker cp "$MONGO_NAME:$bson_inside" "$EVIDENCE/cycle-b.shipments.bson"
fi
docker stop "$MONGO_NAME" >/dev/null
if [[ ! -s "$EVIDENCE/cycle-b.shipments.bson" ]]; then
  echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)" > "$EVIDENCE/resource-export3.txt"
  echo "mongodump produced no bson" >&2
  exit 1
fi

python3 - "$EVIDENCE" <<'PY'
import sys
from pathlib import Path
evidence = Path(sys.argv[1])
blob = (evidence / "cycle-b.shipments.bson").read_bytes()
docs = []
index = 0
while index < len(blob):
    if index + 4 > len(blob):
        raise SystemExit(f"truncated at {index}")
    size = int.from_bytes(blob[index:index + 4], "little", signed=True)
    if size < 5 or index + size > len(blob):
        raise SystemExit(f"bad bson length {size} at {index}")
    docs.append(blob[index:index + size])
    index += size

def object_id(doc):
    if len(doc) < 5 or doc[4] != 0x07:
        raise SystemExit("first field is not an object id")
    end = doc.index(0, 5)
    name = doc[5:end].decode("utf-8")
    if name != "_id":
        raise SystemExit("first field is " + name)
    return doc[end + 1:end + 13].hex()

docs.sort(key=object_id)
(evidence / "cycle-b.hex").write_text("".join(doc.hex() + "\n" for doc in docs))
print("split", len(docs))
PY

set +e
docker run --rm --name oo-dl01w1-41-docseal-cycleb3 \
  --platform linux/arm64 --pull never --network none \
  --memory 4g --memory-swap 4g \
  -v "$ELF_VOLUME:/target:ro" \
  -v "$EVIDENCE:/evidence" \
  "$IMAGE" \
  /target/debug/mongodb-preview --decode-docs --hex /evidence/cycle-b.hex --out /evidence/cycle-b.decode.json
printf '%s\n' "$?" > "$EVIDENCE/cycle-b.decode.exit"
set -e

python3 - "$EVIDENCE" "$CYCLE_B" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
canonical = json.loads(Path(sys.argv[2]).read_text())
decoded = json.loads((evidence / "cycle-b.decode.json").read_text())
export = (evidence / "cycle-b.export-3.txt").read_text().splitlines()
mismatches = []
rows = decoded.get("rows", [])
by_key = {}
for row in rows:
    oid = row.get("extended_json", {}).get("_id", {}).get("$oid")
    if oid:
        by_key[oid] = row
    else:
        mismatches.append("row without oid")
expected = {}
for snap in canonical["rows"]:
    fields = {}
    for line in snap["canonical"].split("\n"):
        if "=" in line:
            key, value = line.split("=", 1)
            fields[key] = value
    expected[snap["key"]] = fields
if set(by_key) != set(expected):
    mismatches.append({"keys": sorted(by_key), "canonical_keys": sorted(expected)})
deleted = "c141d04141eef0410000000f"
inserted = "c141d04141eef04100000010"
updated = "c141d04141eef04100000000"
temporary = "c141d04141eef04100000011"
if deleted in by_key:
    mismatches.append("deleted key present")
if temporary in by_key:
    mismatches.append("temporary key present")
if inserted not in by_key:
    mismatches.append("inserted key absent")
scope = []
sealed_candidates = []
for key, fields in expected.items():
    row = by_key.get(key)
    if row is None:
        continue
    if row.get("weight") != fields.get("weight"):
        mismatches.append({"key": key, "weight": row.get("weight"), "canonical": fields.get("weight")})
    if row.get("source_acceptance") is not False:
        mismatches.append({"key": key, "source_acceptance": True})
    code = row.get("extended_json", {}).get("code")
    if code != fields.get("code"):
        mismatches.append({"key": key, "code": code, "canonical_code": fields.get("code")})
    hidden = "hiddenNote" in row.get("keys", [])
    if hidden:
        scope.append(key)
        if row.get("outcome") != "scope_refused" or row.get("raw_doc_authorized") is not False:
            mismatches.append({"key": key, "outcome": row.get("outcome")})
        if "weight" not in row.get("extended_json", {}):
            mismatches.append({"key": key, "weight_dropped": True})
    else:
        if row.get("outcome") != "decoded":
            mismatches.append({"key": key, "outcome": row.get("outcome")})
        sealed_candidates.append(key)
    if fields.get("memo_presence") == "0" and "memo" in row.get("keys", []):
        mismatches.append({"key": key, "memo": "present while canonical presence is absent"})
if scope != [updated, inserted]:
    mismatches.append({"scope_keys": scope})
count_line = next((line for line in export if line.startswith("COUNT ")), "")
reader = next((line for line in export if line.startswith("READER_DML ")), "")
if count_line != "COUNT " + str(len(expected)):
    mismatches.append({"count_line": count_line})
if reader != "READER_DML 0":
    mismatches.append({"reader": reader})
body = {
    "protocol": "oo.mongodb-preview.docseal/v1",
    "rows": len(rows),
    "decoded": len(sealed_candidates),
    "scope_refused": scope,
    "deleted_absent": deleted not in by_key,
    "temporary_absent": temporary not in by_key,
    "inserted_present": inserted in by_key,
    "reader_dml_line": reader,
    "source_acceptance": False,
    "generation_created": False,
    "import_invoked": False,
    "mismatches": mismatches,
}
(evidence / "cycle-b.compare.json").write_text(json.dumps(body, indent=2) + "\n")
print("compare", "ok" if not mismatches else "mismatch", len(rows))
if mismatches:
    raise SystemExit(1)
PY

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 --pull never \
  --memory 2g --memory-swap 2g \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_PASSWORD=local \
  "$PG_IMAGE" >/dev/null
for _ in $(seq 1 60); do
  if docker exec "$PG_NAME" pg_isready -q; then
    break
  fi
  sleep 1
done
docker exec "$PG_NAME" pg_isready
docker exec "$PG_NAME" cat /sys/fs/cgroup/cpu.max > "$EVIDENCE/pg-docseal3.cpu.max" || true
docker exec "$PG_NAME" postgres --version > "$EVIDENCE/pg-docseal3.version.txt"

python3 - "$EVIDENCE" <<'PY'
import json, subprocess, sys
from pathlib import Path
evidence = Path(sys.argv[1])
decoded = json.loads((evidence / "cycle-b.decode.json").read_text())
hex_lines = (evidence / "cycle-b.hex").read_text().splitlines()

def pg_send(text):
    hx = text.encode().hex()
    sql = (
        "SELECT encode(b, 'hex'), convert_from(substring(b from 2), 'UTF8'), "
        "jsonb_typeof(doc->'weight'), doc#>>'{weight,$numberDouble}' "
        "FROM (SELECT convert_from(decode('" + hx + "', 'hex'), 'UTF8')::jsonb AS doc) s, "
        "LATERAL (SELECT jsonb_send(s.doc) AS b) j"
    )
    result = subprocess.run(
        ["docker", "exec", "-i", "oo-dl01w1-41-pg-docseal3", "psql", "-U", "postgres", "-At", "-v", "ON_ERROR_STOP=1"],
        input=sql + "\n", text=True, capture_output=True,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr)
    send_hex, send_text, typeof, token = result.stdout.strip().split("|", 3)
    return send_hex, send_text, typeof, token

in_scope = []
hidden = []
types = []
for row, raw_hex in zip(decoded["rows"], hex_lines, strict=True):
    text = json.dumps(row["extended_json"], separators=(",", ":"), ensure_ascii=False)
    send_hex, send_text, typeof, token = pg_send(text)
    oid = row["extended_json"]["_id"]["$oid"]
    if typeof != "object" or token != row["weight"]:
        raise SystemExit(f"weight type {oid} {typeof} {token}")
    item = {
        "name": "cycle-b-" + oid,
        "raw_bson_hex": raw_hex,
        "pg_jsonb_hex": send_hex,
        "pg_jsonb_text": send_text,
    }
    types.append({
        "name": item["name"],
        "outcome": row["outcome"],
        "jsonb_typeof_weight": typeof,
        "number_double": token,
        "decoded_weight": row["weight"],
        "bare_number": False,
    })
    if row["outcome"] == "decoded":
        in_scope.append(item)
    elif row["outcome"] == "scope_refused":
        hidden.append(item)
    else:
        raise SystemExit("unexpected outcome " + row["outcome"])
(evidence / "cycle-b.seal-request.json").write_text(json.dumps({"rows": in_scope}))
(evidence / "cycle-b.hidden.seal-request.json").write_text(json.dumps({"rows": hidden}))
(evidence / "cycle-b.types.json").write_text(json.dumps(types, indent=2) + "\n")
print("pg-send", len(in_scope), "hidden", len(hidden))
PY

seal_one() {
  local name="$1"
  local request="$2"
  local out="$3"
  local exit_file="$4"
  set +e
  docker run --rm --name "$name" \
    --platform linux/arm64 --pull never --network none \
    --memory 4g --memory-swap 4g \
    -v "$ELF_VOLUME:/target:ro" \
    -v "$EVIDENCE:/evidence" \
    "$IMAGE" \
    /target/debug/mongodb-preview --seal-docs --request "$request" --out "$out"
  printf '%s\n' "$?" > "$exit_file"
  set -e
}
seal_one oo-dl01w1-41-docseal-cycleb-seal /evidence/cycle-b.seal-request.json /evidence/cycle-b.seal.json "$EVIDENCE/cycle-b.seal.exit"
seal_one oo-dl01w1-41-docseal-cycleb-hidden /evidence/cycle-b.hidden.seal-request.json /evidence/cycle-b.hidden.seal.json "$EVIDENCE/cycle-b.hidden.seal.exit"

python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
sealed = json.loads((evidence / "cycle-b.seal.json").read_text())
hidden = json.loads((evidence / "cycle-b.hidden.seal.json").read_text())
if any(row.get("outcome") != "sealed" for row in sealed["rows"]):
    raise SystemExit("in-scope seal refused")
if any(row.get("oid") != 3802 or row.get("typmod") != -1 for row in sealed["rows"]):
    raise SystemExit("oid")
if any(row.get("source_acceptance") is not False or row.get("generation_created") is not False for row in sealed["rows"]):
    raise SystemExit("acceptance")
if any(row.get("extended_json", {}).get("weight", {}).get("$numberDouble") != row.get("weight") for row in sealed["rows"]):
    raise SystemExit("sealed weight")
if len(hidden["rows"]) != 2 or any(row.get("code") != "oo.document.bson-jsonb/v1/field.scope" for row in hidden["rows"]):
    raise SystemExit("hidden seal")
if any(row.get("raw_doc_authorized") is not False or row.get("source_acceptance") is not False for row in hidden["rows"]):
    raise SystemExit("hidden authorization")
print("seal-ok", len(sealed["rows"]), "hidden-refused", len(hidden["rows"]))
PY

{
  echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)"
  echo "old-pg $(docker inspect -f '{{.State.Status}}' oo-dl01w1-41-pg)"
} > "$EVIDENCE/resource-export3.txt"
echo "export-3-done"
