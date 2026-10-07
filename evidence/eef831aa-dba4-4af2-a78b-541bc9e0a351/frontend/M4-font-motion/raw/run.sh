#!/bin/bash
# UUID-private M4 font subset and focused browser proof. No package install.
set -u
OUT=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M4-font-motion
RAW="$OUT/raw"
mkdir -p "$RAW" /home/fe01/tmp /home/fe01/bin /home/fe01/apt/archives/partial /home/fe01/apt/lists/partial
ln -sfn /home/fe01/corepack/v1/pnpm/12.10.1/pnpm-native /home/fe01/bin/pnpm
export PATH="/home/fe01/bin:${PATH:-}"
export HOME=/home/fe01
export CI=1
export TMPDIR=/home/fe01/tmp
export FONT_SRC=/fonts/src
export PLAYWRIGHT_BROWSERS_PATH=/home/fe01/ms-playwright
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
export FE01_EVIDENCE="$OUT"
cd /src
exec > >(tee -a "$RAW/console.log") 2>&1

record() { printf '%s=%s\n' "$1" "$2" | tee -a "$RAW/exits.txt"; }
finish() { record "$1" "$2"; echo "FAILED phase=$1 exit=$2"; exit "$2"; }

echo "phase=identity"
{
  echo "node=$(node -v)"
  echo "pnpm=$(pnpm --version)"
  uname -m
  id
  grep -E '^(CapEff|CapBnd|CapAmb):' /proc/1/status || true
  awk '/MemTotal|MemAvailable/ {print}' /proc/meminfo
  echo "nproc=$(nproc)"
  sha256sum /src/pnpm-lock.yaml
  ls -l /fonts/src
} > "$RAW/identity.txt" 2>&1
cat "$RAW/identity.txt"
lock=$(sha256sum /src/pnpm-lock.yaml | awk 'NR==1 {print $1}')
if [ "$lock" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  finish lock 1
fi
record lock 0

echo "phase=fonts-before"
sha256sum \
  apps/web/public/fonts/NotoSansSC-300-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-400-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-500-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-full.woff2 \
  apps/web/public/fonts/Geist-wght.woff2 \
  apps/web/public/fonts/GeistMono-Regular.woff2 \
  apps/web/public/fonts/licenses/Geist-OFL.txt \
  apps/web/public/fonts/licenses/NotoSans-LICENSE \
  > "$RAW/fonts-before.txt"
record fonts_before 0

echo "phase=font-build"
node apps/web/config/build-fonts.mjs > "$RAW/font-build.log" 2>&1
font_ec=$?
cat "$RAW/font-build.log"
if [ "$font_ec" -ne 0 ]; then
  finish font_build "$font_ec"
fi
record font_build 0

echo "phase=fonts-after"
sha256sum \
  apps/web/public/fonts/NotoSansSC-300-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-400-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-500-subset.woff2 \
  apps/web/public/fonts/NotoSansSC-full.woff2 \
  apps/web/public/fonts/Geist-wght.woff2 \
  apps/web/public/fonts/GeistMono-Regular.woff2 \
  apps/web/public/fonts/licenses/Geist-OFL.txt \
  apps/web/public/fonts/licenses/NotoSans-LICENSE \
  > "$RAW/fonts-after.txt"
cat "$RAW/fonts-after.txt"
full_after=$(awk '/NotoSansSC-full.woff2/ {print $1}' "$RAW/fonts-after.txt")
if [ "$full_after" != "bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b" ]; then
  finish full_font 1
fi
subset_before=$(awk '/NotoSansSC-400-subset.woff2/ {print $1}' "$RAW/fonts-before.txt")
subset_after=$(awk '/NotoSansSC-400-subset.woff2/ {print $1}' "$RAW/fonts-after.txt")
if [ "$subset_before" = "$subset_after" ]; then
  echo "subset bytes did not change" | tee "$RAW/subset-unchanged.txt"
  finish subset_changed 1
fi
record fonts_after 0

cd /src/apps/web

echo "phase=vitest"
pnpm exec vitest run --config config/vitest.config.ts src/styles/font-coverage.test.ts > "$RAW/vitest.log" 2>&1
vitest_ec=$?
tail -n 40 "$RAW/vitest.log"
if [ "$vitest_ec" -ne 0 ]; then
  finish vitest "$vitest_ec"
fi
record vitest 0

echo "phase=types"
pnpm exec tsc -p tsconfig.json --noEmit --pretty false > "$RAW/types.log" 2>&1
types_ec=$?
tail -n 20 "$RAW/types.log"
if [ "$types_ec" -ne 0 ]; then
  finish types "$types_ec"
fi
record types 0

echo "phase=lint"
pnpm exec eslint --config config/eslint.config.mjs \
  src/styles/font-coverage.test.ts \
  config/build-fonts.mjs \
  design/e2e/foundation.visual.spec.ts \
  > "$RAW/lint.log" 2>&1
lint_ec=$?
tail -n 40 "$RAW/lint.log"
if [ "$lint_ec" -ne 0 ]; then
  finish lint "$lint_ec"
fi
record lint 0

echo "phase=build"
pnpm exec vite build > "$RAW/build.log" 2>&1
build_ec=$?
tail -n 30 "$RAW/build.log"
if [ "$build_ec" -ne 0 ]; then
  finish build "$build_ec"
fi
record build 0

echo "phase=bundle"
(
  cd /src/apps/web/dist
  find . -type f -printf '%s %p\n' | sort -n > "$RAW/dist-files.txt"
  js=$(find assets -name 'index-*.js' | head -n 1)
  css=$(find assets -name 'index-*.css' | head -n 1)
  html_raw=$(wc -c < index.html)
  html_gzip=$(gzip -c index.html | wc -c)
  js_raw=$(wc -c < "$js")
  js_gzip=$(gzip -c "$js" | wc -c)
  css_raw=$(wc -c < "$css")
  css_gzip=$(gzip -c "$css" | wc -c)
  printf 'html=index.html html_raw=%s html_gzip=%s\n' "$html_raw" "$html_gzip"
  printf 'js=%s js_raw=%s js_gzip=%s\n' "$js" "$js_raw" "$js_gzip"
  printf 'css=%s css_raw=%s css_gzip=%s\n' "$css" "$css_raw" "$css_gzip"
  unexpected=0
  if grep -E -q 'fonts\.googleapis|fonts\.gstatic|use\.typekit' "$js"; then
    unexpected=1
  fi
  if ! grep -q '此能力尚未接入' "$js"; then
    echo "missing not-connected copy"
    unexpected=1
  fi
  printf 'bundle_unexpected=%s\n' "$unexpected"
  exit "$unexpected"
) > "$RAW/bundle.log" 2>&1
bundle_ec=$?
cat "$RAW/bundle.log"
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
} > "$RAW/apt.log" 2>&1
apt_ec=$?
tail -n 20 "$RAW/apt.log"
if [ "$apt_ec" -ne 0 ]; then
  finish apt "$apt_ec"
fi
record apt 0

echo "phase=ldd"
missing=0
{
  find /home/fe01/ms-playwright -type f \( -name chrome -o -name headless_shell -o -name chromium \) -print
  bins=$(find /home/fe01/ms-playwright -type f \( -name chrome -o -name headless_shell -o -name chromium \) -print | wc -l)
  echo "bin_count=$bins"
  if [ "$bins" -lt 1 ]; then
    missing=1
  fi
  while IFS= read -r bin; do
    echo "BIN $bin"
    if ! ldd "$bin"; then
      missing=1
    fi
    if ldd "$bin" | grep -q 'not found'; then
      missing=1
    fi
  done < <(find /home/fe01/ms-playwright -type f \( -name chrome -o -name headless_shell -o -name chromium \) -print)
  printf 'ldd_missing=%s\n' "$missing"
} > "$RAW/ldd.log" 2>&1
tail -n 30 "$RAW/ldd.log"
if [ "$missing" -ne 0 ]; then
  finish ldd 1
fi
record ldd 0

echo "phase=playwright"
pnpm exec playwright test --config config/playwright.visual.config.ts -g production > "$RAW/playwright.log" 2>&1
pw_ec=$?
tail -n 60 "$RAW/playwright.log"
if [ "$pw_ec" -ne 0 ]; then
  finish playwright "$pw_ec"
fi
record playwright 0

echo "phase=lock-after"
lock_after=$(sha256sum /src/pnpm-lock.yaml | awk 'NR==1 {print $1}')
printf '%s\n' "$lock_after" > "$RAW/lock-after.txt"
if [ "$lock_after" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  finish lock_after 1
fi
record lock_after 0
echo "DONE"
exit 0
