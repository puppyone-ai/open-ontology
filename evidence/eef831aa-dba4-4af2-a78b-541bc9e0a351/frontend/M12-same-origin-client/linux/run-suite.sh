#!/bin/bash
# Focused types, lint, and the changed Vitest files. No browser, no font rebuild, no full suite.
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
./node_modules/.bin/tsc -p tsconfig.json --noEmit > /evidence/raw/types.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types.exit
./node_modules/.bin/tsc -p tsconfig.node.json --noEmit > /evidence/raw/types-node.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types-node.exit
./node_modules/.bin/tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit > /evidence/raw/types-sdk.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types-sdk.exit
./node_modules/.bin/eslint --config config/eslint.config.mjs \
  src/api/sdk-client.test.ts \
  src/api/identity-session.ts \
  src/app/identity-context.tsx \
  ../../packages/sdk-ts/src/client.ts \
  ../../packages/sdk-ts/src/bindings.ts \
  ../../packages/sdk-ts/src/index.ts \
  config/vitest.drive.config.ts \
  design/drive/b01-client-drive.test.ts > /evidence/raw/lint.log 2>&1
printf '%s\n' "$?" > /evidence/raw/lint.exit
pnpm exec vitest run --config config/vitest.config.ts src/api/sdk-client.test.ts src/api/identity-session.test.ts > /evidence/raw/vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/vitest.exit
set -e
lock_after=$(sha256sum /src/pnpm-lock.yaml | awk '{print $1}')
printf '%s\n' "$lock_after" > /evidence/raw/lock-after.txt
if [ "$lock_after" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  exit 1
fi
for name in types types-node types-sdk lint vitest; do
  if [ "$(cat "/evidence/raw/$name.exit")" != 0 ]; then
    exit 1
  fi
done
if ! grep -q "sdk-client.test.ts" /evidence/raw/vitest.log; then
  echo "sdk-client.test.ts did not run" > /evidence/raw/missing.txt
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/verify.exit
