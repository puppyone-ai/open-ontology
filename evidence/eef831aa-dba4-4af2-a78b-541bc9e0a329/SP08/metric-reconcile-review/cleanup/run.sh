#!/bin/bash
# Remove only a329 review resources. No prune. No other owner's containers.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a329
PG_EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP08/metric-reconcile-review/cleanup
mkdir -p "$PG_EVIDENCE"
{
  docker rm -f oo-a329-review >/dev/null 2>&1 || true
  docker rm -f oo-a329-review-2 >/dev/null 2>&1 || true
  docker rm -f oo-a329-review-3 >/dev/null 2>&1 || true
  docker rm -f oo-a329-pg
  echo "pg_rm=$?"
  docker volume rm oo-a329-eef831aa-cargo
  echo "cargo_vol=$?"
  docker volume rm oo-a329-eef831aa-target
  echo "target_vol=$?"
  docker volume rm oo-a329-eef831aa-pgdata
  echo "pg_vol=$?"
  docker network rm oo-a329-eef831aa-net
  echo "net=$?"
} > "$PG_EVIDENCE/cleanup.txt" 2>&1
rm -rf "/tmp/oo-a329-${RUN_ID}"
echo "tmp_removed"
printf '%s\n' 0 > "$PG_EVIDENCE/cleanup.exit"
