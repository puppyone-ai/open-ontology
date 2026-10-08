#!/bin/bash
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
export PLAYWRIGHT_BROWSERS_PATH=/home/fe01/ms-playwright
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
export NODE_EXTRA_CA_CERTS=/secrets/ca.crt
export FE01_EVIDENCE=/evidence
export FE01_B01_ORIGIN=https://oo-fe01-m7-edge:8443
export FE01_BOOTSTRAP_FILE=/secrets/bootstrap.token
mkdir -p /evidence/raw /evidence/screenshots /home/fe01/apt /etc/apt/apt.conf.d
{
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo missing)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing)"
} > /evidence/raw/node-cgroup.txt
shell=$(find /home/fe01/ms-playwright -type f -name chrome-headless-shell -print | head -n 1 || true)
if [ -z "$shell" ]; then
  echo "browser cache missing" > /evidence/raw/browser-missing.txt
  touch /evidence/stop
  exit 1
fi
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
ldd "$shell" > /evidence/raw/ldd.log 2>&1 || true
if grep -q 'not found' /evidence/raw/ldd.log; then
  touch /evidence/stop
  exit 1
fi
cd /src/apps/web
set +e
pnpm exec vite build > /evidence/raw/build.log 2>&1
printf '%s\n' "$?" > /evidence/raw/build.exit
set -e
if [ "$(cat /evidence/raw/build.exit)" != 0 ]; then
  touch /evidence/stop
  exit 1
fi
cat > /evidence/raw/adapter.used.json <<'EOF'
{
  "pageOrigin": null,
  "listen": { "host": "0.0.0.0", "port": 8443 },
  "tls": { "certFile": "/secrets/edge.crt", "keyFile": "/secrets/edge.key" },
  "identity": {
    "upstream": "https://oo-fe01-m7-proof:7443",
    "caFile": "/secrets/ca.crt",
    "servername": "oo-fe01-m7-proof"
  },
  "static": { "upstream": "http://127.0.0.1:4173" }
}
EOF
pnpm exec vite preview --host 127.0.0.1 --port 4173 --strictPort > /evidence/raw/vite.log 2>&1 &
OO_B01_ADAPTER_CONFIG=/evidence/raw/adapter.used.json node deploy/b01-adapter.mjs > /evidence/raw/adapter.log 2>&1 &
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
  touch /evidence/stop
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/edge.exit
set +e
pnpm exec playwright test -c playwright.b01-loop.config.ts > /evidence/raw/playwright.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /evidence/raw/playwright.exit
touch /evidence/stop
exit "$code"
