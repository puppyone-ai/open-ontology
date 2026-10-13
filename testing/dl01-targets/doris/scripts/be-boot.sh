#!/bin/bash
# Cap BE memory, wait until the FE query port accepts root, then exec the
# image init_be.sh. entry_point.sh discards init_be stderr and exits if the
# backend is not Alive within 60 seconds. A heap-capped cold start does not
# fit that window. init_be.sh is the script entry_point.sh invokes: it appends
# priority_networks, registers the backend, and runs start_be.sh --console.
set -euo pipefail
conf=/opt/apache-doris/be/conf/be.conf
: "${BE_JVM_HEAP:?BE_JVM_HEAP is required}"
: "${BE_MEM_LIMIT:?BE_MEM_LIMIT is required}"
: "${FE_IP:?FE_IP is required}"
: "${BE_IP:?BE_IP is required}"
: "${BE_HEARTBEAT_PORT:?BE_HEARTBEAT_PORT is required}"
: "${QUERY_PORT:?QUERY_PORT is required}"
sed -i "s/-Xmx2048m/-Xmx${BE_JVM_HEAP}/g" "$conf"
if grep -q -- '-Xmx2048m' "$conf"; then
  echo "default 2048m BE heap is still present" >&2
  exit 1
fi
if ! grep -q '^mem_limit' "$conf"; then
  printf '\nmem_limit = %s\n' "$BE_MEM_LIMIT" >>"$conf"
fi
grep -q '^mem_limit' "$conf"
echo "be heap override applied: ${BE_JVM_HEAP} mem_limit=${BE_MEM_LIMIT}"

ready=0
for _ in $(seq 1 120); do
  if mysql -uroot -P"${QUERY_PORT}" -h"${FE_IP}" --connect-timeout=5 -e "SELECT 1" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [[ "$ready" != "1" ]]; then
  echo "fe query port did not accept a select before be init" >&2
  exit 1
fi

IFS=. read -r oct1 oct2 oct3 _ <<<"$BE_IP"
export MASTER_FE_IP="$FE_IP"
export CURRENT_BE_IP="$BE_IP"
export CURRENT_BE_PORT="$BE_HEARTBEAT_PORT"
export PRIORITY_NETWORKS="${oct1}.${oct2}.${oct3}.0/24"
echo "be priority_networks=${PRIORITY_NETWORKS}"
exec bash /usr/local/bin/init_be.sh
