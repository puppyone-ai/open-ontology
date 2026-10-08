#!/bin/bash
# Adapter plus the listBuilds drive. No browser and no font rebuild.
set -eu
export PATH=/home/fe01/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/home/fe01
export NODE_EXTRA_CA_CERTS=/secrets/ca.crt
export FE01_EVIDENCE=/evidence
export FE01_B01_ORIGIN=https://oo-fe01-m13-edge:8443
export FE01_BOOTSTRAP_FILE=/secrets/bootstrap.token
export FE01_B04_DRIVE=1
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
    "upstream": "https://oo-fe01-m13-proof:7443",
    "caFile": "/secrets/ca.crt",
    "servername": "oo-fe01-m13-proof"
  },
  "static": { "upstream": "http://127.0.0.1:9" },
  "gateway": { "upstream": null },
  "selfbuild": {
    "upstream": "https://oo-fe01-m13-b04:7444",
    "caFile": "/secrets/ca.crt",
    "servername": "oo-fe01-m13-b04"
  },
  "ws": { "upstream": null },
  "routing": {
    "selfbuild": [
      "/control/v1/builds",
      "/control/v1/builds/",
      "/control/v1/reviews",
      "/control/v1/reviews/",
      "/control/v1/classifications",
      "/control/v1/classifications/"
    ],
    "identity": ["/control", "/control/"],
    "api": ["/v1", "/v1/", "/api", "/api/"],
    "oidc": ["/.well-known", "/.well-known/", "/oauth", "/oauth/", "/oidc", "/oidc/"],
    "ws": ["/ws", "/ws/"],
    "assets": ["/fonts/", "/assets/"],
    "spaFallback": "page-deep-link-only"
  }
}
EOF
OO_B01_ADAPTER_CONFIG=/evidence/raw/adapter.used.json node /src/apps/web/deploy/b01-adapter.mjs > /evidence/raw/adapter-edge.log 2>&1 &
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if node --input-type=module -e 'try { const current = await fetch(process.env.FE01_B01_ORIGIN + "/control/v1/identity/current", { redirect: "manual" }); if (current.status !== 401) process.exit(1); const builds = await fetch(process.env.FE01_B01_ORIGIN + "/control/v1/builds?project_id=alpha", { redirect: "manual" }); const text = await builds.text(); if (builds.status !== 401 || !text.includes("oo.selfbuild-control-api/v1")) process.exit(1); } catch { process.exit(1); }' >/dev/null 2>&1; then
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
pnpm exec vitest run --config config/vitest.drive.config.ts design/drive/b04-list-drive.test.ts > /evidence/raw/drive.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /evidence/raw/drive.exit
touch /evidence/stop
exit "$code"
