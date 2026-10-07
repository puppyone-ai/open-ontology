#!/bin/bash
# Review and pack the stored pass1 drafts. No model socket. No bearer.
set -u
BIN=${1:?binary}
EV=/evidence
LIVE=$EV/live
OUT=$LIVE/s9
READY=$LIVE/ready
mkdir -p "$OUT"
worst=0

mark() {
  printf '%s\n' "$2" > "$OUT/$1.exit"
  if [ "$2" -ne 0 ]; then
    worst=$2
  fi
}

hash_set() {
  (
    cd "$EV" || exit 1
    find \
      live/ready/pass1.json \
      live/ready/pass1-hit.json \
      live/ready/pass2.json \
      live/ready/timeout.json \
      live/ready/unavailable.json \
      live/ready/pass1-calls \
      live/ready/cache-snapshot \
      live/pass1/curated.json \
      live/operator-review.json \
      live/roles.json \
      live/classifications.json \
      live/settings.json \
      live/llm/success.json \
      live/llm/timeout.json \
      live/per-table/timeout.json \
      -type f | sort | while IFS= read -r path; do
        sha256sum "$path"
      done
  ) > "$1"
}

hash_set "$OUT/protected-before.sha256"

for dataset in shop.party shop.account shop.entry; do
  gen="$READY/pass1-calls/${dataset}-batch0.json"
  "$BIN" review \
    --generation "$gen" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-review.json" \
    --out "$OUT/review-${dataset}.json" \
    > "$OUT/review-${dataset}.stdout" 2> "$OUT/review-${dataset}.stderr"
  mark "review-${dataset}" "$?"
  "$BIN" pack \
    --generation "$gen" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-review.json" \
    --settings "$OUT/settings.json" \
    --classifications "$LIVE/classifications.json" \
    --roles "$OUT/roles.json" \
    --hidden national_id \
    --out "$OUT/pack-${dataset}.json" \
    > "$OUT/pack-${dataset}.stdout" 2> "$OUT/pack-${dataset}.stderr"
  mark "pack-${dataset}" "$?"
  if [ -f "$OUT/review-${dataset}.json" ] && grep -q '"bound":true' "$OUT/review-${dataset}.json"; then
    printf '%s\n' bound > "$OUT/rebind-${dataset}.txt"
  else
    printf '%s\n' unbound > "$OUT/rebind-${dataset}.txt"
    worst=1
  fi
  if [ -f "$OUT/pack-${dataset}.json" ] && grep -q '"model_publication":false' "$OUT/pack-${dataset}.json"; then
    printf '%s\n' false > "$OUT/model-publication-${dataset}.txt"
  else
    printf '%s\n' missing > "$OUT/model-publication-${dataset}.txt"
    worst=1
  fi
done

fail_case() {
  local name="$1"
  local gen="$2"
  rm -f "$OUT/${name}-review.json" "$OUT/${name}-pack.json"
  "$BIN" review \
    --generation "$gen" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-review.json" \
    --out "$OUT/${name}-review.json" \
    > "$OUT/${name}-review.stdout" 2> "$OUT/${name}-review.stderr"
  local review_cli=$?
  printf '%s\n' "$review_cli" > "$OUT/${name}-review.cli"
  "$BIN" pack \
    --generation "$gen" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-review.json" \
    --settings "$OUT/settings.json" \
    --classifications "$LIVE/classifications.json" \
    --roles "$OUT/roles.json" \
    --hidden national_id \
    --out "$OUT/${name}-pack.json" \
    > "$OUT/${name}-pack.stdout" 2> "$OUT/${name}-pack.stderr"
  local pack_cli=$?
  printf '%s\n' "$pack_cli" > "$OUT/${name}-pack.cli"
  local proof=0
  if [ "$review_cli" -ne 1 ] || [ -f "$OUT/${name}-review.json" ]; then
    proof=1
  fi
  if [ "$pack_cli" -ne 1 ] || [ -f "$OUT/${name}-pack.json" ]; then
    proof=1
  fi
  mark "${name}-zero-effect" "$proof"
}

fail_case timeout "$READY/timeout.json"
fail_case unavailable "$READY/unavailable.json"

hash_set "$OUT/protected-after.sha256"
if cmp -s "$OUT/protected-before.sha256" "$OUT/protected-after.sha256"; then
  printf '%s\n' unchanged > "$OUT/protected-unchanged.txt"
  mark protected-unchanged 0
else
  printf '%s\n' changed > "$OUT/protected-unchanged.txt"
  mark protected-unchanged 1
fi

printf '%s\n' "$worst" > "$OUT/run.exit"
exit "$worst"
