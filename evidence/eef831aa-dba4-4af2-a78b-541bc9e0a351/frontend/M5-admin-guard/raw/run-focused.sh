#!/bin/bash
# Focused follow-up. Does not rebuild fonts or rerun the passed visual suite.
set -u
OUT=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M5-admin-guard
RAW="$OUT/raw"
mkdir -p "$RAW" /home/fe01/tmp /home/fe01/bin /home/fe01/apt/archives/partial /home/fe01/apt/lists/partial
ln -sfn /home/fe01/corepack/v1/pnpm/12.10.1/pnpm-native /home/fe01/bin/pnpm
export PATH="/home/fe01/bin:${PATH:-}"
export HOME=/home/fe01
export CI=1
export TMPDIR=/home/fe01/tmp
export PLAYWRIGHT_BROWSERS_PATH=/home/fe01/ms-playwright
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
export FE01_EVIDENCE="$OUT"
cd /src
exec > >(tee -a "$RAW/focused-console.log") 2>&1

record() { printf '%s=%s\n' "$1" "$2" | tee -a "$RAW/focused-exits.txt"; }
finish() { record "$1" "$2"; echo "FAILED phase=$1 exit=$2"; exit "$2"; }

echo "phase=identity"
{
  echo "node=$(node -v)"
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || true)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || true)"
  sha256sum /src/pnpm-lock.yaml
  sha256sum apps/web/public/fonts/NotoSansSC-full.woff2 apps/web/public/fonts/NotoSansSC-400-subset.woff2
} > "$RAW/focused-identity.txt"
cat "$RAW/focused-identity.txt"
lock=$(sha256sum /src/pnpm-lock.yaml | awk 'NR==1 {print $1}')
full=$(sha256sum apps/web/public/fonts/NotoSansSC-full.woff2 | awk 'NR==1 {print $1}')
if [ "$lock" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  finish lock 1
fi
if [ "$full" != "bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b" ]; then
  finish full_font 1
fi
record lock 0

cd /src/apps/web
echo "phase=vitest"
pnpm exec vitest run --config config/vitest.config.ts \
  src/components/shared/shared.behavior.test.tsx \
  src/api/sdk-binding.test.ts \
  src/app/runtime-config.test.ts \
  > "$RAW/focused-vitest.log" 2>&1
vitest_ec=$?
tail -n 30 "$RAW/focused-vitest.log"
if [ "$vitest_ec" -ne 0 ]; then
  finish vitest "$vitest_ec"
fi
record vitest 0

echo "phase=lint"
pnpm exec eslint --config config/eslint.config.mjs \
  src/components/shared/review-decision-form.tsx \
  src/components/shared/shared.behavior.test.tsx \
  src/api/sdk-binding.test.ts \
  src/app/runtime-config.ts \
  src/app/runtime-config.test.ts \
  design/e2e/foundation.behavior.spec.ts \
  > "$RAW/focused-lint.log" 2>&1
lint_ec=$?
tail -n 20 "$RAW/focused-lint.log"
if [ "$lint_ec" -ne 0 ]; then
  finish lint "$lint_ec"
fi
record lint 0

echo "phase=build"
pnpm exec vite build > "$RAW/focused-build.log" 2>&1
build_ec=$?
tail -n 20 "$RAW/focused-build.log"
if [ "$build_ec" -ne 0 ]; then
  finish build "$build_ec"
fi
record build 0

echo "phase=bundle"
(
  cd /src/apps/web/dist
  js=$(find assets -name 'index-*.js' | head -n 1)
  css=$(find assets -name 'index-*.css' | head -n 1)
  printf 'js=%s js_raw=%s js_gzip=%s\n' "$js" "$(wc -c < "$js")" "$(gzip -c "$js" | wc -c)"
  printf 'css=%s css_raw=%s css_gzip=%s\n' "$css" "$(wc -c < "$css")" "$(gzip -c "$css" | wc -c)"
  printf 'html_raw=%s html_gzip=%s\n' "$(wc -c < index.html)" "$(gzip -c index.html | wc -c)"
  unexpected=0
  if grep -E -q 'fonts\.googleapis|fonts\.gstatic|use\.typekit' "$js"; then unexpected=1; fi
  if ! grep -q '此能力尚未接入' "$js"; then unexpected=1; fi
  printf 'bundle_unexpected=%s\n' "$unexpected"
  exit "$unexpected"
) > "$RAW/focused-bundle.log" 2>&1
bundle_ec=$?
cat "$RAW/focused-bundle.log"
if [ "$bundle_ec" -ne 0 ]; then
  finish bundle "$bundle_ec"
fi
record bundle 0

echo "phase=apt"
cat > /etc/apt/apt.conf.d/99fe01-private <<'EOF'
APT::Sandbox::User "root";
Dir::Cache "/home/fe01/apt";
Dir::Cache::archives "/home/fe01/apt/archives";
Dir::State::lists "/home/fe01/apt/lists";
EOF
{
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    libasound2 libatk-bridge2.0-0 libatk1.0-0 libatspi2.0-0 libcups2 \
    libdbus-1-3 libdrm2 libgbm1 libnspr4 libnss3 libxcomposite1 \
    libxdamage1 libxfixes3 libxi6 libxkbcommon0 libxrandr2 libxshmfence1
} > "$RAW/focused-apt.log" 2>&1
apt_ec=$?
tail -n 8 "$RAW/focused-apt.log"
if [ "$apt_ec" -ne 0 ]; then
  finish apt "$apt_ec"
fi
record apt 0

echo "phase=playwright-behavior"
pnpm exec playwright test --config config/playwright.config.ts > "$RAW/focused-playwright.log" 2>&1
pw_ec=$?
tail -n 40 "$RAW/focused-playwright.log"
if [ "$pw_ec" -ne 0 ]; then
  finish playwright "$pw_ec"
fi
record playwright 0
echo "DONE"
exit 0
