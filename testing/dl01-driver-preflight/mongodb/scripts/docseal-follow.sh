#!/bin/bash
# Second read of the copied source, plus the frozen importer's next error.
# Does not start oo-dl01w1-41-mongodb and does not remove the first export failure.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/docseal-80"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
PG_IMAGE="docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
MONGO_IMAGE="mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c"
MONGO_NAME="oo-dl01w1-41-mongodb-docseal2"
PG_NAME="oo-dl01w1-41-pg-docseal2"
VOLUME="oo-dl01w1-41-data-docseal2"
ELF_VOLUME="oo-dl01w1-41-docseal-target"

if [[ ! -f "$EVIDENCE/cycle-b.export.txt" ]]; then
  echo "first export receipt is missing" >&2
  exit 2
fi
if [[ -f "$EVIDENCE/cycle-b.decode.json" ]]; then
  echo "cycle-b decode already exists" >&2
  exit 2
fi
for name in "$MONGO_NAME" "$PG_NAME"; do
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    echo "named container already present: $name" >&2
    exit 2
  fi
done
cleanup() {
  docker rm -f "$MONGO_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker volume create "$VOLUME" >/dev/null
docker run --rm --name oo-dl01w1-41-docseal-copy2 --entrypoint cp \
  --platform linux/arm64 --pull never \
  -v oo-dl01w1-41-data:/from:ro \
  -v "$VOLUME:/to" \
  "$MONGO_IMAGE" -a /from/. /to/
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
printf '%s\n' "$ready" > "$EVIDENCE/mongo2-ready.exit"
docker exec "$MONGO_NAME" cat /sys/fs/cgroup/cpu.max > "$EVIDENCE/mongo2.cpu.max" || true
if [[ "$ready" -ne 0 ]]; then
  echo "copy mongod did not answer ping" >&2
  exit 1
fi
docker cp "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/docseal-export-2.js" "$MONGO_NAME:/tmp/docseal-export-2.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO_NAME" \
  mongosh --quiet --norc "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-docseal-reader&maxPoolSize=1" \
  --file /tmp/docseal-export-2.js \
  > "$EVIDENCE/cycle-b.export-2.txt" 2> "$EVIDENCE/cycle-b.export-2.stderr"
printf '%s\n' "$?" > "$EVIDENCE/cycle-b.export-2.exit"
set -e
docker stop "$MONGO_NAME" >/dev/null

python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
rows = []
current = None
for line in (evidence / "cycle-b.export-2.txt").read_text().splitlines():
    if line.startswith("ROW "):
        parts = line.split(" ", 2)
        current = {"key": parts[1], "keys": parts[2].split(",") if len(parts) > 2 else []}
    elif current is not None and line and not line.startswith(("COUNT", "READER", "REQUIRE", "FAIL")):
        current["hex"] = line
        rows.append(current)
        current = None
(evidence / "cycle-b.rows.json").write_text(json.dumps([{k: row[k] for k in ("key", "keys")} for row in rows], indent=2) + "\n")
if rows:
    (evidence / "cycle-b.hex").write_text("".join(row["hex"] + "\n" for row in rows))
print("parsed", len(rows))
PY

if [[ -f "$EVIDENCE/cycle-b.hex" ]]; then
  docker run --rm --name oo-dl01w1-41-docseal-cycleb \
    --platform linux/arm64 --pull never --network none \
    --memory 4g --memory-swap 4g \
    -v "$ELF_VOLUME:/target:ro" \
    -v "$EVIDENCE:/evidence" \
    "$IMAGE" \
    /target/debug/mongodb-preview --decode-docs --hex /evidence/cycle-b.hex --out /evidence/cycle-b.decode.json
fi

python3 - "$EVIDENCE" <<'PY'
import json, sys
from pathlib import Path
evidence = Path(sys.argv[1])
envelope = json.loads((evidence / "baseline-0.envelope.json").read_text())
envelope.pop("approved_fields", None)
(evidence / "importer-noscope.request.json").write_text(json.dumps({
    "protocol": "oo.pipeline.jsonb-document-import/v1",
    "engine": "mongodb",
    "source_id": "oo_w141",
    "catalog": json.loads((evidence / "importer.request.json").read_text())["catalog"],
    "document": envelope,
}))
print("noscope-request")
PY
set +e
docker run --rm --name oo-dl01w1-41-docseal-importer2 \
  --platform linux/arm64 --pull never --network none \
  --memory 2g --memory-swap 2g \
  -v /tmp/oo-dl01w1-41-elf-631720c5:/elf:ro \
  -v "$EVIDENCE:/evidence:ro" \
  "$IMAGE" \
  bash -c '/elf/oo-pipeline import-jsonb-document < /evidence/importer-noscope.request.json' \
  > "$EVIDENCE/importer-noscope.stdout" 2> "$EVIDENCE/importer-noscope.stderr"
printf '%s\n' "$?" > "$EVIDENCE/importer-noscope.exit"
set -e
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
python3 - "$EVIDENCE" <<'PY'
import json, subprocess, sys
from pathlib import Path
evidence = Path(sys.argv[1])
mutated = json.loads((evidence / "mutate.json").read_text())
extended = mutated["decoded"]["extended_json"]
text = json.dumps(extended, separators=(",", ":"), ensure_ascii=False)
hx = text.encode().hex()
sql = (
    "SELECT encode(b, 'hex'), convert_from(substring(b from 2), 'UTF8'), jsonb_typeof(doc->'weight') "
    "FROM (SELECT convert_from(decode('" + hx + "', 'hex'), 'UTF8')::jsonb AS doc) s, "
    "LATERAL (SELECT jsonb_send(s.doc) AS b) j"
)
result = subprocess.run(
    ["docker", "exec", "-i", "oo-dl01w1-41-pg-docseal2", "psql", "-U", "postgres", "-At", "-v", "ON_ERROR_STOP=1"],
    input=sql + "\n", text=True, capture_output=True,
)
if result.returncode != 0:
    raise SystemExit(result.stderr)
send_hex, send_text, typeof = result.stdout.strip().split("|", 2)
assert typeof == "object"
body = {
    "rows": [{
        "name": "cycle-a-updated-hidden",
        "raw_bson_hex": mutated["raw_bson_hex"],
        "pg_jsonb_hex": send_hex,
        "pg_jsonb_text": send_text,
    }]
}
(evidence / "mutate.seal-request.json").write_text(json.dumps(body))
print("mutate-pg", typeof)
PY
set +e
docker run --rm --name oo-dl01w1-41-docseal-mutseal \
  --platform linux/arm64 --pull never --network none \
  --memory 4g --memory-swap 4g \
  -v "$ELF_VOLUME:/target:ro" \
  -v "$EVIDENCE:/evidence" \
  "$IMAGE" \
  /target/debug/mongodb-preview --seal-docs --request /evidence/mutate.seal-request.json --out /evidence/mutate.seal.json
printf '%s\n' "$?" > "$EVIDENCE/mutate.seal.exit"
set -e
echo "old-mongo $(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' oo-dl01w1-41-mongodb)" > "$EVIDENCE/resource-follow.txt"
echo "follow-done"
