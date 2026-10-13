#!/bin/bash
# Create private networks, the data volume, and mode-0600 secrets. Does not start containers.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
if [ -f "${EVIDENCE}/run/init.exit" ]; then
  echo "refusing to overwrite init.exit" >&2
  exit 2
fi
umask 077
mkdir -p "${STATE}/secrets/init" "${STATE}/config" "${STATE}/bin" "${STATE}/probe" "${STATE}/packs" "${STATE}/work"
chmod 700 "${STATE}" "${STATE}/secrets" "${STATE}/config"
if [ ! -f "${STATE}/secrets/ca.crt" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/ca.key" -out "${STATE}/secrets/ca.crt" -days 7 -subj "/CN=oo-a395-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/server.key" -out "${STATE}/secrets/server.csr" -subj "/CN=control-db" >/dev/null 2>&1
  cat > "${STATE}/secrets/pg-san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:control-db
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/server.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/server.crt" -days 7 -extfile "${STATE}/secrets/pg-san.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/identity.key" -out "${STATE}/secrets/identity.csr" -subj "/CN=control-identity" >/dev/null 2>&1
  cat > "${STATE}/secrets/id-san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:control-identity
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/identity.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/identity.crt" -days 7 -extfile "${STATE}/secrets/id-san.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/audit.key" -out "${STATE}/secrets/audit.csr" -subj "/CN=audit-ingest" >/dev/null 2>&1
  cat > "${STATE}/secrets/audit-san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:audit-ingest
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/audit.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/audit.crt" -days 7 -extfile "${STATE}/secrets/audit-san.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/audit-client.key" -out "${STATE}/secrets/audit-client.csr" -subj "/CN=audit-client" >/dev/null 2>&1
  cat > "${STATE}/secrets/audit-client.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=URI:spiffe://oo-a395/audit
keyUsage=digitalSignature
extendedKeyUsage=clientAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/audit-client.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/audit-client.crt" -days 7 -extfile "${STATE}/secrets/audit-client.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/release.key" -out "${STATE}/secrets/release.csr" -subj "/CN=release-control-api" >/dev/null 2>&1
  cat > "${STATE}/secrets/release-san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:release-control-api
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/release.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/release.crt" -days 7 -extfile "${STATE}/secrets/release-san.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "${STATE}/secrets/selfbuild.key" -out "${STATE}/secrets/selfbuild.csr" -subj "/CN=selfbuild-control-api" >/dev/null 2>&1
  cat > "${STATE}/secrets/selfbuild-san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:selfbuild-control-api
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "${STATE}/secrets/selfbuild.csr" -CA "${STATE}/secrets/ca.crt" -CAkey "${STATE}/secrets/ca.key" -CAcreateserial -out "${STATE}/secrets/selfbuild.crt" -days 7 -extfile "${STATE}/secrets/selfbuild-san.cnf" >/dev/null 2>&1
  for name in fixture-admin identity-admin identity-runtime audit session-admin session-runtime session-cancel session-query token-admin token-runtime api-admin api-runtime client release-api release-producer release-reader selfbuild-api selfbuild-source llm reviewer; do
    openssl rand -hex 24 > "${STATE}/secrets/${name}.pass"
  done
fi
find "${STATE}/secrets" -maxdepth 1 -type f -exec chmod 600 {} +
chmod 700 "${STATE}/secrets/init"
chmod 644 "${STATE}/secrets/ca.crt" "${STATE}/secrets/server.crt" "${STATE}/secrets/identity.crt"

python3 - "${STATE}" <<'PY'
import pathlib, sys
state = pathlib.Path(sys.argv[1])
sec = state / "secrets"
def read(name):
    return (sec / name).read_text().strip()
names = {
    "admin": read("session-admin.pass"),
    "runtime": read("session-runtime.pass"),
    "cancel": read("session-cancel.pass"),
    "query": read("session-query.pass"),
    "token_admin": read("token-admin.pass"),
    "token_runtime": read("token-runtime.pass"),
    "api_admin": read("api-admin.pass"),
    "api_runtime": read("api-runtime.pass"),
    "identity_admin": read("identity-admin.pass"),
    "audit": read("audit.pass"),
    "fixture": read("fixture-admin.pass"),
    "release_api": read("release-api.pass"),
    "release_producer": read("release-producer.pass"),
    "release_reader": read("release-reader.pass"),
    "selfbuild_api": read("selfbuild-api.pass"),
    "selfbuild_source": read("selfbuild-source.pass"),
}
sql = f"""CREATE ROLE oo_identity_runtime NOLOGIN;
CREATE ROLE oo_identity_admin LOGIN PASSWORD '{names["identity_admin"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE audit_ingest LOGIN PASSWORD '{names["audit"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_admin LOGIN PASSWORD '{names["admin"]}';
CREATE ROLE oo_session_runtime LOGIN PASSWORD '{names["runtime"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_fixture_query LOGIN PASSWORD '{names["query"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_cancel LOGIN PASSWORD '{names["cancel"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT oo_fixture_query TO oo_session_cancel;
CREATE ROLE oo_token_admin LOGIN PASSWORD '{names["token_admin"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_token_runtime LOGIN PASSWORD '{names["token_runtime"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_api_admin LOGIN PASSWORD '{names["api_admin"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_api_runtime LOGIN PASSWORD '{names["api_runtime"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_release_api_admin LOGIN PASSWORD '{names["release_api"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_release_admin LOGIN PASSWORD '{names["release_producer"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_release_reader LOGIN PASSWORD '{names["release_reader"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_selfbuild_api_admin LOGIN PASSWORD '{names["selfbuild_api"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_selfbuild_source LOGIN PASSWORD '{names["selfbuild_source"]}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE DATABASE oo_a395 OWNER oo_session_admin;
REVOKE ALL ON DATABASE oo_a395 FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a395 TO oo_session_admin, oo_token_admin, oo_session_api_admin, oo_identity_admin, oo_release_admin, oo_release_api_admin, oo_selfbuild_api_admin;
GRANT CONNECT ON DATABASE oo_a395 TO oo_session_runtime, oo_fixture_query, oo_session_cancel, oo_token_runtime, oo_session_api_runtime, audit_ingest, oo_release_reader, oo_selfbuild_source;
"""
path = sec / "init" / "01-roles.sql"
path.write_text(sql)
path.chmod(0o600)
PY

commit=$(oracle identity-commit <<'EOF'
{"audience":"oo-a395","client_id":"oo-a395-client","issuer":"https://idp.oo-a395.invalid","revision":1,"redirect_uris":["https://control-identity:8443/control/v1/auth/callback"],"bindings":[{"project_id":"alpha","role":"admin","subject":"fixture-admin"}],"bootstrap":{"project_id":"alpha","role":"admin","subject":"fixture-admin"}}
EOF
)
python3 - "${STATE}" "${commit}" <<'PY'
import json, pathlib, sys
state = pathlib.Path(sys.argv[1])
commit = json.loads(sys.argv[2])["config_commit"]
config = {
    "https_origin": "https://control-identity:8443",
    "listen_host": "0.0.0.0",
    "listen_port": 8443,
    "tls_cert_file": "/secrets/identity.crt",
    "tls_key_file": "/secrets/identity.key",
    "issuer": "https://idp.oo-a395.invalid",
    "audience": "oo-a395",
    "client_id": "oo-a395-client",
    "client_secret_file": "/secrets/client.pass",
    "redirect_uris": ["https://control-identity:8443/control/v1/auth/callback"],
    "config_commit": commit,
    "revision": 1,
    "idp_ca_file": "/secrets/ca.crt",
    "pg": {
        "host": "control-db",
        "port": 5432,
        "database": "oo_a395",
        "user": "oo_identity_admin",
        "password_file": "/secrets/identity-admin.pass",
        "ca_file": "/secrets/ca.crt",
    },
    "bindings": [{"subject": "fixture-admin", "project_id": "alpha", "role": "admin"}],
    "bootstrap": {"subject": "fixture-admin", "project_id": "alpha", "role": "admin"},
    "idle_seconds": 600,
    "absolute_seconds": 3600,
    "oauth_ttl_seconds": 60,
    "bootstrap_ttl_seconds": 300,
    "clock_skew_seconds": 30,
}
path = state / "config" / "identity.json"
path.write_text(json.dumps(config, separators=(",", ":")))
path.chmod(0o600)
(state / "config" / "audit.conf").write_text("\n".join([
    "listen=0.0.0.0:8444",
    "ca_file=/secrets/ca.crt",
    "cert_file=/secrets/audit.crt",
    "key_file=/secrets/audit.key",
    "registry_file=/config/registry.txt",
    "pg_host=control-db",
    "pg_port=5432",
    "pg_db=oo_a395",
    "pg_user=audit_ingest",
    "pg_password_file=/secrets/audit.pass",
    "pg_ca_file=/secrets/ca.crt",
    "",
]))
(state / "config" / "audit.conf").chmod(0o600)
(state / "config" / "registry.txt").write_text("spiffe://oo-a395/audit alpha\n")
(state / "config" / "registry.txt").chmod(0o600)
(state / "config" / "session-serve.sh").write_text("#!/bin/bash\nexec /usr/local/bin/service serve < /config/session.json\n")
(state / "config" / "session-serve.sh").chmod(0o755)
(state / "config" / "bindings.json").write_text(json.dumps({
    "bindings": [{
        "principal_id": "fixture-admin",
        "project_id": "alpha",
        "role": "admin",
        "claims": [["tenant", "alpha"]],
        "policy_version": "p1",
        "knowledge_version": "k1",
        "config_version": "c1",
        "capabilities": ["session.use", "query"],
    }]
}, separators=(",", ":")))
(state / "config" / "bindings.json").chmod(0o600)
ca = (state / "secrets" / "ca.crt").read_text()
def pw(name):
    return (state / "secrets" / name).read_text().strip()
session = {
    "contract": "session-control-api-v1",
    "profile": "localprocess",
    "bind": "127.0.0.1:0",
    "owner_uuid": "eef831aa-dba4-4af2-a78b-541bc9e0a387",
    "state_dir": "/state",
    "bindings_path": "/config/bindings.json",
    "audience_url": "https://control-identity:8443",
    "pg": {
        "host": "control-db",
        "port": 5432,
        "database": "oo_a395",
        "ca_pem": ca,
        "api_admin_password": pw("api-admin.pass"),
        "api_runtime_password": pw("api-runtime.pass"),
        "session_admin_password": pw("session-admin.pass"),
        "session_runtime_password": pw("session-runtime.pass"),
        "cancel_password": pw("session-cancel.pass"),
        "query_password": pw("session-query.pass"),
        "token_admin_password": pw("token-admin.pass"),
        "token_runtime_password": pw("token-runtime.pass"),
    },
}
path = state / "config" / "session.json"
path.write_text(json.dumps(session))
path.chmod(0o600)
def pg(user, password):
    return {
        "ca_file": "/secrets/ca.crt",
        "database": "oo_a395",
        "host": "control-db",
        "password_file": password,
        "port": 5432,
        "user": user,
    }
release = {
    "api_pg": pg("oo_release_api_admin", "/secrets/release-api.pass"),
    "authority_id": "shop-authority",
    "deployment_origins": ["https://release-control-api:8446"],
    "https_origin": "https://release-control-api:8446",
    "identity_ca_file": "/secrets/ca.crt",
    "identity_origin": "https://control-identity:8443",
    "listen_host": "0.0.0.0",
    "listen_port": 8446,
    "pack_dir": "/packs",
    "producer_pg": pg("oo_release_admin", "/secrets/release-producer.pass"),
    "project_id": "alpha",
    "reader_pg": pg("oo_release_reader", "/secrets/release-reader.pass"),
    "tls_cert_file": "/secrets/release.crt",
    "tls_key_file": "/secrets/release.key",
}
path = state / "config" / "release.json"
path.write_text(json.dumps(release, separators=(",", ":")))
path.chmod(0o600)
preimage = b"oo-a395-selfbuild-config-not-a-git-commit"
import hashlib
config_commit = hashlib.sha256(preimage).hexdigest()
(state / "config" / "selfbuild-commit-preimage.txt").write_text(preimage.decode() + "\n")
(state / "config" / "selfbuild-commit-preimage.txt").chmod(0o600)
(state / "config" / "role-policy.json").write_text('{"roles":[]}\n')
(state / "config" / "role-policy.json").chmod(0o600)
def endpoint(user, password):
    return {
        "host": "control-db",
        "port": 5432,
        "database": "oo_a395",
        "user": user,
        "password_file": password,
        "ca_file": "/secrets/ca.crt",
        "server_name": "control-db",
    }
selfbuild = {
    "contract": "oo.selfbuild-control-api.config/v1",
    "deployment_origins": ["https://selfbuild-control-api:8447"],
    "listen": {
        "bind": "0.0.0.0:8447",
        "cert_file": "/secrets/selfbuild.crt",
        "key_file": "/secrets/selfbuild.key",
    },
    "identity": {"origin": "https://control-identity:8443", "ca_file": "/secrets/ca.crt"},
    "postgres": endpoint("oo_selfbuild_api_admin", "/secrets/selfbuild-api.pass"),
    "work_dir": "/work",
    "model_cache_dir": "",
    "binaries": {
        "catalog": "/producers/catalog",
        "pg_builder": "/producers/pg-builder",
        "rules": "/producers/rules",
        "suggestions": "/producers/suggestions",
        "review_ledger": "/producers/review-ledger",
    },
    "projects": [{
        "project_id": "alpha",
        "config_commit": config_commit,
        "source_ref": "shop_source",
        "allowance_id": "shop_allowance",
        "llm_secret_ref": "secret://llm/bearer",
        "source_secret_ref": "secret://source/shop",
        "source": {
            "host": "control-db",
            "port": 5432,
            "database": "oo_a395",
            "username": "oo_selfbuild_source",
            "password_file": "/secrets/selfbuild-source.pass",
            "ca_file": "/secrets/ca.crt",
            "tables": [
                {"schema": "shop", "table": "party"},
                {"schema": "shop", "table": "account"},
                {"schema": "shop", "table": "entry"},
            ],
        },
        "sample_policy": "none",
        "rules_auto_accept": "none",
        "llm": {
            "bearer_file": "/secrets/llm.pass",
            "origin": "http://127.0.0.1:8317",
            "model_id": "grok-4.7",
        },
        "role_policy_file": "/config/role-policy.json",
        "hidden_columns": ["shop.entry.memo"],
        "visible_columns": ["shop.party.id", "shop.account.id"],
    }],
    "ledger": endpoint("oo_selfbuild_api_admin", "/secrets/selfbuild-api.pass"),
    "principals": [{
        "subject": "fixture-admin",
        "ledger_role": "reviewer",
        "principal_id": "fixture-admin",
        "reviewer_label": "fixture-admin",
        "credential_file": "/secrets/reviewer.pass",
        "object_scope": ["shop.party"],
    }],
}
path = state / "config" / "selfbuild.json"
path.write_text(json.dumps(selfbuild, separators=(",", ":")))
path.chmod(0o600)
print("config_commit", commit)
print("selfbuild_config_commit", config_commit)
PY

create_net() {
  local name="$1" internal="$2"
  if ! docker network inspect "$name" >/dev/null 2>&1; then
    if [ "$internal" = yes ]; then
      docker network create --internal --label oo.project=open-ontology --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=network "$name" >/dev/null
    else
      docker network create --label oo.project=open-ontology --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=network "$name" >/dev/null
    fi
  fi
}
create_net oo-a395-edge yes
create_net oo-a395-ctl yes
create_net oo-a395-pipe yes
create_net oo-a395-data-ro yes
for volume in oo-a395-eef831aa-pgdata oo-a395-eef831aa-session; do
  docker volume inspect "$volume" >/dev/null 2>&1 || docker volume create --label oo.project=open-ontology --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=volume "$volume" >/dev/null
done
find "${STATE}/secrets" "${STATE}/config" -type f -exec chmod 600 {} +
chmod 755 "${STATE}/config/session-serve.sh"
bad=0
while IFS= read -r line; do
  mode=${line%% *}
  if [ "$mode" != "600" ] && [ "$mode" != "755" ]; then
    bad=1
  fi
done < <(find "${STATE}/secrets" "${STATE}/config" -type f -exec stat -f '%Lp %N' {} +)
printf '%s\n' "$bad" > "${EVIDENCE}/run/secret-mode.bad"
printf '%s\n' 0 > "${EVIDENCE}/run/init.exit"
echo "init_ok"
