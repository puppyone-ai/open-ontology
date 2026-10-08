#!/bin/bash
# After the live drive, the destroyed m12 hosts must be refused. No browser and no full suite.
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
mkdir -p /evidence/raw
{
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo missing)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing)"
  node --version
} > /evidence/raw/refuse-cgroup.txt
cd /src/apps/web
set +e
./node_modules/.bin/tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit > /evidence/raw/refuse-types-sdk.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-types-sdk.exit
./node_modules/.bin/eslint --config config/eslint.config.mjs \
  src/api/sdk-client.test.ts \
  src/api/identity-session.ts \
  src/app/identity-context.tsx \
  ../../packages/sdk-ts/src/client.ts \
  deploy/b01-adapter.mjs > /evidence/raw/refuse-lint.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-lint.exit
pnpm exec vitest run --config config/vitest.config.ts \
  src/api/sdk-client.test.ts \
  src/api/identity-session.test.ts \
  src/features/admin-overview/production-route.test.tsx \
  src/features/permissions/current-session.test.tsx > /evidence/raw/refuse-vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/refuse-vitest.exit
set -e
for name in refuse-types-sdk refuse-lint refuse-vitest; do
  if [ "$(cat "/evidence/raw/$name.exit")" != 0 ]; then
    exit 1
  fi
done
if ! grep -q "oo-fe01-m12-edge" /src/packages/sdk-ts/src/client.ts; then
  echo "m12 host missing from client" > /evidence/raw/refuse-missing.txt
  exit 1
fi
if ! grep -q "oo-fe01-m12-edge" /src/apps/web/deploy/b01-adapter.mjs; then
  echo "m12 host missing from adapter" > /evidence/raw/refuse-missing.txt
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/refuse.exit
