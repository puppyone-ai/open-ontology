#!/bin/bash
# Remove only this review's containers, volumes, network, and /tmp tree.
# Run after console logs and the PostgreSQL log are copied and scanned.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a116
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-owner-review
test -f "${EVIDENCE}/REPORT.md"
test -f "${EVIDENCE}/live/console.log"
test -f "${EVIDENCE}/smoke/console.log"
test -f "${EVIDENCE}/live/phases/oracle-outcome.txt"
docker ps -aq --filter "name=${RUN_ID}" | while read -r id; do
  [ -n "$id" ] || continue
  docker rm -f "$id" >/dev/null
done
docker volume rm \
  "${RUN_ID}-cargo" \
  "${RUN_ID}-target" \
  "${RUN_ID}-pglog" \
  >/dev/null 2>&1 || true
docker network rm "${RUN_ID}-net" >/dev/null 2>&1 || true
rm -rf "/tmp/oo-a116-${RUN_ID}"
echo "cleaned own a116 resources"
