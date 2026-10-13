#!/bin/bash
# Continue native load after the preserved fast-5 OO024.
# Does not rebuild, does not talk to OpenSearch, does not overwrite phases/load-fast-5.
# Generation 3 uses the producer request plus full_snapshot=true because the sealed
# Arrow is the full 6-row live set and incremental copy_forward cannot see a new column.
# Generation 4 uses the producer request unchanged (tombstone led-c5).
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
OUT="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-20261012"
IMAGE="oo-a395-selfbuild:local"
PIPE="oo-dl01w1-43-native-target"
ELF_EXPECT="4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
PIT_STDOUT="a47fba71e26fa71811afe1b5d8517930a95e2f3f1c7572e42e0ded869be19f10"
PIT_STDERR="62c501bde9d3fe655da71c75cecf5d75c6fa037825b0d34fc79da20d97fcb84d"
PIT_EXIT="4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865"

test "$(sha256sum "$OUT/phases/load-fast-5/stdout.txt" | awk '{print $1}')" = "$PIT_STDOUT"
test "$(sha256sum "$OUT/phases/load-fast-5/stderr.txt" | awk '{print $1}')" = "$PIT_STDERR"
test "$(sha256sum "$OUT/phases/load-fast-5/exit.txt" | awk '{print $1}')" = "$PIT_EXIT"

elf="$(docker run --rm --platform linux/arm64 --pull never --entrypoint sha256sum \
  -v "$PIPE":/pipe:ro --label owner=oo-dl01w1-44 --label task=DL01-W1-44 \
  "$IMAGE" /pipe/debug/oo-pipeline | awk '{print $1}')"
test "$elf" = "$ELF_EXPECT"
printf '%s\n' "$elf" > "$OUT/phases/pipeline-elf.sha256"

python3 - "$OUT" <<'PY'
import hashlib, json, sys
from pathlib import Path
out = Path(sys.argv[1])
raw = (out / "receipts/fast-5/request.json").read_bytes()
if not raw.endswith(b"}"):
    raise SystemExit("producer request is not a single JSON object")
body = raw[:-1] + b',"full_snapshot":true}'
doc = json.loads(body)
base = json.loads(raw)
if set(doc) - set(base) != {"full_snapshot"} or doc["full_snapshot"] is not True:
    raise SystemExit("snapshot request changed more than full_snapshot")
for key, value in base.items():
    if doc[key] != value:
        raise SystemExit(f"field drifted: {key}")
if doc["tombstones"] != [] or doc["batch_id"] != "os44-fast-5":
    raise SystemExit("fast-5 identity drifted")
dest = out / "phases/load-fast-5-snapshot"
dest.mkdir(parents=True)
(dest / "request.json").write_bytes(body)
note = {
    "protocol": "oo.opensearch.native-load-shape/v1",
    "reason": "preserved incremental load returned OO024 parent_absent because copy_forward looks up c5_extra on generation 2",
    "arrow_rows": 6,
    "tombstones": [],
    "shape": "snapshot",
    "receipt_bytes": "unchanged",
    "batch_id": "os44-fast-5",
    "producer_request_sha256": hashlib.sha256(raw).hexdigest(),
    "snapshot_request_sha256": hashlib.sha256(body).hexdigest(),
    "pipeline_elf_sha256": "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe",
}
(dest / "shape-note.json").write_text(json.dumps(note, indent=2) + "\n")
PY

load_one() {
  local name="$1"
  local request="$2"
  local dir="$OUT/phases/load-$name"
  mkdir -p "$dir"
  if [ ! -f "$dir/request.json" ]; then
    cp "$request" "$dir/request.json"
  fi
  set +e
  docker run --rm -i --network container:oo-dl01w1-44-native-pg --platform linux/arm64 --pull never \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --entrypoint /pipe/debug/oo-pipeline \
    -v "$PIPE":/pipe:ro \
    -v "$OUT/receipts":/work:ro \
    -v /tmp/oo-dl01w1-44-private:/private:ro \
    --label owner=oo-dl01w1-44 --label task=DL01-W1-44 \
    "$IMAGE" import-native-update \
    < "$dir/request.json" > "$dir/stdout.txt" 2> "$dir/stderr.txt"
  local code=$?
  set -e
  printf '%s\n' "$code" > "$dir/exit.txt"
  echo "load $name exit $code"
  if [ "$code" != "0" ]; then
    exit "$code"
  fi
}

load_one fast-5-snapshot "$OUT/phases/load-fast-5-snapshot/request.json"
load_one fast-8 "$OUT/receipts/fast-8/request.json"
test "$(sha256sum "$OUT/phases/load-fast-5/stdout.txt" | awk '{print $1}')" = "$PIT_STDOUT"
echo LOAD_CONTINUE_OK
