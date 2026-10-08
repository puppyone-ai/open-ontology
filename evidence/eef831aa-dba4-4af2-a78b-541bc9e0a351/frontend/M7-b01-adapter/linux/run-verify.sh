#!/bin/bash
# Font regeneration, adapter tests, and the combination unit suite. No browser.
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
export FONT_SRC=/fonts
node config/build-fonts.mjs > /evidence/raw/fonts.log
node --input-type=module -e '
import manifest from "./design/font-manifest.json" with { type: "json" };
const bad = [0x4fee, 0x8ba2, 0x9f98].filter((point) => manifest.subsetCodepoints.includes(point));
const line = JSON.stringify({ count: manifest.subsetCodepoints.length, bad, snapshot: manifest.sourceBinding.g2Snapshot });
console.log(line);
if (bad.length || manifest.sourceBinding.g2Snapshot !== "0c3caa184c93bcd0af91c5d2aa75bc4ee96416d8") process.exit(1);
' > /evidence/raw/subset-count.txt
set +e
node --test deploy/b01-adapter.test.mjs > /evidence/raw/adapter-test.log 2>&1
printf '%s\n' "$?" > /evidence/raw/adapter-test.exit
if [ "$(cat /evidence/raw/adapter-test.exit)" != 0 ]; then
  exit 1
fi
pnpm exec tsc -p tsconfig.json --noEmit > /evidence/raw/types.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types.exit
pnpm exec tsc -p tsconfig.node.json --noEmit >> /evidence/raw/types.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types-node.exit
pnpm exec tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit >> /evidence/raw/types.log 2>&1
printf '%s\n' "$?" > /evidence/raw/types-sdk.exit
pnpm exec eslint --config config/eslint.config.mjs src/styles/font-coverage.test.ts config/build-fonts.mjs > /evidence/raw/lint.log 2>&1
printf '%s\n' "$?" > /evidence/raw/lint.exit
pnpm exec vitest run --config config/vitest.config.ts > /evidence/raw/vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/vitest.exit
set -e
lock_after=$(sha256sum /src/pnpm-lock.yaml | awk '{print $1}')
printf '%s\n' "$lock_after" > /evidence/raw/lock-after.txt
if [ "$lock_after" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  exit 1
fi
for name in types types-node types-sdk lint; do
  if [ "$(cat /evidence/raw/$name.exit)" != 0 ]; then
    exit 1
  fi
done
if [ "$(cat /evidence/raw/vitest.exit)" != 0 ]; then
  if grep -q "Test Files  1 failed" /evidence/raw/vitest.log \
    && grep -q "production-route.test.tsx" /evidence/raw/vitest.log \
    && grep -F "schema_frozen" /evidence/raw/vitest.log >/dev/null \
    && ! grep -q "Test Files  2 failed" /evidence/raw/vitest.log; then
    printf '%s\n' "g2 production-route still expects schema_frozen" > /evidence/raw/g2-stale.txt
  else
    exit 1
  fi
fi
printf '%s\n' 0 > /evidence/raw/verify.exit
