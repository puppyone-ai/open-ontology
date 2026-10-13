#!/bin/bash
# Schedule API and producer-receipt proof in the existing Rust 1.94 container.
# Does not create a container, does not delete a volume, and does not restart Doris.
set -eu
set -o pipefail

PHASE=${1:-}
PHASE_DIR=${OO_NS3_PHASE_DIR:-/evidence/linux/run-source-3}
CFG=${OO_JW_CONFIG:-/secrets/jw.conf}
JW=${CARGO_TARGET_DIR:-/target}/release/job-worker
ART=evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live/build/doris-reader.sha256
SECRET_REF=${OO_READER_SECRET_REF:-secret://oo-dl01w1-43/reader}
HOST=${OO_DORIS_HOST:-172.16.143.10}
PORT=${OO_DORIS_PORT:-9030}
DRIVER=a5aed1167caa54e68e3a2be7335142465fb2d31a
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
    runp bind-a capture-bind --source doris --driver "$DRIVER" \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns3_a --dedupe src-ns3-a
    runp bind-b capture-bind --source doris --driver "$DRIVER" \
      --artifact "$ART" --table nz_obs --period-ms 7000 --rows 64 --bytes 2000000 \
      --shard src_ns3_b --dedupe src-ns3-b
    runp bind-dup capture-bind --source doris --driver "$DRIVER" \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns3_dup --dedupe src-ns3-a
    runp bind-d capture-bind --source doris --driver "$DRIVER" \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns3_d --dedupe src-ns3-d
    A=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-a.out")
    B=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-b.out")
    D=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-d.out")
    AS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-a.out")
    BS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-b.out")
    DS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-d.out")
    printf 'A=%s\nB=%s\nD=%s\nAS=%s\nBS=%s\nDS=%s\n' "$A" "$B" "$D" "$AS" "$BS" "$DS" > "$PHASE_DIR/ids.env"
    runp source-a capture-source --binding "$A" --host "$HOST" --port "$PORT" \
      --evidence linux/run-source-3/tick-a --secret-ref "$SECRET_REF"
    runp source-b capture-source --binding "$B" --host "$HOST" --port "$PORT" \
      --evidence linux/run-source-3/tick-b --secret-ref "$SECRET_REF"
    runp show-before-period capture-show
    runp period capture-period --binding "$A" --period-ms 5000
    runp show-after-period capture-show
    runp disable capture-disable --schedule "$DS"
    runp fire-d capture-fire --schedule "$DS"
    runp show-before-fire capture-show
    runp fire-a capture-fire --schedule "$AS"
    runp fire-a-again capture-fire --schedule "$AS"
    runp show-after-fire capture-show
    python3 - "$PHASE_DIR" << 'PY'
import sys
from datetime import datetime
from pathlib import Path
phase = Path(sys.argv[1])

def line_for(name, dedupe):
    for line in (phase / name).read_text().splitlines():
        if f"dedupe={dedupe}" in line and " next=" in line:
            return line
    raise SystemExit(f"missing {dedupe} in {name}")

def nxt(text):
    rest = text.split(" next=", 1)[1]
    return rest.split(" ", 1)[0] + " " + rest.split(" ", 2)[1]

def field(text, name):
    key = name + "="
    for part in text.split():
        if part.startswith(key):
            return part.split("=", 1)[1]
    raise SystemExit(f"missing {name}")

before = line_for("show-before-period.out", "src-ns3-a")
after = line_for("show-after-period.out", "src-ns3-a")
if nxt(before) != nxt(after):
    raise SystemExit("period change moved next_run_at")
if field(before, "period_ms") != "3000" or field(after, "period_ms") != "5000":
    raise SystemExit("api period did not change")
if field(after, "enabled") != "true":
    raise SystemExit("period change disabled the schedule")
period = (phase / "period.out").read_text()
if "next_before=" not in period or "next_after=" not in period:
    raise SystemExit("period command omitted next_run")
nb = [line.split("=", 1)[1] for line in period.splitlines() if line.startswith("next_before=")][0]
na = [line.split("=", 1)[1] for line in period.splitlines() if line.startswith("next_after=")][0]
if nb != na:
    raise SystemExit("schedule_set_period moved next_run_at")
fired = line_for("show-before-fire.out", "src-ns3-a")
done = line_for("show-after-fire.out", "src-ns3-a")
delta = (datetime.fromisoformat(nxt(done).replace("+00", "+00:00")) - datetime.fromisoformat(nxt(fired).replace("+00", "+00:00"))).total_seconds()
(phase / "period-delta.txt").write_text(
    f"before={nxt(fired)}\nafter={nxt(done)}\ndelta_seconds={delta}\napi_period_ms={field(after, 'period_ms')}\n"
)
if abs(delta - 5.0) > 0.001:
    raise SystemExit(f"delta {delta}")
disabled = line_for("show-after-period.out", "src-ns3-d")
if "enabled=false" not in (phase / "disable.out").read_text():
    raise SystemExit("disable did not report enabled=false")
if field(line_for("show-after-fire.out", "src-ns3-d"), "enabled") != "false":
    raise SystemExit("disabled schedule is still enabled")
if "fire=idle" not in (phase / "fire-d.out").read_text():
    raise SystemExit("disabled schedule fired")
if "fire=idle" not in (phase / "fire-a-again.out").read_text() and "fire=overlap" not in (phase / "fire-a-again.out").read_text():
    raise SystemExit("second fire was not idle")
b_line = line_for("show-after-fire.out", "src-ns3-b")
if field(b_line, "period_ms") != "7000" or field(b_line, "occurrences") != "0":
    raise SystemExit("second period was fired or changed")
print(delta)
PY
    if [ "$(cat "$PHASE_DIR/bind-dup.exit")" = "0" ]; then
      note "duplicate_schedule_accepted"
      exit 1
    fi
    for gate in source-apply bind-a bind-b bind-d source-a source-b period disable fire-d fire-a fire-a-again; do
      if [ "$(cat "$PHASE_DIR/$gate.exit")" != "0" ]; then
        note "prepare_gate_failed=$gate"
        exit 1
      fi
    done
    note "period_delta_ok"
    ;;
  lease)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp hold capture-hold --worker w-ns3-old
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
    runp finish-old capture-finish-old --task "$HELD" --worker w-ns3-old --generation "$GEN"
    ;;
  reader)
    runp step-source capture-step --worker w-ns3-new
    ;;
  refuse)
    python3 - << 'PY'
import json
from pathlib import Path
src = Path("/evidence/linux/run-source-3/tick-a/native-request.json")
doc = json.loads(src.read_text())
doc["admission_path"] = "/evidence/linux/run-source-3/tick-a/pin1.json"
Path("/evidence/linux/run-source-3/native-request-bad.json").write_text(json.dumps(doc))
PY
    set +e
    /opt/oo-pipeline-2e0bd/oo-pipeline import-native-update \
      < "$PHASE_DIR/native-request-bad.json" \
      > "$PHASE_DIR/loader-refuse.out" \
      2> "$PHASE_DIR/loader-refuse.err"
    status=$?
    set -e
    printf '%s\n' "$status" > "$PHASE_DIR/loader-refuse.exit"
    note "cmd=loader-refuse exit=$status"
    ;;
  replay)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp fire-replay capture-fire --schedule "$AS"
    runp step-replay capture-step --worker w-ns3-replay
    ;;
  second)
    # shellcheck disable=SC1091
    . "$PHASE_DIR/ids.env"
    runp fire-b capture-fire --schedule "$BS"
    runp step-b capture-step --worker w-ns3-b
    ;;
  failure)
    runp bind-f capture-bind --source doris --driver "$DRIVER" \
      --artifact "$ART" --table str_key --period-ms 3000 --rows 64 --bytes 2000000 \
      --shard src_ns3_f --dedupe src-ns3-f
    F=$(awk -F= '/^bind=/{print $2; exit}' "$PHASE_DIR/bind-f.out")
    FS=$(awk -F= '/^schedule=/{print $2; exit}' "$PHASE_DIR/bind-f.out")
    printf 'F=%s\nFS=%s\n' "$F" "$FS" >> "$PHASE_DIR/ids.env"
    runp source-f capture-source --binding "$F" --host "$HOST" --port 19030 \
      --evidence linux/run-source-3/tick-f --secret-ref "$SECRET_REF"
    runp fire-f capture-fire --schedule "$FS"
    runp step-f capture-step --worker w-ns3-fail
    ;;
  show)
    runp show-final capture-show
    ;;
  *)
    echo "usage: ns3-proof.sh gates|prepare|lease|reader|refuse|replay|second|failure|show" >&2
    exit 2
    ;;
esac
