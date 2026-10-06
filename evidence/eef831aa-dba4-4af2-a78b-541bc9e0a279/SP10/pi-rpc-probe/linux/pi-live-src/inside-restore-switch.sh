#!/bin/bash
# Compile the corrected observe probe, then run restore against real Pi.
# rustc runs in this same private runtime container. /tmp is noexec.
set -u
export HOME=/var/pi-home
export RUSTUP_TOOLCHAIN=1.94.0
mkdir -p /var/pi-home/bin
rustc -vV > /evidence/rustc-version.txt
rustc --edition 2024 -D warnings /src/observe.rs -o /var/pi-home/bin/observe > /evidence/observe-rustc.stdout 2> /evidence/observe-rustc.stderr
rc=$?
printf '%s\n' "$rc" > /evidence/observe-rustc.exit
if [ "$rc" -ne 0 ]; then
  echo "PHASE rustc_failed"
  exit 0
fi
cp -a /var/pi-home/bin/observe /evidence/observe-bin
/var/pi-home/bin/observe restore \
  /opt/pi-prefix/install/node_modules/.bin/pi \
  /opt/pi-prefix/node/bin \
  /var/pi-home \
  /evidence \
  /opt/probe/mock_openai \
  > /evidence/observe.stdout 2> /evidence/observe.stderr
printf '%s\n' "$?" > /evidence/observe.exit
echo "PHASE observe_exit=$(cat /evidence/observe.exit)"
exit 0
