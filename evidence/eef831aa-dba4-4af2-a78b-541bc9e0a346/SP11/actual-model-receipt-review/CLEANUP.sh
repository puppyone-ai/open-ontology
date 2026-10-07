#!/bin/bash
# Remove only a346 resources. Do not open auth stores. Do not prune.
# Pid files are in the evidence tree. macOS ps comm is the full path,
# so the stop check uses the basename.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346
fail=0
{
  echo "auth_store_touched=false"
  echo "auth_store_paths_opened=0"
} > "$EVIDENCE/CLEANUP.txt"

stop_pid() {
  local file="$1"
  local expect="$2"
  if [ ! -f "$file" ]; then
    echo "pid_file_absent=${file}" >> "$EVIDENCE/CLEANUP.txt"
    fail=1
    return
  fi
  local pid
  pid=$(tr -d '[:space:]' < "$file")
  if [ -z "$pid" ]; then
    echo "pid_empty=${expect}" >> "$EVIDENCE/CLEANUP.txt"
    fail=1
    return
  fi
  local comm base
  comm=$(ps -p "$pid" -o comm= 2>/dev/null || true)
  base=${comm##*/}
  base=$(printf '%s' "$base" | tr -d '[:space:]')
  case "$base" in
    "$expect"*)
      kill "$pid" 2>/dev/null || true
      local i=0
      while [ "$i" -lt 25 ]; do
        if ! ps -p "$pid" >/dev/null 2>&1; then
          break
        fi
        sleep 0.2
        i=$((i + 1))
      done
      if ps -p "$pid" >/dev/null 2>&1; then
        kill -9 "$pid" 2>/dev/null || true
        sleep 0.2
      fi
      if ps -p "$pid" >/dev/null 2>&1; then
        echo "stop_failed=${expect} pid=${pid}" >> "$EVIDENCE/CLEANUP.txt"
        fail=1
      else
        echo "stopped=${expect} pid=${pid}" >> "$EVIDENCE/CLEANUP.txt"
      fi
      ;;
    "")
      echo "already_absent=${expect} pid=${pid}" >> "$EVIDENCE/CLEANUP.txt"
      ;;
    *)
      echo "refused_stop=${expect} pid=${pid} comm=${base}" >> "$EVIDENCE/CLEANUP.txt"
      fail=1
      ;;
  esac
}

stop_pid "$EVIDENCE/host/relay.pid" oo-local-llm-relay
stop_pid "$EVIDENCE/host/observer.pid" a346-observe

if docker container inspect oo-a346-pg >/dev/null 2>&1; then
  owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' oo-a346-pg)
  if [ "$owner" = a346 ]; then
    if docker rm -f oo-a346-pg >> "$EVIDENCE/CLEANUP.txt" 2>&1; then
      echo "pg_removed=oo-a346-pg" >> "$EVIDENCE/CLEANUP.txt"
    else
      echo "pg_remove_failed=oo-a346-pg" >> "$EVIDENCE/CLEANUP.txt"
      fail=1
    fi
  else
    echo "refused_pg_owner=${owner}" >> "$EVIDENCE/CLEANUP.txt"
    fail=1
  fi
else
  echo "pg_absent=oo-a346-pg" >> "$EVIDENCE/CLEANUP.txt"
fi

for name in oo-a346-smoke oo-a346-propose oo-a346-execute; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name")
    if [ "$owner" = a346 ]; then
      if ! docker rm -f "$name" >> "$EVIDENCE/CLEANUP.txt" 2>&1; then
        echo "container_remove_failed=${name}" >> "$EVIDENCE/CLEANUP.txt"
        fail=1
      fi
    else
      echo "refused_container=${name} owner=${owner}" >> "$EVIDENCE/CLEANUP.txt"
      fail=1
    fi
  fi
done

for vol in oo-a346-eef831aa-cargo oo-a346-eef831aa-target oo-a346-eef831aa-pgdata; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol")
    if [ "$owner" = a346 ]; then
      if docker volume rm "$vol" >> "$EVIDENCE/CLEANUP.txt" 2>&1; then
        echo "volume_removed=${vol}" >> "$EVIDENCE/CLEANUP.txt"
      else
        echo "volume_remove_failed=${vol}" >> "$EVIDENCE/CLEANUP.txt"
        fail=1
      fi
    else
      echo "refused_volume=${vol} owner=${owner}" >> "$EVIDENCE/CLEANUP.txt"
      fail=1
    fi
  else
    echo "volume_absent=${vol}" >> "$EVIDENCE/CLEANUP.txt"
  fi
done

if docker network inspect oo-a346-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' oo-a346-eef831aa-net)
  if [ "$owner" = a346 ]; then
    if docker network rm oo-a346-eef831aa-net >> "$EVIDENCE/CLEANUP.txt" 2>&1; then
      echo "network_removed=oo-a346-eef831aa-net" >> "$EVIDENCE/CLEANUP.txt"
    else
      echo "network_remove_failed=oo-a346-eef831aa-net" >> "$EVIDENCE/CLEANUP.txt"
      fail=1
    fi
  else
    echo "refused_network_owner=${owner}" >> "$EVIDENCE/CLEANUP.txt"
    fail=1
  fi
else
  echo "network_absent=oo-a346-eef831aa-net" >> "$EVIDENCE/CLEANUP.txt"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  echo "removed_private_tmp=${ROOT}" >> "$EVIDENCE/CLEANUP.txt"
else
  echo "private_tmp_absent=${ROOT}" >> "$EVIDENCE/CLEANUP.txt"
fi

echo "left_in_place=proxy_8317 upstream_token_file foreign_containers" >> "$EVIDENCE/CLEANUP.txt"
echo "prune=no" >> "$EVIDENCE/CLEANUP.txt"
if [ "$fail" -ne 0 ]; then
  echo "cleanup_incomplete" >> "$EVIDENCE/CLEANUP.txt"
  exit 1
fi
echo "cleanup_done" >> "$EVIDENCE/CLEANUP.txt"
