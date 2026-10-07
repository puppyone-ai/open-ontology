#!/bin/bash
# Host transcript for one owned public-fixture. Does not create resources itself.
# The product script owns PostgreSQL, the builder, and cleanup refusal.
set -eu
set -o pipefail
REPO=/Users/redux/puppy-one/open-ontology
EV=$REPO/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a312/SP11/trusted-demo-identity
MAN=$EV/operator-manifest.json
LOG=$EV/linux/public-fixture.host.log
mkdir -p "$EV/linux"
printf '%s\n' '{"protocol":"oo.deploy.trusted-demo-deployment/v1","project":"open-ontology","fixed_source":"25dfc9022780c3deff12f37b7ad7ea4ed1fb6480","owner":"a312","run_id":"eef831aa-dba4-4af2-a78b-541bc9e0a312","source_id":"public_shop"}' > "$MAN"
own=$(docker ps -q --filter label=oo.owner=a312 --filter label=oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a312 --filter label=oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a312 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_pg=%s cap_global=10\n' "$own" "$global" "$host_cargo" "$own_pg" > "$EV/linux/public-fixture.capacity.txt"
docker ps --filter label=oo.kind=postgres --format '{{.Names}} {{.Status}}' > "$EV/linux/foreign-pg-before.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: builder capacity own=$own global=$global host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$EV/linux/public-fixture.host.exit"
  exit 3
fi
if [ -e /tmp/oo-a312-eef831aa-dba4-4af2-a78b-541bc9e0a312 ]; then
  echo "private directory already exists" >&2
  printf '%s\n' 2 > "$EV/linux/public-fixture.host.exit"
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -Eq '^oo-a312-(pg|build|smoke)$'; then
  echo "owned container name already exists" >&2
  printf '%s\n' 2 > "$EV/linux/public-fixture.host.exit"
  exit 2
fi
if docker volume ls --format '{{.Name}}' | grep -Eq '^oo-a312-eef831aa-(cargo|target|pgdata)$'; then
  echo "owned volume already exists" >&2
  printf '%s\n' 2 > "$EV/linux/public-fixture.host.exit"
  exit 2
fi
if docker network ls --format '{{.Name}}' | grep -Eq '^oo-a312-eef831aa-net$'; then
  echo "owned network already exists" >&2
  printf '%s\n' 2 > "$EV/linux/public-fixture.host.exit"
  exit 2
fi
{
  echo "start_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "head=$(git -C "$REPO" rev-parse HEAD)"
  echo "manifest=$MAN"
} > "$LOG"
cd "$REPO"
set +e
./deploy/trusted-demo/start.sh --manifest "$MAN" --public-fixture >> "$LOG" 2>&1
code=$?
set -e
echo "end_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ) exit=$code" >> "$LOG"
printf '%s\n' "$code" > "$EV/linux/public-fixture.host.exit"
exit "$code"
