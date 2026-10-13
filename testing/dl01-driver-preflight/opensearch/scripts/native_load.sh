#!/bin/bash
# Load four native generations from producer receipts. Does not talk to OpenSearch.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
C5="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/c5-loop-20261012"
OUT="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-20261012"
IMAGE="oo-a395-selfbuild:local"
PIPE="oo-dl01w1-43-native-target"
for name in fast-1 fast-3 fast-5 fast-8; do
  cp -n "$C5/proof/$name/batch.arrow" "$OUT/receipts/$name/batch.arrow"
done
load_one() {
  local name="$1"
  local dir="$OUT/phases/load-$name"
  mkdir -p "$dir"
  cp "$OUT/receipts/$name/request.json" "$dir/request.json"
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
load_one fast-1
load_one fast-3
load_one fast-5
load_one fast-8
echo LOAD_OK
