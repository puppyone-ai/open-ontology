#!/bin/bash
# Run produce --config against the compiled query-pg binary and check the
# captured NDJSON and audit receipt against the literal gold.
set -u
export HOME=/tmp
export PATH="/usr/local/cargo/bin:/usr/bin:/bin"
mkdir -p /tmp/bin /out/capture /out/gold
rustc --edition 2021 -O -o /tmp/bin/sha256 /tools/sha256.rs > /out/sha256-tool.txt 2> /out/sha256-tool.stderr
printf '%s\n' "$?" > /out/sha256-tool.exit
rustc --edition 2021 -O -o /tmp/bin/oracle /probe/oracle.rs > /out/oracle-build.txt 2> /out/oracle-build.stderr
printf '%s\n' "$?" > /out/oracle-build.exit
if [ "$(cat /out/sha256-tool.exit)" != "0" ] || [ "$(cat /out/oracle-build.exit)" != "0" ]; then
  echo "FAILED oracle tools"
  exit 1
fi
/tmp/bin/sha256 file /gold/select.sql > /out/gold/select.sha256
printf '%s\n' "$?" > /out/sha256-select.exit
if [ "$(cat /out/sha256-select.exit)" != "0" ]; then
  echo "FAILED sql hash"
  exit 1
fi
# The oracle reads gold next to the evidence root. The mount is /gold.
cp /gold/select.sql /gold/probe.sql /gold/row-line.txt /out/gold/
if [ "${OO_A364_JUDGE_ONLY:-0}" = "1" ]; then
  echo "judge only; keeping the captured receipt"
else
  /sbs-target/debug/selfbuild-suggestions produce --config /out/gateway-config.json \
    < /out/live-command.json > /out/outcome.json 2> /out/outcome.stderr
  printf '%s\n' "$?" > /out/outcome.exit
  if [ "$(cat /out/outcome.exit)" != "0" ]; then
    echo "FAILED produce=$(cat /out/outcome.exit)"
    exit 1
  fi
fi
/tmp/bin/oracle /out > /out/oracle.txt 2> /out/oracle.stderr
printf '%s\n' "$?" > /out/oracle.exit
if [ "$(cat /out/oracle.exit)" != "0" ]; then
  echo "FAILED oracle=$(cat /out/oracle.exit)"
  exit 1
fi
echo "DONE live"
exit 0
