#!/bin/bash
# One container: corrected restore observation, then adapter tests when /work/READY appears.
# rustc output is /usr/local/bin because the home tmpfs is noexec.
set -u
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
mkdir -p /usr/local/bin /cargo /target
echo PHASE rustc
rustc -vV > /evidence/rustc-version.txt
rustc --edition 2024 -D warnings /src/observe.rs -o /usr/local/bin/observe > /evidence/observe-rustc.stdout 2> /evidence/observe-rustc.stderr
rc=$?
printf '%s\n' "$rc" > /evidence/observe-rustc.exit
if [ "$rc" -ne 0 ]; then
  echo PHASE rustc_failed
  exit 0
fi
cp -a /usr/local/bin/observe /evidence/observe-bin
echo PHASE observe
/usr/local/bin/observe restore \
  /opt/pi-prefix/install/node_modules/.bin/pi \
  /opt/pi-prefix/node/bin \
  /var/pi-home \
  /evidence \
  /opt/probe/mock_openai \
  > /evidence/observe.stdout 2> /evidence/observe.stderr
printf '%s\n' "$?" > /evidence/observe.exit
echo "PHASE observe_exit=$(cat /evidence/observe.exit)"
deadline=$((SECONDS + 1200))
while [ ! -f /work/READY ]; do
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo PHASE ready_timeout
    exit 0
  fi
  sleep 2
done
echo PHASE test
bash /work/inside-test.sh > /proof/test-console.log 2>&1
printf '%s\n' "$?" > /proof/test-script.exit
echo "PHASE test_exit=$(cat /proof/test-script.exit)"
exit 0
