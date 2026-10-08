#!/bin/bash
# Shared paths for the a395 product stack. Secrets stay under /tmp, never $HOME.
set -euo pipefail
ROOT=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a395
STATE=/tmp/oo-a395-${UUID}
EVIDENCE=${ROOT}/evidence/${UUID}/SP14/product-stack
STACK=${ROOT}/deploy/product-stack
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET_CTL=oo-a395-ctl
PG_NAME=oo-a395-control-db

load_profiles() {
  export OO_A395_STATE="$STATE"
  export OO_A395_ROOT="$ROOT"
  export OO_A395_IDENTITY_IMAGE=oo-a395-identity:absent
  export OO_A395_AUDIT_IMAGE=oo-a395-audit:absent
  export OO_A395_SESSION_IMAGE=oo-a395-session:absent
  export OO_A395_SELFBUILD_IMAGE=oo-a395-selfbuild:absent
  export OO_A395_SOURCE_IMAGE=oo-a395-source:absent
  export OO_A395_RELEASE_IMAGE=oo-a395-release:absent
  COMPOSE_PROFILES=(dev)
  if docker image inspect oo-a395-identity:local >/dev/null 2>&1; then
    export OO_A395_IDENTITY_IMAGE=oo-a395-identity:local
    COMPOSE_PROFILES+=(identity)
  fi
  if docker image inspect oo-a395-audit:local >/dev/null 2>&1; then
    export OO_A395_AUDIT_IMAGE=oo-a395-audit:local
    COMPOSE_PROFILES+=(audit)
  fi
  if docker image inspect oo-a395-session:local >/dev/null 2>&1; then
    export OO_A395_SESSION_IMAGE=oo-a395-session:local
    COMPOSE_PROFILES+=(session)
  fi
  COMPOSE_ARGS=()
  local profile
  for profile in "${COMPOSE_PROFILES[@]}"; do
    COMPOSE_ARGS+=(--profile "$profile")
  done
}

compose() {
  docker compose -f "${STACK}/compose/compose.yml" "$@"
}

oracle_bin() {
  if [ -x "${STATE}/bin/oo-product-stack" ]; then
    printf '%s\n' "${STATE}/bin/oo-product-stack"
  elif [ -x /tmp/oo-a395-oracle ]; then
    printf '%s\n' /tmp/oo-a395-oracle
  else
    return 1
  fi
}

oracle() {
  local bin
  bin=$(oracle_bin) || {
    echo "oracle binary is absent" >&2
    return 1
  }
  docker run --rm -i --platform linux/arm64 --pull never --network none \
    --user 65534:65534 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 256m --memory-swap 256m --cpus 1 --pids-limit 128 \
    -e HOME=/tmp \
    --mount "type=bind,source=${bin},target=/oracle,readonly" \
    --tmpfs /tmp:rw,nosuid,nodev,noexec,size=32m \
    --label oo.project=open-ontology \
    --label oo.owner=a395 \
    --label oo.session="${UUID}" \
    --label oo.kind=oracle \
    "${RUST_IMAGE}" /oracle "$@"
}

secret_tree_sha() {
  python3 - "${STATE}/secrets" <<'PY'
import hashlib, os, sys
root = sys.argv[1]
digest = hashlib.sha256()
for dirpath, names, files in os.walk(root):
    for name in sorted(files):
        path = os.path.join(dirpath, name)
        mode = oct(os.stat(path).st_mode & 0o777)[2:]
        digest.update(os.path.relpath(path, root).encode())
        digest.update(b"\0")
        digest.update(mode.encode())
        digest.update(b"\0")
        with open(path, "rb") as handle:
            digest.update(handle.read())
        digest.update(b"\0")
print(digest.hexdigest())
PY
}

psql_admin() {
  docker exec -i -u root "${PG_NAME}" bash -c 'export PGPASSWORD="$(cat /secrets/fixture-admin.pass)"; export PGSSLMODE=disable; exec psql -h 127.0.0.1 -U oo_fixture_admin -d oo_a395 -v ON_ERROR_STOP=1 "$@"' bash "$@"
}

row_snapshot() {
  local query
  query=$(psql_admin -Atqc "SELECT COALESCE(string_agg(format('SELECT %L || ''='' || count(*)::text FROM %I.%I', n.nspname || '.' || c.relname, n.nspname, c.relname), ' UNION ALL ' ORDER BY n.nspname, c.relname), 'SELECT ''none=0''') FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind = 'r' AND n.nspname LIKE 'oo\\_%'") || return 0
  psql_admin -Atqc "${query}" || true
}
