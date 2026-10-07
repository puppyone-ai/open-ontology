#!/bin/bash
# Retry only the failed font-request browser test. Fonts, unit, types, lint, and build already passed.
set -u
OUT=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M4-font-motion
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
cd /src/apps/web
exec > >(tee -a "$RAW/console-retry.log") 2>&1

record() { printf '%s=%s\n' "$1" "$2" | tee -a "$RAW/exits-retry.txt"; }
finish() { record "$1" "$2"; echo "FAILED phase=$1 exit=$2"; exit "$2"; }

echo "phase=identity-retry"
{
  echo "node=$(node -v)"
  echo "pnpm=$(pnpm --version)"
  grep -E '^(CapEff|CapBnd):' /proc/1/status || true
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || true)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || true)"
  sha256sum /src/pnpm-lock.yaml
  sha256sum /src/apps/web/public/fonts/NotoSansSC-full.woff2
} > "$RAW/identity-retry.txt"
cat "$RAW/identity-retry.txt"
lock=$(sha256sum /src/pnpm-lock.yaml | awk 'NR==1 {print $1}')
full=$(sha256sum /src/apps/web/public/fonts/NotoSansSC-full.woff2 | awk 'NR==1 {print $1}')
if [ "$lock" != "73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e" ]; then
  finish lock 1
fi
if [ "$full" != "bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b" ]; then
  finish full_font 1
fi
record identity 0

echo "phase=apt-retry"
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
} > "$RAW/apt-retry.log" 2>&1
apt_ec=$?
tail -n 8 "$RAW/apt-retry.log"
if [ "$apt_ec" -ne 0 ]; then
  finish apt "$apt_ec"
fi
record apt 0

echo "phase=ldd-retry"
missing=0
chrome=/home/fe01/ms-playwright/chromium-1243/chrome-linux-arm64/chrome
ldd "$chrome" > "$RAW/ldd-retry.log" 2>&1 || missing=1
if grep -q 'not found' "$RAW/ldd-retry.log"; then
  missing=1
fi
if [ "$missing" -ne 0 ]; then
  finish ldd 1
fi
record ldd 0

echo "phase=playwright-retry"
pnpm exec playwright test --config config/playwright.visual.config.ts -g "keep send disabled" > "$RAW/playwright-retry.log" 2>&1
pw_ec=$?
tail -n 40 "$RAW/playwright-retry.log"
if [ "$pw_ec" -ne 0 ]; then
  finish playwright "$pw_ec"
fi
record playwright 0
echo "DONE"
exit 0
