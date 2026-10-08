#!/bin/bash
# Refuse-list retest. Does not rewrite the drive logs.
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
mkdir -p /evidence/raw
{
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo missing)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing)"
  node --version
} > /evidence/raw/refuse-cgroup.txt
cd /src
lock=$(sha256sum pnpm-lock.yaml | awk '{print $1}')
printf '%s\n' "$lock" > /evidence/raw/refuse-lock.txt
if [ "$lock" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  exit 1
fi
cd /src/apps/web
set +e
./node_modules/.bin/tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit > /evidence/raw/refuse-types-sdk.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-types-sdk.exit
./node_modules/.bin/eslint --config config/eslint.config.mjs \
  src/api/sdk-client.test.ts \
  ../../packages/sdk-ts/src/client.ts \
  ../../packages/sdk-ts/src/index.ts \
  deploy/b01-adapter.mjs \
  deploy/b01-adapter.test.mjs > /evidence/raw/refuse-lint.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-lint.exit
node --test deploy/b01-adapter.test.mjs > /evidence/raw/refuse-adapter.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-adapter.exit
pnpm exec vitest run --config config/vitest.config.ts src/api/sdk-client.test.ts > /evidence/raw/refuse-vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-vitest.exit
set -e
for name in refuse-types-sdk refuse-lint refuse-adapter refuse-vitest; do
  if [ "$(cat "/evidence/raw/$name.exit")" != 0 ]; then
    exit 1
  fi
done
printf '%s\n' 0 > /evidence/raw/refuse.exit
