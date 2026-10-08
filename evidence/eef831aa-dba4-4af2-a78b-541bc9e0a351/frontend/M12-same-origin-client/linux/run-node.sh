#!/bin/bash
# Adapter plus the SDK client drive. No browser and no font rebuild.
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
export NODE_EXTRA_CA_CERTS=/secrets/ca.crt
export FE01_EVIDENCE=/evidence
export FE01_B01_ORIGIN=https://oo-fe01-m12-edge:8443
export FE01_BOOTSTRAP_FILE=/secrets/bootstrap.token
mkdir -p /evidence/raw
{
  echo "memory.max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || echo missing)"
  echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing)"
  node --version
} > /evidence/raw/node-cgroup.txt
cat > /evidence/raw/adapter.used.json <<'EOF'
{
  "pageOrigin": null,
  "listen": { "host": "0.0.0.0", "port": 8443 },
  "tls": { "certFile": "/secrets/edge.crt", "keyFile": "/secrets/edge.key" },
  "identity": {
    "upstream": "https://oo-fe01-m12-proof:7443",
    "caFile": "/secrets/ca.crt",
    "servername": "oo-fe01-m12-proof"
  },
  "static": { "upstream": "http://127.0.0.1:9" },
  "gateway": { "upstream": null },
  "ws": { "upstream": null },
  "routing": {
    "identity": ["/control", "/control/"],
    "api": ["/v1", "/v1/", "/api", "/api/"],
    "oidc": ["/.well-known", "/.well-known/", "/oauth", "/oauth/", "/oidc", "/oidc/"],
    "ws": ["/ws", "/ws/"],
    "assets": ["/fonts/", "/assets/"],
    "spaFallback": "page-deep-link-only"
  }
}
EOF
OO_B01_ADAPTER_CONFIG=/evidence/raw/adapter.used.json node /src/apps/web/deploy/b01-adapter.mjs > /evidence/raw/adapter.log 2>&1 &
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if node --input-type=module -e 'const res = await fetch(process.env.FE01_B01_ORIGIN + "/control/v1/identity/current", { redirect: "manual" }); if (res.status !== 401) process.exit(1);'; then
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
cd /src/apps/web
set +e
pnpm exec vitest run --config config/vitest.drive.config.ts > /evidence/raw/drive.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /evidence/raw/drive.exit
touch /evidence/stop
exit "$code"
