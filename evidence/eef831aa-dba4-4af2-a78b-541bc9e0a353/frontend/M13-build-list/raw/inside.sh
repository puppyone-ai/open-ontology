#!/bin/bash
set -u
export HOME=/home/fe02
export COREPACK_HOME=/corepack
export CI=1
export TMPDIR=/tmp
cd /src/apps/web
{
  echo "node=$(node -v)"
  echo "pnpm=$(corepack pnpm --version)"
  uname -m
  id
} > /raw/identity.txt 2>&1
cat /raw/identity.txt
worst=0
phase() {
  name="$1"
  shift
  "$@" > "/raw/${name}.log" 2>&1
  ec=$?
  printf '%s\n' "$ec" > "/raw/${name}.exit"
  echo "phase=${name} exit=${ec}"
  if [ "$ec" -gt "$worst" ]; then worst=$ec; fi
}
phase types corepack pnpm exec tsc -p tsconfig.json --noEmit --pretty false
phase types-node corepack pnpm exec tsc -p tsconfig.node.json --noEmit --pretty false
phase types-sdk corepack pnpm exec tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit --pretty false
phase lint corepack pnpm exec eslint --config config/eslint.config.mjs .
phase test corepack pnpm exec vitest run --config config/vitest.config.ts
echo DONE
exit "$worst"
