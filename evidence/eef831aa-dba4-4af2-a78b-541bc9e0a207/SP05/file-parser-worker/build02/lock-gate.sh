#!/bin/bash
# Accept a Cargo.lock diff only when the new registry rows are the Arrow IPC 60 closure.
set -eu
before=$1
after=$2

parse_lock() {
  awk '
    function emit() {
      if (name != "") {
        print name "\t" version "\t" checksum
      }
      name = ""
      version = ""
      checksum = ""
    }
    /^\[\[package\]\]/ { emit(); next }
    /^name = / { gsub(/"/, "", $3); name = $3; next }
    /^version = / { gsub(/"/, "", $3); version = $3; next }
    /^checksum = / { gsub(/"/, "", $3); checksum = $3; next }
    END { emit() }
  ' "$1" | sort
}

parse_lock "$before" > /tmp/lock-before.tsv
parse_lock "$after" > /tmp/lock-after.tsv

awk -F '\t' '
  NR == FNR { before[$1 "\t" $2] = $3; next }
  {
    key = $1 "\t" $2
    if (key in before) {
      if (before[key] != $3) {
        print "checksum-changed\t" key
      }
      delete before[key]
    } else {
      print "added\t" key
    }
  }
  END {
    for (key in before) {
      print "removed\t" key
    }
  }
' /tmp/lock-before.tsv /tmp/lock-after.tsv > /tmp/lock-delta.txt

cp /tmp/lock-delta.txt /evidence/lock-delta.txt

allowed='arrow-cmp	60.0.0
arrow-ipc	60.0.0
arrow-select	60.0.0
flatbuffers	25.2.10'

fail=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  kind=${line%%	*}
  rest=${line#*	}
  case "$kind" in
    checksum-changed|removed)
      echo "lock gate reject: $line" >&2
      fail=1
      ;;
    added)
      if ! printf '%s\n' "$allowed" | grep -Fxq "$rest"; then
        echo "lock gate reject: $line" >&2
        fail=1
      fi
      ;;
  esac
done < /tmp/lock-delta.txt

for need in "arrow-cmp	60.0.0" "arrow-ipc	60.0.0" "arrow-select	60.0.0" "flatbuffers	25.2.10"; do
  if ! grep -Fxq "added	${need}" /tmp/lock-delta.txt; then
    echo "lock gate reject: missing ${need}" >&2
    fail=1
  fi
done

if grep -E '(^|	)(lz4|zstd|lz4_flex)(	|$)' /tmp/lock-after.tsv /tmp/lock-delta.txt >/dev/null 2>&1; then
  echo "lock gate reject: compression crate" >&2
  fail=1
fi

# The path package has no checksum, so its dependency list is checked separately.
awk '
  $0 == "name = \"oo-connectors\"" { capture = 1 }
  capture { print }
  capture && $0 == "]" { exit }
' "$after" > /tmp/oo-connectors-deps.txt
if ! grep -Fxq ' "arrow-ipc",' /tmp/oo-connectors-deps.txt; then
  echo "lock gate reject: oo-connectors does not depend on arrow-ipc" >&2
  fail=1
fi
cp /tmp/oo-connectors-deps.txt /evidence/oo-connectors-deps.txt

if [ "$fail" -ne 0 ]; then
  exit 4
fi
echo "lock gate ok"
