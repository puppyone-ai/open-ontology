#!/bin/bash
# Third generate. The second call stored the batch that the first call left unvalidated.
set -u
echo "oo_task=sp12-product-benchmark-hit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a389"
if [ ! -x /cargo-home/bin/pg-builder ] || [ ! -x /cargo-home/bin/product-benchmark ]; then
  echo "builder bins are absent" >&2
  exit 2
fi
set +e
/cargo-home/bin/pg-builder generate \
  --config /evidence/live/builder/generate-config.json \
  --seal /evidence/live/builder/seal.json \
  --cards /evidence/live/builder/cards.json \
  --approval /evidence/live/builder/approval.json \
  --cache /evidence/live/builder/cache \
  --out /evidence/live/builder/third.json \
  > /evidence/bench/generate-third.log 2>&1
code=$?
set +e
printf '%s\n' "$code" > /evidence/bench/phases/generate-third.exit
echo "phase_exit=$code phase=generate-third"
if [ "$code" -eq 0 ]; then
  cp /evidence/live/builder/third.json /evidence/score-input/builder-hit.json
fi
set +e
/cargo-home/bin/product-benchmark score \
  --freeze /crate/FREEZE.json \
  --input /evidence/score-input \
  --out /evidence/report \
  > /evidence/bench/score-third.log 2>&1
score=$?
set +e
printf '%s\n' "$score" > /evidence/bench/phases/score-third.exit
printf '%s\n' "$score" > /evidence/bench/bench.exit
echo "phase_exit=$score phase=score-third"
exit "$score"
