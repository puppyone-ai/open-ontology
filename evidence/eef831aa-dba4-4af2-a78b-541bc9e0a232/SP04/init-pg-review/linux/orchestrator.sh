#!/bin/bash
# Sequential a232 review: private network, one PostgreSQL, smoke, then proof.
# Does not prune. Does not pull. Does not edit product source.
set -u
SRC=/Users/redux/puppy-one/open-ontology
ROOT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review
NET=oo-a232-eef831aa-net
LOG=${ROOT}/linux/orchestrator.log

mkdir -p "${ROOT}/linux"
exec > >(tee -a "$LOG") 2>&1
echo "orchestrator_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a232 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a232 \
  --label oo.kind=network \
  "$NET" >/dev/null

bash "${ROOT}/pg/up.sh"
pg_exit=$?
echo "PHASE pg_exit=$pg_exit"
if [ "$pg_exit" -ne 0 ]; then
  echo "orchestrator_stop pg"
  exit "$pg_exit"
fi

bash "${ROOT}/linux/smoke/launch.sh"
smoke_exit=$?
echo "PHASE smoke_exit=$smoke_exit"
if [ "$smoke_exit" -ne 0 ]; then
  echo "orchestrator_stop smoke"
  exit "$smoke_exit"
fi

bash "${ROOT}/linux/proof/launch.sh"
proof_exit=$?
echo "PHASE proof_exit=$proof_exit"
echo "orchestrator_stop proof"
exit "$proof_exit"
