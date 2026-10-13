#!/bin/bash
# Capture schedule proof inside the Rust 1.94 container.
# build: locked clippy, unit tests, release ELF. The ignored 100-worker gate is not run.
# before-restart / after-restart: real control and target databases over TLS.
set -eu
set -o pipefail

PHASE=${1:-}
PHASE_DIR=${OO_PHASE_DIR:-/evidence/linux/run}
CFG=${OO_JW_CONFIG:-/secrets/jw.conf}
JW=${CARGO_TARGET_DIR:-/target}/release/job-worker
mkdir -p "$PHASE_DIR" /target/tmp
export TMPDIR=/target/tmp
cd /src/tools/job-worker
chmod_evidence() { chmod -R a+rwX /evidence || true; }
trap chmod_evidence EXIT

note() { printf '%s\n' "$*" | tee -a "$PHASE_DIR/proof.log"; }

case "$PHASE" in
  build|before-restart|resume|after-restart) ;;
  *)
    echo "usage: capture-proof.sh build|before-restart|after-restart" >&2
    exit 2
    ;;
esac

if [ "$PHASE" = "build" ]; then
  {
    echo "HOME=$HOME"
    echo "CARGO_HOME=$CARGO_HOME"
    echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-}"
    echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-unset}"
    rustc --version
    cargo --version
  } > "$PHASE_DIR/rustc.txt" 2>&1
  if ! rustup component list --installed | grep -q '^clippy-'; then
    rustup component add clippy > "$PHASE_DIR/rustup-component.txt" 2>&1
  fi
  set +e
  code=0
  cargo metadata --locked --format-version 1 --no-deps > "$PHASE_DIR/metadata.txt" 2> "$PHASE_DIR/metadata.err"
  meta=$?
  printf '%s\n' "$meta" > "$PHASE_DIR/metadata.exit"
  if [ "$meta" -ne 0 ]; then code=$meta; fi
  if [ "$code" -eq 0 ]; then
    cargo clippy --all-targets --locked -- -D warnings > "$PHASE_DIR/clippy.txt" 2>&1
    clip=$?
    printf '%s\n' "$clip" > "$PHASE_DIR/clippy.exit"
    if [ "$clip" -ne 0 ]; then code=$clip; fi
  fi
  if [ "$code" -eq 0 ]; then
    cargo test --locked > "$PHASE_DIR/test.txt" 2>&1
    unit=$?
    printf '%s\n' "$unit" > "$PHASE_DIR/test.exit"
    if [ "$unit" -ne 0 ]; then code=$unit; fi
  fi
  if [ "$code" -eq 0 ]; then
    cargo build --locked --release > "$PHASE_DIR/build.txt" 2>&1
    built=$?
    printf '%s\n' "$built" > "$PHASE_DIR/build.exit"
    if [ "$built" -ne 0 ]; then code=$built; fi
  fi
  if [ -x "$JW" ]; then
    sha256sum "$JW" > "$PHASE_DIR/elf.sha256"
    sha256sum Cargo.lock > "$PHASE_DIR/lock.sha256"
  fi
  printf '%s\n' "$code" > "$PHASE_DIR/build-phase.exit"
  exit "$code"
fi

runp() {
  local name=$1
  shift
  set +e
  "$JW" --config "$CFG" "$@" > "$PHASE_DIR/$name.out" 2> "$PHASE_DIR/$name.err"
  local status=$?
  set -e
  printf '%s\n' "$status" > "$PHASE_DIR/$name.exit"
  note "cmd=$name exit=$status"
  cat "$PHASE_DIR/$name.out" >> "$PHASE_DIR/proof.log"
  if [ -s "$PHASE_DIR/$name.err" ]; then
    sed 's/^/err /' "$PHASE_DIR/$name.err" >> "$PHASE_DIR/proof.log"
  fi
  if [ "$status" -ne 0 ]; then
    echo "$name exit=$status" >> "$PHASE_DIR/failures.txt"
  fi
  return 0
}

require_out() {
  local name=$1 pattern=$2
  if grep -q "$pattern" "$PHASE_DIR/$name.out"; then
    note "check=$name ok $pattern"
  else
    note "check=$name FAIL missing $pattern"
    echo "$name $pattern" >> "$PHASE_DIR/failures.txt"
  fi
}

if [ "$PHASE" = "before-restart" ]; then
  : > "$PHASE_DIR/failures.txt"
  DORIS_A1=evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/a1.json
  DORIS_B2=evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/b2.json
  CH_ART=evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4/plan.json
  MONGO_ART=evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79/plan-c5-a-request.json
  STAR_ART=evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/capture-schedule/summary.json
  runp apply capture-apply
  runp reject capture-reject
  require_out reject 'reject_argv=true'
  require_out reject 'reject_extra=true'
  require_out reject 'reject_secret=true sqlstate=OO010'
  runp fence capture-fence
  require_out fence 'fence_update=rejected'
  require_out fence 'target_fence_true=0'

  runp bind-nz capture-bind \
    --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
    --artifact "$DORIS_A1" --table nz_obs --period-ms 3000 \
    --rows 64 --bytes 2000000 --shard cap.nz --dedupe cap-nz
  runp bind-str capture-bind \
    --source doris --driver d9255c6a69a9b534a5f5c792a964c43023764f58 \
    --artifact "$DORIS_A1" --table str_key --period-ms 7000 \
    --rows 64 --bytes 2000000 --shard cap.str --dedupe cap-str
  NZ=$(awk -F= '/^bind=/{print $2}' "$PHASE_DIR/bind-nz.out")
  NZ_SCHED=$(awk -F= '/^schedule=/{print $2}' "$PHASE_DIR/bind-nz.out")
  STR=$(awk -F= '/^bind=/{print $2}' "$PHASE_DIR/bind-str.out")
  STR_SCHED=$(awk -F= '/^schedule=/{print $2}' "$PHASE_DIR/bind-str.out")
  printf 'NZ=%s\nNZ_SCHED=%s\nSTR=%s\nSTR_SCHED=%s\n' "$NZ" "$NZ_SCHED" "$STR" "$STR_SCHED" > "$PHASE_DIR/state.env"
  note "nz=$NZ schedule=$NZ_SCHED str=$STR schedule=$STR_SCHED"

  runp fire-a capture-fire --schedule "$NZ_SCHED" &
  runp fire-b capture-fire --schedule "$NZ_SCHED" &
  wait || true
  fired=$(grep -h 'fire=fired' "$PHASE_DIR"/fire-a.out "$PHASE_DIR"/fire-b.out | wc -l | tr -d ' ')
  note "concurrent_fired_lines=$fired"
  if [ "$fired" -ne 1 ]; then
    echo "concurrent fire" >> "$PHASE_DIR/failures.txt"
  fi
  runp fire-idle capture-fire --schedule "$NZ_SCHED"
  require_out fire-idle 'fire=idle'
  sleep 3.2
  runp fire-overlap capture-fire --schedule "$NZ_SCHED"
  require_out fire-overlap 'fire=overlap'
  runp period capture-period --binding "$NZ" --period-ms 5000
  require_out period 'period_was_ms=3000'
  require_out period 'period_now_ms=5000'
  require_out period 'inflight_payload=unchanged'
  runp show-before-apply capture-show
  runp arm-apply capture-arm --binding "$NZ" --kind apply_once
  runp step-apply-fail capture-step --worker w-cap
  require_out step-apply-fail 'reason=apply_once'
  require_out step-apply-fail 'watermark=absent'
  sleep 2
  runp step-nz-ok capture-step --worker w-cap
  require_out step-nz-ok 'step=succeeded'
  require_out step-nz-ok 'copied=true'
  require_out step-nz-ok 'watermark=advanced'
  NZ_PHYS=$(sed -n 's/.*receipt=src_doris\/\(pt_[0-9a-f]\{60\}\)\/pin1.*/\1/p' "$PHASE_DIR/step-nz-ok.out" | head -1)
  printf 'NZ_PHYS=%s\n' "$NZ_PHYS" >> "$PHASE_DIR/state.env"
  runp show-nz capture-show --physical "$NZ_PHYS"
  require_out show-nz 'rows=3'
  runp show-before-period-fire capture-show
  runp fire-period capture-fire --schedule "$NZ_SCHED"
  require_out fire-period 'period_ms=5000'
  require_out fire-period 'fire=fired'
  runp show-after-period-fire capture-show
  python3 - "$PHASE_DIR" <<'PY'
import sys
from datetime import datetime
from pathlib import Path
phase = Path(sys.argv[1])
def nxt(name):
    for line in (phase / name).read_text().splitlines():
        if "table=nz_obs" in line and " next=" in line:
            return line.split(" next=", 1)[1].split(" ", 1)[0] + " " + line.split(" next=", 1)[1].split(" ", 2)[1]
    raise SystemExit("missing next")
def parse(text):
    text = text.replace("+00", "+00:00")
    return datetime.fromisoformat(text)
before = parse(nxt("show-before-period-fire.out"))
after = parse(nxt("show-after-period-fire.out"))
delta = (after - before).total_seconds()
(phase / "period-delta.txt").write_text(f"before={before.isoformat()}\nafter={after.isoformat()}\ndelta_seconds={delta}\n")
if abs(delta - 5.0) > 0.001:
    raise SystemExit(f"delta {delta}")
PY
  note "period_delta_ok"
  runp step-nz-replay capture-step --worker w-cap
  require_out step-nz-replay 'recover=replay'
  require_out step-nz-replay 'copied=false'
  runp show-nz-replay capture-show --physical "$NZ_PHYS"
  require_out show-nz-replay 'rows=3'

  runp fire-str capture-fire --schedule "$STR_SCHED"
  require_out fire-str 'period_ms=7000'
  runp arm-mirror capture-arm --binding "$STR" --kind mirror_once
  runp step-mirror capture-step --worker w-cap
  require_out step-mirror 'step=mirror_pending'
  require_out step-mirror 'watermark=absent'
  STR_PHYS=$(sed -n 's/.*physical=\(pt_[0-9a-f]\{60\}\) .*/\1/p' "$PHASE_DIR/step-mirror.out" | head -1)
  printf 'STR_PHYS=%s\n' "$STR_PHYS" >> "$PHASE_DIR/state.env"
  runp show-str-pending capture-show --physical "$STR_PHYS"
  require_out show-str-pending 'rows=7'
  runp step-mirror-replay capture-step --worker w-cap
  require_out step-mirror-replay 'recover=replay'
  require_out step-mirror-replay 'copied=false'
  require_out step-mirror-replay 'watermark=advanced'
  runp show-str capture-show --physical "$STR_PHYS"
  require_out show-str 'rows=7'
  runp show-watermarks capture-show
  note "before_restart_done"
  if [ -s "$PHASE_DIR/failures.txt" ]; then
    note "before_restart_failures"
    cat "$PHASE_DIR/failures.txt" >> "$PHASE_DIR/proof.log"
    exit 1
  fi
  exit 0
fi

if [ "$PHASE" = "resume" ]; then
  : > "$PHASE_DIR/failures.txt"
  : "${NZ:?}" "${NZ_SCHED:?}" "${STR:?}" "${STR_SCHED:?}"
  printf 'NZ=%s\nNZ_SCHED=%s\nSTR=%s\nSTR_SCHED=%s\n' "$NZ" "$NZ_SCHED" "$STR" "$STR_SCHED" > "$PHASE_DIR/state.env"
  note "resume nz=$NZ schedule=$NZ_SCHED str=$STR schedule=$STR_SCHED from=${OO_RESUME_FROM:-apply}"
  if [ "${OO_RESUME_FROM:-apply}" != "success" ]; then
    runp step-apply-fail capture-step --worker w-cap
    require_out step-apply-fail 'reason=apply_once'
    require_out step-apply-fail 'watermark=absent'
    sleep 2
  fi
  runp step-nz-ok capture-step --worker w-cap
  require_out step-nz-ok 'step=succeeded'
  require_out step-nz-ok 'copied=true'
  require_out step-nz-ok 'watermark=advanced'
  NZ_PHYS=$(sed -n 's/.*receipt=src_doris\/\(pt_[0-9a-f]\{60\}\)\/pin1.*/\1/p' "$PHASE_DIR/step-nz-ok.out" | head -1)
  printf 'NZ_PHYS=%s\n' "$NZ_PHYS" >> "$PHASE_DIR/state.env"
  runp show-nz capture-show --physical "$NZ_PHYS"
  require_out show-nz 'rows=3'
  runp show-before-period-fire capture-show
  runp fire-period capture-fire --schedule "$NZ_SCHED"
  require_out fire-period 'period_ms=5000'
  require_out fire-period 'fire=fired'
  runp show-after-period-fire capture-show
  python3 - "$PHASE_DIR" <<'PY'
import sys
from datetime import datetime
from pathlib import Path
phase = Path(sys.argv[1])
def nxt(name):
    for line in (phase / name).read_text().splitlines():
        if "table=nz_obs" in line and " next=" in line:
            return line.split(" next=", 1)[1].split(" ", 1)[0] + " " + line.split(" next=", 1)[1].split(" ", 2)[1]
    raise SystemExit("missing next")
def parse(text):
    text = text.replace("+00", "+00:00")
    return datetime.fromisoformat(text)
before = parse(nxt("show-before-period-fire.out"))
after = parse(nxt("show-after-period-fire.out"))
delta = (after - before).total_seconds()
(phase / "period-delta.txt").write_text(f"before={before.isoformat()}\nafter={after.isoformat()}\ndelta_seconds={delta}\n")
if abs(delta - 5.0) > 0.001:
    raise SystemExit(f"delta {delta}")
PY
  note "period_delta_ok"
  runp step-nz-replay capture-step --worker w-cap
  require_out step-nz-replay 'recover=replay'
  require_out step-nz-replay 'copied=false'
  runp show-nz-replay capture-show --physical "$NZ_PHYS"
  require_out show-nz-replay 'rows=3'
  runp fire-str capture-fire --schedule "$STR_SCHED"
  require_out fire-str 'period_ms=7000'
  runp arm-mirror capture-arm --binding "$STR" --kind mirror_once
  runp step-mirror capture-step --worker w-cap
  require_out step-mirror 'step=mirror_pending'
  require_out step-mirror 'watermark=absent'
  STR_PHYS=$(sed -n 's/.*physical=\(pt_[0-9a-f]\{60\}\) .*/\1/p' "$PHASE_DIR/step-mirror.out" | head -1)
  printf 'STR_PHYS=%s\n' "$STR_PHYS" >> "$PHASE_DIR/state.env"
  runp show-str-pending capture-show --physical "$STR_PHYS"
  require_out show-str-pending 'rows=7'
  runp step-mirror-replay capture-step --worker w-cap
  require_out step-mirror-replay 'recover=replay'
  require_out step-mirror-replay 'copied=false'
  require_out step-mirror-replay 'watermark=advanced'
  runp show-str capture-show --physical "$STR_PHYS"
  require_out show-str 'rows=7'
  runp show-watermarks capture-show
  note "resume_before_restart_done"
  if [ -s "$PHASE_DIR/failures.txt" ]; then
    note "resume_failures"
    cat "$PHASE_DIR/failures.txt" >> "$PHASE_DIR/proof.log"
    exit 1
  fi
  exit 0
fi

# after-restart
set -a
# shellcheck disable=SC1091
. "$PHASE_DIR/state.env"
set +a
: > "$PHASE_DIR/failures-after.txt"
runp show-after-restart capture-show
require_out show-after-restart "binding=$NZ"
require_out show-after-restart "binding=$STR"
require_out show-after-restart 'seal=awaiting_publish'
runp show-nz-survive capture-show --physical "$NZ_PHYS"
require_out show-nz-survive 'rows=3'
runp show-str-survive capture-show --physical "$STR_PHYS"
require_out show-str-survive 'rows=7'
runp repin capture-repin --binding "$NZ" --artifact evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/b2.json
require_out repin 'pin=2'
runp period-short capture-period --binding "$NZ" --period-ms 1000
for _ in 1 2 3 4 5 6 7 8; do
  runp fire-repin capture-fire --schedule "$NZ_SCHED"
  if grep -q 'fire=fired' "$PHASE_DIR/fire-repin.out"; then
    break
  fi
  sleep 1
done
require_out fire-repin 'fire=fired'
require_out fire-repin 'pin=2'
runp hold capture-hold --worker w-old
require_out hold 'hold=running'
TASK=$(sed -n 's/.*task=\([^ ]*\).*/\1/p' "$PHASE_DIR/hold.out" | head -1)
GEN=$(sed -n 's/.*generation=\([^ ]*\).*/\1/p' "$PHASE_DIR/hold.out" | head -1)
sleep 11
runp beat capture-beat --task "$TASK" --worker w-old --generation "$GEN"
require_out beat 'heartbeat=extended'
sleep 31
runp finish-old capture-finish-old --task "$TASK" --worker w-old --generation "$GEN"
require_out finish-old 'watermark=not_written'
if grep -q 'old_owner=succeeded' "$PHASE_DIR/finish-old.out"; then
  echo "old owner succeeded" >> "$PHASE_DIR/failures-after.txt"
fi
runp step-b2 capture-step --worker w-cap
require_out step-b2 'step=succeeded'
require_out step-b2 'generation=2'
B2_PHYS=$(sed -n 's/.*receipt=src_doris\/\(pt_[0-9a-f]\{60\}\)\/pin2.*/\1/p' "$PHASE_DIR/step-b2.out" | head -1)
runp show-b2 capture-show --physical "$B2_PHYS"
require_out show-b2 'rows=2'
runp show-old-shadow capture-show --physical "$NZ_PHYS"
require_out show-old-shadow 'rows=3'

runp bind-part capture-bind \
  --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
  --artifact evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/a1.json \
  --table part_day --period-ms 1000 --rows 64 --bytes 2000000 --shard cap.part --dedupe cap-part
runp bind-ch capture-bind \
  --source clickhouse --driver 15e35662cdd83237514006bb638d2d586fe3830b \
  --artifact evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-14-c4/plan.json \
  --table readings --period-ms 1000 --rows 64 --bytes 2000000 --shard cap.ch --dedupe cap-ch
runp bind-mongo capture-bind \
  --source mongodb --driver c7678516a362a64d86d4b75c0a102741099134d1 \
  --artifact evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79/plan-c5-a-request.json \
  --table events --period-ms 1000 --rows 64 --bytes 2000000 --shard cap.mongo --dedupe cap-mongo
runp bind-star capture-bind \
  --source starrocks --driver eee2157a2886009a2de877c7624157359235a88f \
  --artifact evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/capture-schedule/summary.json \
  --table uniq_dim --period-ms 1000 --rows 64 --bytes 2000000 --shard cap.star --dedupe cap-star
for pair in part:bind-part ch:bind-ch mongo:bind-mongo star:bind-star; do
  label=${pair%%:*}
  file=${pair##*:}
  id=$(awk -F= '/^bind=/{print $2}' "$PHASE_DIR/$file.out")
  sched=$(awk -F= '/^schedule=/{print $2}' "$PHASE_DIR/$file.out")
  runp "fire-$label" capture-fire --schedule "$sched"
  runp "step-$label" capture-step --worker w-cap
  require_out "step-$label" 'step=failed'
  require_out "step-$label" 'watermark=absent'
  if grep -q "watermark binding=$id" "$PHASE_DIR/show-after-restart.out"; then
    echo "early watermark $label" >> "$PHASE_DIR/failures-after.txt"
  fi
done
runp show-final capture-show
for file in bind-part bind-ch bind-mongo bind-star; do
  id=$(awk -F= '/^bind=/{print $2}' "$PHASE_DIR/$file.out")
  if grep -q "watermark binding=$id " "$PHASE_DIR/show-final.out"; then
    echo "refused binding advanced $id" >> "$PHASE_DIR/failures-after.txt"
  fi
done
note "after_restart_done"
if [ -s "$PHASE_DIR/failures.txt" ] || [ -s "$PHASE_DIR/failures-after.txt" ]; then
  cat "$PHASE_DIR/failures.txt" "$PHASE_DIR/failures-after.txt" >> "$PHASE_DIR/proof.log"
  exit 1
fi
exit 0
