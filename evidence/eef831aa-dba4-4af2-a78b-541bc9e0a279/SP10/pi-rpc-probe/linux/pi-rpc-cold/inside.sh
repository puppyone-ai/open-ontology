#!/bin/bash
# Send documented and unknown RPC bytes to the real Pi process. Record raw stdio.
# Network is none. No API key. No adapter interpretation.
set -u
export HOME=/var/pi-home
export PI_CODING_AGENT_DIR=/var/pi-home/agent
export PATH=/opt/pi-prefix/node/bin:/opt/pi-prefix/install/node_modules/.bin:/usr/bin:/bin
mkdir -p "$HOME" "$PI_CODING_AGENT_DIR"
if [ -e "$PI_CODING_AGENT_DIR/auth.json" ]; then
  echo "auth.json present" >&2
  echo 2 > /evidence/inside.exit
  exit 2
fi
cd /var/pi-home
set +e
timeout 30 pi --mode rpc --no-session \
  < /evidence/stdin.commands \
  > /evidence/stdout.bin \
  2> /evidence/stderr.bin
run_exit=$?
set -e
echo "$run_exit" > /evidence/pi.exit
wc -c /evidence/stdout.bin /evidence/stderr.bin > /evidence/sizes.txt
echo "PHASE pi_exit=$run_exit"
echo 0 > /evidence/inside.exit
exit 0
