#!/bin/bash
# Independent reader journey. One business connection at a time. No DML.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

CASE="$RAW/cases"
EXITDIR="$RAW/exits"
if [[ -d "$CASE" ]] && [[ -n "$(ls -A "$CASE" 2>/dev/null || true)" ]]; then
  n=1
  while [[ -e "$RAW/cases-attempt-$n" ]]; do
    n=$((n + 1))
  done
  mv "$CASE" "$RAW/cases-attempt-$n"
  if [[ -d "$EXITDIR" ]]; then
    mv "$EXITDIR" "$RAW/exits-attempt-$n"
  fi
fi
mkdir -p "$CASE" "$EXITDIR"
export PYTHONPYCACHEPREFIX="${RUNDIR}/pyc"

run_named() {
  local name="$1" sql="$2" cnf="$3"
  mysql_client "$cnf" "$sql" "$CASE/${name}.stdout" "$CASE/${name}.stderr" "$EXITDIR/${name}.exit"
}

read_fail=0
for name in dup agg uniq decimal_rows decimal_sum null_versus_zero \
  partition_p202601 partition_p202602 str_order \
  identity_frontends identity_backends identity_version partitions; do
  run_named "$name" "$ROOT/reader/queries/${name}.sql" "$SECRETS/reader.cnf"
  if [[ "$(cat "$EXITDIR/${name}.exit")" != "0" ]]; then
    echo "reader query $name failed" >&2
    read_fail=1
  fi
done

run_named missing_partition "$ROOT/reader/queries/missing_partition.sql" "$SECRETS/reader.cnf"
cp "$EXITDIR/missing_partition.exit" "$EXITDIR/negative_missing_partition.exit"

for name in insert update delete create drop alter truncate; do
  run_named "reader-${name}" "$ROOT/reader/negative/${name}.sql" "$SECRETS/reader.cnf"
  cp "$EXITDIR/reader-${name}.exit" "$EXITDIR/negative_reader_${name}.exit"
done

# DBA negatives were recorded by setup. Copy those exits into the compare directory.
for name in negative_varbinary negative_partition negative_streams; do
  src_name="${name//_/-}"
  cp "$RAW/${src_name}.exit" "$EXITDIR/${name}.exit"
done

set +e
python3 "$ROOT/reader/compare.py" \
  "$ROOT/fixtures/expected.json" \
  "$CASE" \
  "$EXITDIR" >"$RAW/compare-before-cancel.json"
echo $? >"$RAW/compare-before-cancel.exit"
set -e
redact_evidence

set +e
python3 - "$CASE" "$RAW/reader-checksum-before.txt" <<'PY'
import hashlib, pathlib, sys
case = pathlib.Path(sys.argv[1])
names = ["dup","agg","uniq","decimal_rows","decimal_sum","null_versus_zero","partition_p202601","partition_p202602","str_order"]
h = hashlib.sha256()
missing = [name for name in names if not (case / f"{name}.stdout").is_file()]
if missing:
    pathlib.Path(sys.argv[2]).write_text("missing=" + ",".join(missing) + "\n")
    raise SystemExit(1)
for name in names:
    h.update(name.encode())
    h.update(b"\0")
    h.update((case / f"{name}.stdout").read_bytes())
    h.update(b"\0")
pathlib.Path(sys.argv[2]).write_text(h.hexdigest() + "\n")
print(h.hexdigest())
PY
echo $? >"$RAW/reader-checksum-before.exit"
set -e

if [[ "$(cat "$RAW/compare-before-cancel.exit")" != "0" ]]; then
  echo "compare failed; cancel still runs so the failure is not the only record" >&2
fi

set +e
"$(dirname "${BASH_SOURCE[0]}")/cancel.sh"
echo $? >"$RAW/cancel.exit"
set -e

# Re-read after cancel and after the rejected writes.
for name in dup agg uniq decimal_rows str_order; do
  run_named "after-${name}" "$ROOT/reader/queries/${name}.sql" "$SECRETS/reader.cnf"
done
set +e
python3 - "$CASE" "$RAW/reader-checksum-after.txt" <<'PY'
import hashlib, pathlib, sys
case = pathlib.Path(sys.argv[1])
names = ["dup","agg","uniq","decimal_rows","str_order"]
before = hashlib.sha256()
after = hashlib.sha256()
path = pathlib.Path(sys.argv[2])
missing = [name for name in names if not (case / f"{name}.stdout").is_file() or not (case / f"after-{name}.stdout").is_file()]
if missing:
    path.write_text("same=false\nmissing=" + ",".join(missing) + "\n")
    raise SystemExit(1)
for name in names:
    blob = (case / f"{name}.stdout").read_bytes()
    again = (case / f"after-{name}.stdout").read_bytes()
    before.update(blob)
    after.update(again)
same = before.digest() == after.digest()
path.write_text(f"same={str(same).lower()}\nbefore={before.hexdigest()}\nafter={after.hexdigest()}\n")
print(path.read_text(), end="")
if not same:
    raise SystemExit(1)
PY
echo $? >"$RAW/reader-checksum-after.exit"
set -e
dump_runtime || true
if [[ "$read_fail" != "0" ]]; then
  echo "one or more reader queries failed" >&2
  exit 1
fi
if [[ "$(cat "$RAW/compare-before-cancel.exit")" != "0" ]]; then
  echo "compare failed" >&2
  exit 1
fi
if [[ "$(cat "$RAW/cancel.exit")" != "0" ]]; then
  echo "cancel failed" >&2
  exit 1
fi
if [[ "$(cat "$RAW/reader-checksum-after.exit")" != "0" ]]; then
  echo "reader checksum changed or could not be computed" >&2
  exit 1
fi
echo "journey reads finished"
