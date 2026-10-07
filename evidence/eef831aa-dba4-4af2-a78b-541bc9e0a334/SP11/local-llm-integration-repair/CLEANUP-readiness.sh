#!/bin/bash
# Remove this phase's relay, Postgres, volumes, network, and private tmp.
# Leave the 6-byte token, the proxy, and every foreign container.
set -u
export PATH="/usr/bin:/bin:/usr/sbin:/usr/local/bin:/opt/homebrew/bin"
EV=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair
ROOT=/var/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
OUT="$EV/CLEANUP-readiness.txt"
ERR="$EV/CLEANUP-readiness.stderr"
: > "$ERR"
{
  echo "cleanup_readiness_start"
  pid=$(cat "$EV/host-3/relay.pid" 2>/dev/null || true)
  if [ -n "${pid:-}" ]; then
    args=$(ps -p "$pid" -o args= 2>/dev/null || true)
    case "$args" in
      *"$ROOT/host-3/target/debug/oo-local-llm-relay"*)
        kill "$pid" 2>>"$ERR" || true
        echo "relay_stopped=$pid"
        ;;
      *)
        echo "relay_pid_not_ours"
        ;;
    esac
  else
    echo "relay_pid_absent"
  fi
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if ! lsof -nP -iTCP:18317 -sTCP:LISTEN >/dev/null 2>&1; then
      echo "relay_listen_clear"
      break
    fi
    sleep 0.2
  done
  if docker ps -a --filter "name=^/oo-a334-pg$" --format '{{.Names}}' | grep -qx oo-a334-pg; then
    docker rm -f oo-a334-pg >>"$ERR" 2>&1
    echo "pg_rm=$?"
  else
    echo "pg_absent"
  fi
  for vol in oo-a334-eef831aa-cargo oo-a334-eef831aa-target oo-a334-eef831aa-dialogue-target oo-a334-eef831aa-pgdata; do
    if docker volume inspect "$vol" >/dev/null 2>&1; then
      docker volume rm "$vol" >>"$ERR" 2>&1
      echo "volume_removed=$vol"
    else
      echo "volume_absent=$vol"
    fi
  done
  if docker network inspect oo-a334-eef831aa-net >/dev/null 2>&1; then
    docker network rm oo-a334-eef831aa-net >>"$ERR" 2>&1
    echo "network_removed"
  else
    echo "network_absent"
  fi
  if [ -d "$ROOT" ]; then
    rm -rf "$ROOT"
    echo "tmp_removed"
  else
    echo "tmp_absent"
  fi
  echo "a334_containers=$(docker ps -a --filter name=oo-a334 --format '{{.Names}}' | tr '\n' ' ')"
  echo "a334_volumes=$(docker volume ls --filter name=oo-a334 --format '{{.Name}}' | tr '\n' ' ')"
  if lsof -nP -iTCP:18317 -sTCP:LISTEN >/dev/null 2>&1; then
    echo "relay_listen_still_open"
  else
    echo "relay_listen_clear"
  fi
  if [ -f "$TOKEN" ]; then
    stat -f 'token_mode_bytes=%OLp %z' "$TOKEN"
  else
    echo "token_missing"
  fi
  echo "auth_store_touched=false"
  echo "cleanup_readiness_done"
} > "$OUT"
exit 0
