#!/bin/bash
# Writable-archive combination suite. No font rebuild and no browser.
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
mkdir -p /evidence/raw
{
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo missing)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing)"
  node --version
} > /evidence/raw/cgroup.txt
cd /src
lock=$(sha256sum pnpm-lock.yaml | awk '{print $1}')
printf '%s\n' "$lock" > /evidence/raw/lock-before.txt
if [ "$lock" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  echo "lock mismatch" > /evidence/raw/lock-error.txt
  exit 1
fi
cd /src/apps/web
set +e
node --test deploy/b01-adapter.test.mjs > /evidence/raw/adapter-test.log 2>&1
printf '%s\n' "$?" > /evidence/raw/adapter-test.exit
pnpm exec vitest run --config config/vitest.config.ts > /evidence/raw/vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/vitest.exit
set -e
lock_after=$(sha256sum /src/pnpm-lock.yaml | awk '{print $1}')
printf '%s\n' "$lock_after" > /evidence/raw/lock-after.txt
if [ "$lock_after" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  exit 1
fi
if [ "$(cat /evidence/raw/adapter-test.exit)" != 0 ]; then
  exit 1
fi
if [ "$(cat /evidence/raw/vitest.exit)" != 0 ]; then
  exit 1
fi
if ! grep -q "production-route.test.tsx" /evidence/raw/vitest.log; then
  echo "production-route.test.tsx did not run" > /evidence/raw/g2-missing.txt
  exit 1
fi
if ! grep -q "modules.test.tsx" /evidence/raw/vitest.log; then
  echo "modules.test.tsx did not run" > /evidence/raw/g2-missing.txt
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/verify.exit
