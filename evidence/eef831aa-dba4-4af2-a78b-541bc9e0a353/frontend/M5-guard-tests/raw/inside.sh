#!/bin/bash
# Second Linux run. Fixed M5 archive, committed tools, owned test overlay.
set -u
export HOME=/home/fe02
export COREPACK_HOME=/corepack
export CI=1
export TMPDIR=/tmp
cd /src/apps/web

mod=src/features/admin-overview/modules.test.tsx
prod=src/features/admin-overview/production-route.test.tsx
restore() {
  if [ -f /tmp/modules.overlay.tsx ]; then
    cp /tmp/modules.overlay.tsx "$mod"
    cp /tmp/production.overlay.tsx "$prod"
  fi
}
trap restore EXIT

{
  echo "node=$(node -v)"
  echo "pnpm=$(corepack pnpm --version)"
  uname -m
  id
} > /raw/identity.txt 2>&1
cat /raw/identity.txt

phase() {
  name="$1"
  shift
  "$@" > "/raw/${name}.log" 2>&1
  ec=$?
  printf '%s\n' "$ec" > "/raw/${name}.exit"
  echo "phase=${name} exit=${ec}"
}

phase types corepack pnpm exec tsc -p tsconfig.json --noEmit --pretty false
phase types-node corepack pnpm exec tsc -p tsconfig.node.json --noEmit --pretty false
phase types-sdk corepack pnpm exec tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit --pretty false
phase lint corepack pnpm exec eslint --config config/eslint.config.mjs .
phase test corepack pnpm exec vitest run --config config/vitest.config.ts

cp "$mod" /tmp/modules.overlay.tsx
cp "$prod" /tmp/production.overlay.tsx
cp /original/modules.test.tsx "$mod"
cp /original/production-route.test.tsx "$prod"
phase font-control corepack pnpm exec vitest run --config config/vitest.config.ts src/styles/font-coverage.test.ts
restore
cmp "$mod" /tmp/modules.overlay.tsx
cmp "$prod" /tmp/production.overlay.tsx
echo RESTORED
echo DONE
