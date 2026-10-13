#!/bin/bash
# New-endpoint source capture proof inside the existing Rust 1.94 container.
# Does not create a container, does not delete a volume, and does not start or restart Doris.
set -eu
set -o pipefail

PHASE=${1:-}
PHASE_DIR=${OO_NS2_PHASE_DIR:-/evidence/linux/run-source-2}
CFG=${OO_JW_CONFIG:-/secrets/jw.conf}
JW=${CARGO_TARGET_DIR:-/target}/release/job-worker
ART=evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader.sha256
SECRET_REF=${OO_READER_SECRET_REF:-secret://oo-dl01w1-43/reader}
HOST=${OO_DORIS_HOST:-172.16.143.10}
PORT=${OO_DORIS_PORT:-9030}
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
      echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-unset}"
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
    exit "$code"
    ;;
  prepare)
    runp source-apply capture-source-apply
    runp bind-a capture-bind --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns2_a --dedupe src-ns2-a
    runp bind-b capture-bind --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
      --artifact "$ART" --table nz_obs --period-ms 7000 --rows 64 --bytes 2000000 \
      --shard src_ns2_b --dedupe src-ns2-b
    runp bind-dup capture-bind --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns2_dup --dedupe src-ns2-a
    A=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-a.out")
    B=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-b.out")
    AS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-a.out")
    BS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-b.out")
    printf 'A=%s\nB=%s\nAS=%s\nBS=%s\n' "$A" "$B" "$AS" "$BS" > "$PHASE_DIR/ids.env"
    runp source-a capture-source --binding "$A" --host "$HOST" --port "$PORT" \
      --evidence linux/run-source-2/tick-a --secret-ref "$SECRET_REF"
    runp source-b capture-source --binding "$B" --host "$HOST" --port "$PORT" \
      --evidence linux/run-source-2/tick-b --secret-ref "$SECRET_REF"
    runp period capture-period --binding "$A" --period-ms 5000
    runp show-before-fire capture-show
    runp fire-a capture-fire --schedule "$AS"
    runp fire-a-again capture-fire --schedule "$AS"
    runp show-after-fire capture-show
    python3 - "$PHASE_DIR" << 'PY'
import sys
from datetime import datetime
from pathlib import Path
phase = Path(sys.argv[1])
def nxt(name):
    for line in (phase / name).read_text().splitlines():
        if "dedupe=src-ns2-a" in line and " next=" in line:
            rest = line.split(" next=", 1)[1]
            return rest.split(" ", 1)[0] + " " + rest.split(" ", 2)[1]
    raise SystemExit("missing next")
def parse(text):
    return datetime.fromisoformat(text.replace("+00", "+00:00"))
before = parse(nxt("show-before-fire.out"))
after = parse(nxt("show-after-fire.out"))
delta = (after - before).total_seconds()
(phase / "period-delta.txt").write_text(
    f"before={before.isoformat()}\nafter={after.isoformat()}\ndelta_seconds={delta}\n"
)
if abs(delta - 5.0) > 0.001:
    raise SystemExit(f"delta {delta}")
print(delta)
PY
    if [ "$(cat "$PHASE_DIR/bind-dup.exit")" = "0" ]; then
      note "duplicate_schedule_accepted"
      exit 1
    fi
    note "period_delta_ok"
    ;;
  lease)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp hold capture-hold --worker w-ns2-old
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
    if "task=" in line:
        for part in line.split():
            if part.startswith("task="):
                print(part.split("=", 1)[1])
                raise SystemExit(0)
raise SystemExit("missing task")
PY
)
    printf 'TASK=%s\nGEN=%s\n' "$HELD" "$GEN" >> "$PHASE_DIR/ids.env"
    note "lease_wait_seconds=36"
    sleep 36
    runp finish-old capture-finish-old --task "$HELD" --worker w-ns2-old --generation "$GEN"
    ;;
  reader)
    runp step-source capture-step --worker w-ns2-new
    ;;
  replay)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp fire-replay capture-fire --schedule "$AS"
    runp step-replay capture-step --worker w-ns2-replay
    ;;
  second)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp fire-b capture-fire --schedule "$BS"
    runp step-b capture-step --worker w-ns2-b
    ;;
  failure)
    runp bind-f capture-bind --source doris --driver a5aed1167caa54e68e3a2be7335142465fb2d31a \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns2_f --dedupe src-ns2-f
    F=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-f.out")
    FS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-f.out")
    printf 'F=%s\nFS=%s\n' "$F" "$FS" >> "$PHASE_DIR/ids.env"
    runp source-f capture-source --binding "$F" --host "$HOST" --port 19030 \
      --evidence linux/run-source-2/tick-f --secret-ref "$SECRET_REF"
    runp fire-f capture-fire --schedule "$FS"
    runp step-f capture-step --worker w-ns2-fail
    ;;
  show)
    runp show-after-restart capture-show
    ;;
  *)
    echo "usage: ns2-proof.sh gates|prepare|lease|reader|replay|second|failure|show" >&2
    exit 2
    ;;
esac
