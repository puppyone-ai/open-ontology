#!/bin/bash
# Start one private PostgreSQL, then one review builder. No second builder.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a329/SP08/metric-reconcile-review
NET=oo-a329-eef831aa-net
CARGO_VOL=oo-a329-eef831aa-cargo
TARGET_VOL=oo-a329-eef831aa-target
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a329

own=$(docker ps -q --filter label=oo.owner=a329 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s\n' "$own" "$global_builders"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a329 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a329 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a329 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
bash "$REVIEW/pg/up.sh"
up_exit=$?
if [ "$up_exit" -ne 0 ]; then
  exit "$up_exit"
fi
bash "$REVIEW/linux/review/launch.sh"
exit $?
