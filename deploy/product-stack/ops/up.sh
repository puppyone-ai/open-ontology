#!/bin/bash
# dev profile plus fixed service profiles whose local images exist.
# standard, minimal, and source-fixed stay off.
# release-fixed and selfbuild are added by load_profiles when those local images exist.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run" "${STATE}/install"
load_profiles
printf '%s\n' "${COMPOSE_PROFILES[@]}" > "${EVIDENCE}/run/profiles.txt"
if docker ps -aq --filter name=^/${PG_NAME}$ | grep -q . && ! docker ps -q --filter name=^/${PG_NAME}$ | grep -q .; then
  echo "owned postgres name is inconsistent" >&2
  exit 2
fi
count=$(docker ps -q --filter label=oo.owner=a395 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$count" -gt 1 ]; then
  echo "refusing a second owned postgres" >&2
  exit 2
fi
compose --profile dev up -d control-db > "${EVIDENCE}/run/up-db.log" 2>&1
ready=0
for _ in $(seq 1 40); do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" != 1 ]; then
  printf '%s\n' 1 > "${EVIDENCE}/run/pg-ready.exit"
  exit 1
fi
printf '%s\n' 0 > "${EVIDENCE}/run/pg-ready.exit"
if [ ! -f "${STATE}/install/marker" ]; then
  if docker image inspect oo-a395-identity:local >/dev/null 2>&1; then
    set +e
    compose --profile dev --profile identity run --rm --no-deps --entrypoint /usr/local/bin/service control-identity install --config /config/identity.json > "${EVIDENCE}/run/identity-install.log" 2>&1
    code=$?
    set -e
    printf '%s\n' "$code" > "${EVIDENCE}/run/identity-install.exit"
  fi
  if psql_admin -Atqc 'SELECT 1' >/dev/null 2>&1; then
    set +e
    psql_admin -f - < "${ROOT}/tools/audit-ingest/migrations/oo_audit_ingest_v1.sql" > "${EVIDENCE}/run/audit-install.log" 2>&1
    code=$?
    set -e
    printf '%s\n' "$code" > "${EVIDENCE}/run/audit-install.exit"
  else
    printf '%s\n' 2 > "${EVIDENCE}/run/audit-install.exit"
  fi
  if docker image inspect oo-a395-session:local >/dev/null 2>&1; then
    set +e
    compose --profile dev --profile session run --rm --no-deps --entrypoint bash session-control-api -c 'exec /usr/local/bin/service install < /config/session.json' > "${EVIDENCE}/run/session-install.log" 2>&1
    code=$?
    set -e
    printf '%s\n' "$code" > "${EVIDENCE}/run/session-install.exit"
  fi
  if docker image inspect oo-a395-release:local >/dev/null 2>&1; then
    set +e
    compose --profile dev --profile identity --profile release-fixed run --rm --no-deps --entrypoint /usr/local/bin/service release-control-api install --config /config/release.json > "${EVIDENCE}/run/release-install.log" 2>&1
    code=$?
    set -e
    printf '%s\n' "$code" > "${EVIDENCE}/run/release-install.exit"
  fi
  if docker image inspect oo-a395-selfbuild:local >/dev/null 2>&1; then
    set +e
    compose --profile dev --profile identity --profile selfbuild run --rm --no-deps --entrypoint /usr/local/bin/service selfbuild-control-api install --config /config/selfbuild.json > "${EVIDENCE}/run/selfbuild-install.log" 2>&1
    code=$?
    set -e
    printf '%s\n' "$code" > "${EVIDENCE}/run/selfbuild-install.exit"
  fi
  : > "${STATE}/install/marker"
fi
compose "${COMPOSE_ARGS[@]}" up -d > "${EVIDENCE}/run/up.log" 2>&1
printf '%s\n' 0 > "${EVIDENCE}/run/up.exit"
echo "up_ok"
