#!/bin/bash
set -eu
mkdir -p /evidence/raw /run/fe01 /work/tree /work/b04 /work/b04-work /work/b04-missing
rm -f /evidence/stop /evidence/ready.json /evidence/subject.request /evidence/subject.applied
rm -rf /work/tree /work/b04
mkdir -p /work/tree /work/b04 /work/b04-work /work/b04-missing
cp -R /snapshot/tools/control-identity/. /work/tree/
mkdir -p /work/tree/proof-bin
cp /evidence/harness/main.rs /work/tree/proof-bin/main.rs
printf '\n[[bin]]\nname = "fe01-b01-hold"\npath = "proof-bin/main.rs"\n' >> /work/tree/Cargo.toml
cp -R /snapshot-b04/tools/selfbuild-control-api/. /work/b04/
cd /work/tree
cargo build --locked --bin control-identity --bin fe01-b01-hold
cd /work/b04
cargo build --locked --bin selfbuild-control-api
export CONTROL_IDENTITY_BIN=/work/target/debug/control-identity
cat > /evidence/raw/b04-config.json <<'EOF'
{
  "contract": "oo.selfbuild-control-api.config/v1",
  "deployment_origins": ["https://oo-fe01-m14-edge:8443"],
  "listen": {
    "bind": "0.0.0.0:7444",
    "cert_file": "/secrets/b04.crt",
    "key_file": "/secrets/b04.key"
  },
  "identity": {
    "origin": "https://oo-fe01-m14-proof:7443",
    "ca_file": "/secrets/ca.crt"
  },
  "postgres": {
    "host": "oo-fe01-m14-pg",
    "port": 5432,
    "database": "selfbuild",
    "user": "oo_selfbuild_api_admin",
    "password_file": "/secrets/selfbuild.pass",
    "ca_file": "/secrets/ca.crt",
    "server_name": "oo-fe01-m14-pg"
  },
  "work_dir": "/work/b04-work",
  "binaries": {
    "catalog": "/work/b04-missing/catalog",
    "pg_builder": "/work/b04-missing/pg-builder",
    "rules": "/work/b04-missing/rules",
    "suggestions": "/work/b04-missing/suggestions",
    "review_ledger": "/work/b04-missing/review-ledger"
  },
  "projects": [
    {
      "project_id": "alpha",
      "config_commit": "0000000000000000000000000000000000000000000000000000000000000000",
      "source_ref": "source_a",
      "allowance_id": "allow_a",
      "llm_secret_ref": "secret://llm/unused",
      "source_secret_ref": "secret://source/unused",
      "source": {
        "host": "oo-fe01-m14-pg",
        "port": 5432,
        "database": "ledger",
        "username": "oo_selfbuild_api_admin",
        "password_file": "/secrets/selfbuild.pass",
        "ca_file": "/secrets/ca.crt",
        "tables": [{ "schema": "public", "table": "party" }]
      },
      "sample_policy": "none",
      "rules_auto_accept": "none",
      "llm": {
        "bearer_file": "/secrets/selfbuild.pass",
        "origin": "http://127.0.0.1:8317",
        "model_id": "grok-4.7"
      },
      "role_policy_file": "/work/b04-work/role-policy.json",
      "hidden_columns": [],
      "visible_columns": ["public.party.id"]
    }
  ],
  "ledger": {
    "host": "oo-fe01-m14-pg",
    "port": 5432,
    "database": "ledger",
    "user": "oo_selfbuild_api_admin",
    "password_file": "/secrets/selfbuild.pass",
    "ca_file": "/secrets/ca.crt",
    "server_name": "oo-fe01-m14-pg"
  },
  "principals": [
    {
      "subject": "fixture-owner",
      "ledger_role": "owner",
      "principal_id": "owner_a",
      "reviewer_label": "owner-label",
      "credential_file": "/secrets/owner.cred",
      "object_scope": ["public.party.id"]
    }
  ]
}
EOF
if ! /work/target/debug/selfbuild-control-api install --config /evidence/raw/b04-config.json > /evidence/raw/b04-install.txt 2>&1; then
  printf '%s\n' 1 > /evidence/raw/b04-install.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/b04-install.exit
/work/target/debug/selfbuild-control-api serve --config /evidence/raw/b04-config.json > /evidence/raw/b04-serve.log 2>&1 &
echo $! > /evidence/raw/b04-serve.pid
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if grep -q '"operation":"serve"' /evidence/raw/b04-serve.log 2>/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != 1 ]; then
  printf '%s\n' 1 > /evidence/raw/b04-serve.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/raw/b04-serve.exit
exec /work/target/debug/fe01-b01-hold
