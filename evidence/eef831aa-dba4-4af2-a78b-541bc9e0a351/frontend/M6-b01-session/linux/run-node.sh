#!/bin/bash
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
export PLAYWRIGHT_BROWSERS_PATH=/home/fe01/ms-playwright
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
export NODE_EXTRA_CA_CERTS=/secrets/ca.crt
export FE01_EVIDENCE=/evidence
export FE01_B01_ORIGIN=https://oo-fe01-edge:8443
export FE01_BOOTSTRAP_FILE=/secrets/bootstrap.token
mkdir -p /evidence/raw /evidence/screenshots /home/fe01/apt /etc/apt/apt.conf.d
rm -f /evidence/raw/vitest.exit /evidence/raw/build.exit /evidence/raw/playwright.exit /evidence/raw/edge.exit /evidence/stop
printf '%s\n' 'APT::Sandbox::User "root";' 'Dir::Cache::Archives "/home/fe01/apt";' > /etc/apt/apt.conf.d/00fe01
apt-get update > /evidence/raw/apt.log 2>&1
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  libasound2 libatk-bridge2.0-0 libatk1.0-0 libatspi2.0-0 libcups2 \
  libdbus-1-3 libdrm2 libgbm1 libnspr4 libnss3 libnss3-tools libxcomposite1 \
  libxdamage1 libxfixes3 libxi6 libxkbcommon0 libxrandr2 libxshmfence1 \
  >> /evidence/raw/apt.log 2>&1
rm -rf /home/fe01/.pki/nssdb
mkdir -p /home/fe01/.pki/nssdb
timeout 20 certutil --empty-password -N -d sql:/home/fe01/.pki/nssdb </dev/null
timeout 20 certutil -A -d sql:/home/fe01/.pki/nssdb -t "C,," -n fe01ca -i /secrets/ca.crt </dev/null
certutil -L -d sql:/home/fe01/.pki/nssdb > /evidence/raw/nss-list.txt
shell=$(find /home/fe01/ms-playwright -type f -name chrome-headless-shell -print | head -n 1)
ldd "$shell" > /evidence/raw/ldd.log 2>&1 || true
if grep -q 'not found' /evidence/raw/ldd.log; then
  touch /evidence/stop
  exit 1
fi
cd /src/apps/web
set +e
pnpm exec vitest run --config config/vitest.config.ts src/api/identity-session.test.ts src/app/router.test.tsx src/app/runtime-config.test.ts > /evidence/raw/vitest.log 2>&1
printf '%s\n' "$?" > /evidence/raw/vitest.exit
pnpm exec vite build > /evidence/raw/build.log 2>&1
printf '%s\n' "$?" > /evidence/raw/build.exit
set -e
if [ "$(cat /evidence/raw/vitest.exit)" != 0 ] || [ "$(cat /evidence/raw/build.exit)" != 0 ]; then
  touch /evidence/stop
  exit 1
fi
pnpm exec vite preview --host 127.0.0.1 --port 4173 --strictPort > /evidence/raw/vite.log 2>&1 &
node /evidence/proxy/edge.mjs > /evidence/raw/edge.log 2>&1 &
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if node --input-type=module -e 'const res = await fetch(process.env.FE01_B01_ORIGIN + "/login"); if (!res.ok) process.exit(1);'; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != 1 ]; then
  printf '%s\n' 1 > /evidence/raw/edge.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/edge.exit
set +e
pnpm exec playwright test -c playwright.b01.config.ts > /evidence/raw/playwright.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /evidence/raw/playwright.exit
touch /evidence/stop
exit "$code"
