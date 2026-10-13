#!/bin/bash
# Producer-trust refusal and FE pause. Does not exec oo-pipeline, does not exec
# capture-tick, does not restart a container, and does not delete a volume.
set -eu
set -o pipefail

PHASE=${1:-}
PHASE_DIR=${OO_NS4_PHASE_DIR:-/evidence/linux/run-source-4}
unset CARGO_BUILD_JOBS || true
CFG=${OO_JW_CONFIG:-/secrets/jw.conf}
JW=${CARGO_TARGET_DIR:-/target}/release/job-worker
mkdir -p "$PHASE_DIR" /target/tmp
export TMPDIR=/target/tmp
cd /src/tools/job-worker
chmod_evidence() { chmod -R a+rwX /evidence || true; }
trap chmod_evidence EXIT
note() { printf '%s\n' "$*" | tee -a "$PHASE_DIR/proof.log"; }

runp() {
  local name=$1
  shift
  set +e
  "$JW" --config "$CFG" "$@" > "$PHASE_DIR/$name.out" 2> "$PHASE_DIR/$name.err"
  local status=$?
  set -e
  printf '%s\n' "$status" > "$PHASE_DIR/$name.exit"
  note "cmd=$name exit=$status"
}

case "$PHASE" in
  gates)
    {
      echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-}"
      echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS-<unset>}"
      rustc --version
      cargo --version
    } > "$PHASE_DIR/rustc.txt" 2>&1
    set +e
    code=0
    cargo clippy --offline --all-targets --locked -- -D warnings > "$PHASE_DIR/clippy.txt" 2>&1
    clip=$?
    printf '%s\n' "$clip" > "$PHASE_DIR/clippy.exit"
    if [ "$clip" -ne 0 ]; then code=$clip; fi
    if [ "$code" -eq 0 ]; then
      cargo test --offline --locked > "$PHASE_DIR/test.txt" 2>&1
      unit=$?
      printf '%s\n' "$unit" > "$PHASE_DIR/test.exit"
      if [ "$unit" -ne 0 ]; then code=$unit; fi
    fi
    if [ "$code" -eq 0 ]; then
      cargo build --offline --locked --release > "$PHASE_DIR/build.txt" 2>&1
      built=$?
      printf '%s\n' "$built" > "$PHASE_DIR/build.exit"
      if [ "$built" -ne 0 ]; then code=$built; fi
    fi
    if [ -x "$JW" ]; then
      sha256sum "$JW" > "$PHASE_DIR/elf.sha256"
      sha256sum Cargo.lock > "$PHASE_DIR/lock.sha256"
      cp -f "$JW" "$PHASE_DIR/job-worker"
      chmod a+r "$PHASE_DIR/job-worker"
    fi
    sha256sum /opt/oo-pipeline-2e0bd/oo-pipeline > "$PHASE_DIR/native-elf.sha256"
    printf '%s\n' "$code" > "$PHASE_DIR/gates.exit"
    note "gates_exit=$code"
    exit "$code"
    ;;
  lease)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    if [ "${OPEN:-1}" != "0" ]; then
      note "open_tasks=$OPEN"
      exit 1
    fi
    runp fire-a capture-fire --schedule "$AS"
    if ! grep -q 'fire=fired' "$PHASE_DIR/fire-a.out"; then
      note "fire_not_due"
      exit 1
    fi
    runp hold capture-hold --worker w-ns4-old
    GEN=$(python3 - "$PHASE_DIR/hold.out" << 'PY'
import sys
for line in open(sys.argv[1]):
    if "generation=" in line:
        for part in line.split():
            if part.startswith("generation="):
                print(part.split("=", 1)[1])
                raise SystemExit(0)
raise SystemExit("missing generation")
PY
)
    HELD=$(python3 - "$PHASE_DIR/hold.out" << 'PY'
import sys
for line in open(sys.argv[1]):
    if line.startswith("hold=running"):
        for part in line.split():
            if part.startswith("task="):
                print(part.split("=", 1)[1])
                raise SystemExit(0)
raise SystemExit("missing task")
PY
)
    printf 'TASK=%s\nGEN=%s\n' "$HELD" "$GEN" >> "$PHASE_DIR/ids.env"
    note "lease_wait_seconds=36 task=$HELD generation=$GEN"
    sleep 36
    runp finish-old capture-finish-old --task "$HELD" --worker w-ns4-old --generation "$GEN"
    if ! grep -q 'old_owner=lost_lease' "$PHASE_DIR/finish-old.out"; then
      note "old_owner_advanced"
      exit 1
    fi
    if grep -q 'old_owner=succeeded' "$PHASE_DIR/finish-old.out"; then
      note "old_owner_succeeded"
      exit 1
    fi
    ;;
  refuse)
    runp step-refuse capture-step --worker w-ns4-new
    python3 - "$PHASE_DIR/step-refuse.out" << 'PY'
import sys
text = open(sys.argv[1]).read()
need = [
    "native_update=not_called",
    "loader_exec=refused reason=loader_trust_unrepaired",
    "source_bound=false",
    "producer_trust_trusted=false",
    "producer_trust_entry=absent",
    "jobs_lease_receipt=null",
    "target_fence_certified=false",
    "watermark_state=null",
    "watermark=unchanged",
    "step=loader_refused reason=loader_trust_unrepaired",
]
missing = [item for item in need if item not in text]
if missing:
    raise SystemExit("missing " + ",".join(missing))
if "reader_executed=true" in text or "native_update=called" in text:
    raise SystemExit("loader or reader ran")
before = after = None
for line in text.splitlines():
    if line.startswith("advanced_before="):
        before = line.split("=", 1)[1]
    if line.startswith("advanced_after="):
        after = line.split("=", 1)[1]
if not before or before != after:
    raise SystemExit("advanced_at moved")
print("refuse_ok")
PY
    ;;
  pause)
    runp pause capture-pause-fe --host 172.16.143.10 --port 9030
    if ! grep -q 'pause_enabled_remaining=0' "$PHASE_DIR/pause.out"; then
      note "pause_incomplete"
      exit 1
    fi
    if ! grep -q 'pause_source_writes=0' "$PHASE_DIR/pause.out"; then
      note "pause_wrote_source"
      exit 1
    fi
    python3 - "$PHASE_DIR/pause.out" "$PHASE_DIR/paused-schedules.txt" << 'PY'
import sys
seen = []
for line in open(sys.argv[1]):
    if "schedule=" not in line:
        continue
    if not line.startswith("pause="):
        continue
    for part in line.split():
        if part.startswith("schedule="):
            seen.append(part.split("=", 1)[1])
open(sys.argv[2], "w").write("\n".join(seen) + ("\n" if seen else ""))
if not seen:
    raise SystemExit("no schedules")
print(len(seen))
PY
    while IFS= read -r schedule; do
      [ -n "$schedule" ] || continue
      safe=$(printf '%s' "$schedule" | cut -c1-8)
      runp "fire-paused-$safe" capture-fire --schedule "$schedule"
      if ! grep -q 'fire=idle' "$PHASE_DIR/fire-paused-$safe.out"; then
        note "paused_schedule_fired=$schedule"
        exit 1
      fi
    done < "$PHASE_DIR/paused-schedules.txt"
    ;;
  show)
    runp show-final capture-show
    ;;
  *)
    echo "usage: ns4-proof.sh gates|lease|refuse|pause|show" >&2
    exit 2
    ;;
esac
