#!/bin/bash
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
cd /src/apps/web
run() {
  name=$1
  shift
  set +e
  "$@" > "/evidence/raw/${name}.log" 2>&1
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/raw/${name}.exit"
  if [ "$code" != 0 ]; then
    exit "$code"
  fi
}
run types bash -c 'pnpm exec tsc -p tsconfig.json --noEmit && pnpm exec tsc -p tsconfig.node.json --noEmit && pnpm exec tsc -p ../../packages/sdk-ts/tsconfig.json --noEmit'
run lint pnpm exec eslint --config config/eslint.config.mjs \
  src/api/identity-session.ts \
  src/api/identity-session.test.ts \
  src/api/sdk-binding.test.ts \
  src/app/identity-context.tsx \
  src/app/admin-gate.tsx \
  src/app/runtime-config.ts \
  src/app/runtime-config.test.ts \
  src/main.tsx \
  src/features/auth/login-page.tsx \
  src/features/auth/admin-login-page.tsx \
  src/features/auth/bootstrap-page.tsx \
  src/features/user/account-page.tsx \
  design/e2e/b01.session.spec.ts \
  playwright.b01.config.ts
run vitest-fixture pnpm exec vitest run --config config/vitest.config.ts \
  src/api/sdk-binding.test.ts \
  src/app/runtime-config.test.ts \
  src/api/identity-session.test.ts \
  src/app/router.test.tsx
set +e
pnpm exec vitest run --config config/vitest.config.ts \
  src/features/admin-overview/production-route.test.tsx \
  > /evidence/raw/vitest-g2-stale.log 2>&1
printf '%s\n' "$?" > /evidence/raw/vitest-g2-stale.exit
set -e
run build-after-fixture pnpm exec vite build
sha256sum /src/pnpm-lock.yaml > /evidence/raw/lock-sha.txt
