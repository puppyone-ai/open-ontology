#!/bin/bash
# Seal the captured shipment documents with native jsonb. Does not start the old mongod.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/docseal-80"
HEX="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe/full-approved.bson.hex"
CATALOG_SRC="$ROOT/evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/bson-double/seal/command.json"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
MONGO_IMAGE="mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c"
PG_NAME="oo-dl01w1-41-pg-docseal"
MONGO_NAME="oo-dl01w1-41-mongodb-docseal"
COPY_NAME="oo-dl01w1-41-docseal-copy"
VOLUME="oo-dl01w1-41-data-docseal"
ELF_VOLUME="oo-dl01w1-41-docseal-target"

if [[ -f "$EVIDENCE/baseline.seal.json" ]]; then
  echo "seal evidence already exists" >&2
  exit 2
fi
for name in "$PG_NAME" "$MONGO_NAME" "$COPY_NAME"; do
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    echo "named container already present: $name" >&2
    exit 2
  fi
done
if docker volume ls --format '{{.Name}}' | grep -qx "$VOLUME"; then
  echo "copy volume already present" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
cleanup() {
  docker rm -f "$PG_NAME" "$MONGO_NAME" "$COPY_NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

{
  echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)"
  echo "old-pg $(docker inspect -f '{{.State.Status}}' oo-dl01w1-41-pg)"
  echo "old-pg-cpu $(docker exec oo-dl01w1-41-pg cat /sys/fs/cgroup/cpu.max)"
} > "$EVIDENCE/resource-before.txt"

run_elf() {
  local name="$1"
  shift
  docker run --rm --name "$name" \
    --platform linux/arm64 --pull never --network none \
    --memory 4g --memory-swap 4g \
    -v "$ELF_VOLUME:/target:ro" \
    -v "$EVIDENCE:/evidence" \
    -v "$ROOT:/work:ro" \
    "$IMAGE" \
    "$@"
}

run_elf oo-dl01w1-41-docseal-decode \
  /target/debug/mongodb-preview --decode-docs --hex /work/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe/full-approved.bson.hex --out /evidence/baseline.decode.json
printf '%s\n' 0 > "$EVIDENCE/decode.exit"

python3 - "$CATALOG_SRC" "$EVIDENCE/catalog.json" <<'PY'
import json, sys
command = json.load(open(sys.argv[1]))
json.dump(command["catalog"], open(sys.argv[2], "w"))
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
docker exec "$PG_NAME" cat /sys/fs/cgroup/cpu.max > "$EVIDENCE/pg-docseal.cpu.max"
docker exec "$PG_NAME" psql -U postgres -Atc 'SHOW server_version' > "$EVIDENCE/pg-version.txt"

python3 - "$EVIDENCE" <<'PY'
import json, subprocess, sys
from pathlib import Path
evidence = Path(sys.argv[1])
decoded = json.loads((evidence / "baseline.decode.json").read_text())
rows = decoded["rows"]
assert len(rows) == 16
assert all(row["outcome"] == "decoded" for row in rows)

def pg_send(text):
    hx = text.encode().hex()
    sql = (
        "SELECT encode(b, 'hex'), convert_from(substring(b from 2), 'UTF8'), "
        "jsonb_typeof(doc->'weight'), doc#>>'{weight,$numberDouble}' "
        "FROM (SELECT convert_from(decode('" + hx + "', 'hex'), 'UTF8')::jsonb AS doc) s, "
        "LATERAL (SELECT jsonb_send(s.doc) AS b) j"
    )
    result = subprocess.run(
        ["docker", "exec", "-i", "oo-dl01w1-41-pg-docseal", "psql", "-U", "postgres", "-At", "-v", "ON_ERROR_STOP=1"],
        input=sql + "\n",
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr)
    line = result.stdout.strip()
    send_hex, send_text, typeof, token = line.split("|", 3)
    return send_hex, send_text, typeof, token

request_rows = []
type_rows = []
for row in rows:
    text = json.dumps(row["extended_json"], separators=(",", ":"), ensure_ascii=False)
    send_hex, send_text, typeof, token = pg_send(text)
    assert typeof == "object"
    assert token == row["weight"]
    request_rows.append({
        "name": f"baseline-{row['index']}",
        "raw_bson_hex": Path(
            "/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-64-fulldoc/probe/full-approved.bson.hex"
        ).read_text().splitlines()[row["index"]],
        "pg_jsonb_hex": send_hex,
        "pg_jsonb_text": send_text,
    })
    type_rows.append({
        "name": f"baseline-{row['index']}",
        "jsonb_typeof_weight": typeof,
        "number_double": token,
        "decoded_weight": row["weight"],
        "bare_number": False,
    })
(evidence / "baseline.seal-request.json").write_text(json.dumps({"rows": request_rows}))
(evidence / "baseline.types.json").write_text(json.dumps(type_rows, indent=2) + "\n")
PY

run_elf oo-dl01w1-41-docseal-seal \
  /target/debug/mongodb-preview --seal-docs --request /evidence/baseline.seal-request.json --out /evidence/baseline.seal.json
printf '%s\n' 0 > "$EVIDENCE/seal.exit"

python3 - "$EVIDENCE" "$ROOT" <<'PY'
import json, subprocess, sys
from pathlib import Path
evidence = Path(sys.argv[1])
root = Path(sys.argv[2])
sealed = json.loads((evidence / "baseline.seal.json").read_text())
assert all(row["outcome"] == "sealed" for row in sealed["rows"])
assert all(row["oid"] == 3802 and row["typmod"] == -1 for row in sealed["rows"])
assert all(row["source_acceptance"] is False and row["generation_created"] is False for row in sealed["rows"])
baseline = json.loads((root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79/baseline.json").read_text())
def weight_of(canonical):
    for line in canonical.split("\n"):
        if line.startswith("weight="):
            return line.split("=", 1)[1]
    raise SystemExit("weight missing")
pairs = []
for row, snap in zip(sealed["rows"], baseline["rows"], strict=True):
    assert row["weight"] == weight_of(snap["canonical"])
    assert row["extended_json"]["weight"]["$numberDouble"] == row["weight"]
    assert "blob_presence" not in row["extended_json"]
    pairs.append({"key": snap["key"], "weight": row["weight"], "sealed": True})
(evidence / "baseline.weight-compare.json").write_text(json.dumps(pairs, indent=2) + "\n")
request = json.loads((evidence / "baseline.seal-request.json").read_text())
envelope_hex = request["rows"][0]
# The seal request stores hex, not the envelope. Rebuild is done by the importer command from a saved envelope.
raw = bytes.fromhex(envelope_hex["raw_bson_hex"])
# envelope file is produced by the next elf call's input; write the three fields for --importer-request via a tiny JSON the binary accepts.
(evidence / "baseline-0.fields.json").write_text(json.dumps(envelope_hex))
print("compare-ok", len(pairs), "raw-bytes", len(raw))
PY

# One profile uses the customer catalog and the first sealed row's envelope.
python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
row = json.loads((evidence / "baseline.seal-request.json").read_text())["rows"][0]
# envelope is rebuilt by reading seal output identity; the profile command needs the envelope object.
# Reconstruct from the request fields plus the fixed header written below.
body = {
    "protocol": "oo.document.bson-jsonb/v1",
    "dataset": "oo_w141.shipments",
    "column": "doc",
    "oid": 3802,
    "typmod": -1,
    "source_type": "jsonb",
    "raw_bson_dropped": False,
    "raw_bson_hex": row["raw_bson_hex"],
    "pg_jsonb_hex": row["pg_jsonb_hex"],
    "pg_jsonb_text": row["pg_jsonb_text"],
    "loss_concealed": False,
    "missing_is_null": False,
    "approved_fields": ["_id","code","flag","units","price","created","uid","blob","blob_presence","weight","warehouse","whCode","lane","memo","memo_presence","marks","marks_presence","pack","pack_presence"],
}
import hashlib
body["raw_bson_sha256"] = hashlib.sha256(bytes.fromhex(row["raw_bson_hex"])).hexdigest()
(evidence / "baseline-0.envelope.json").write_text(json.dumps(body))
PY

set +e
run_elf oo-dl01w1-41-docseal-profile \
  /target/debug/mongodb-preview --profile-doc --catalog /evidence/catalog.json --envelope /evidence/baseline-0.envelope.json --out /evidence/baseline-0.profile.json
printf '%s\n' "$?" > "$EVIDENCE/profile.exit"
set -e
run_elf oo-dl01w1-41-docseal-importreq \
  /target/debug/mongodb-preview --importer-request --request /evidence/baseline-0.envelope.json --out /evidence/importer.request.json
set +e
docker run --rm --name oo-dl01w1-41-docseal-importer \
  --platform linux/arm64 --pull never --network none \
  --memory 2g --memory-swap 2g \
  -v /tmp/oo-dl01w1-41-elf-631720c5:/elf:ro \
  -v "$EVIDENCE:/evidence:ro" \
  "$IMAGE" \
  bash -c '/elf/oo-pipeline import-jsonb-document < /evidence/importer.request.json' \
  > "$EVIDENCE/importer.stdout" 2> "$EVIDENCE/importer.stderr"
printf '%s\n' "$?" > "$EVIDENCE/importer.exit"
set -e

# Mutate the first server document the way dba-a did, then seal must refuse hiddenNote.
python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
row = json.loads((evidence / "baseline.seal-request.json").read_text())["rows"][0]
(evidence / "mutate.request.json").write_text(json.dumps({
    "raw_bson_hex": row["raw_bson_hex"],
    "sets": [
        {"key": "memo", "kind": "string", "value": "memo-c5-period"},
        {"key": "hiddenNote", "kind": "string", "value": "not-approved-field"},
    ],
}))
PY
run_elf oo-dl01w1-41-docseal-mutate \
  /target/debug/mongodb-preview --mutate-set --request /evidence/mutate.request.json --out /evidence/mutate.json

# Current source bytes from a copy. The original container stays stopped.
set +e
docker volume create "$VOLUME" >/dev/null
docker run --rm --name "$COPY_NAME" --entrypoint cp \
  --platform linux/arm64 --pull never \
  -v oo-dl01w1-41-data:/from:ro \
  -v "$VOLUME:/to" \
  "$MONGO_IMAGE" -a /from/. /to/
copy_status=$?
printf '%s\n' "$copy_status" > "$EVIDENCE/copy.exit"
set -e
if [[ "$copy_status" -eq 0 ]]; then
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
  printf '%s\n' "$ready" > "$EVIDENCE/mongo-ready.exit"
  docker exec "$MONGO_NAME" cat /sys/fs/cgroup/cpu.max > "$EVIDENCE/mongo-docseal.cpu.max" || true
  if [[ "$ready" -eq 0 ]]; then
    docker cp "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/docseal-export.js" "$MONGO_NAME:/tmp/docseal-export.js"
    set +e
    docker exec "$MONGO_NAME" mongosh --quiet --file /tmp/docseal-export.js > "$EVIDENCE/cycle-b.export.txt" 2> "$EVIDENCE/cycle-b.export.stderr"
    printf '%s\n' $? > "$EVIDENCE/cycle-b.export.exit"
    set -e
  fi
  docker stop "$MONGO_NAME" >/dev/null
  docker inspect -f '{{.State.ExitCode}}' "$MONGO_NAME" > "$EVIDENCE/mongo-docseal.stop" || true
fi

python3 - "$EVIDENCE" "$ROOT" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
root = Path(sys.argv[2])
cycle = root / "evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79"
export = evidence / "cycle-b.export.txt"
summary = {"source_acceptance": False, "generation_created": False}
if not export.exists():
    summary["cycle_b"] = "not_read"
    (evidence / "cycle-b.summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    raise SystemExit(0)
rows = []
current = None
for line in export.read_text().splitlines():
    if line.startswith("ROW "):
        parts = line.split(" ", 2)
        current = {"key": parts[1], "keys": parts[2].split(",") if len(parts) > 2 else []}
    elif current is not None and line and not line.startswith("COUNT") and not line.startswith("READER"):
        current["hex"] = line
        rows.append(current)
        current = None
summary["cycle_b_rows"] = len(rows)
summary["hidden_keys"] = [row["key"] for row in rows if "hiddenNote" in row["keys"]]
summary["reader_dml"] = 0
(evidence / "cycle-b.rows.json").write_text(json.dumps(rows))
(evidence / "cycle-b.summary.json").write_text(json.dumps(summary, indent=2) + "\n")
if rows:
    (evidence / "cycle-b.hex").write_text("".join(row["hex"] + "\n" for row in rows))
PY
if [[ -f "$EVIDENCE/cycle-b.hex" ]]; then
  run_elf oo-dl01w1-41-docseal-cycleb \
    /target/debug/mongodb-preview --decode-docs --hex /evidence/cycle-b.hex --out /evidence/cycle-b.decode.json
fi
{
  echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)"
  echo "old-pg $(docker inspect -f '{{.State.Status}}' oo-dl01w1-41-pg)"
} > "$EVIDENCE/resource-after.txt"
echo "docseal-run-done"
