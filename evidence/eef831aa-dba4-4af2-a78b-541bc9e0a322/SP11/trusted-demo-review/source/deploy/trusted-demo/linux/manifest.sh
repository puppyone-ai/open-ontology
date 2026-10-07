#!/bin/bash
# Canonical deployment manifest. Bash 3.2. No default run id.
# Callers source this file. It does not create Docker objects.
FIXED_SOURCE=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480
DEPLOY_PROJECT=open-ontology
DEPLOY_SOURCE_ID=public_shop
DEPLOY_PROTOCOL=oo.deploy.trusted-demo-deployment/v1

load_deployment() {
  local file size line prefix rest owner run_id quote expected tail nl
  file=${1:-}
  if [ -z "$file" ] || [ ! -f "$file" ] || [ -L "$file" ]; then
    echo "deployment manifest must be a regular file" >&2
    return 2
  fi
  size=$(wc -c < "$file" | tr -d ' ')
  if [ "$size" -gt 512 ]; then
    echo "deployment manifest is too large" >&2
    return 2
  fi
  if grep -q $'\r' "$file"; then
    echo "deployment manifest must be one object" >&2
    return 2
  fi
  nl=$(tr -cd '\n' < "$file" | wc -c | tr -d ' ')
  if [ "$nl" -gt 1 ]; then
    echo "deployment manifest must be one object" >&2
    return 2
  fi
  line=""
  IFS= read -r line < "$file" || true
  if [ "$nl" -eq 0 ]; then
    printf '%s' "$line" | cmp -s - "$file" || {
      echo "deployment manifest must be one object" >&2
      return 2
    }
  else
    printf '%s\n' "$line" | cmp -s - "$file" || {
      echo "deployment manifest must be one object" >&2
      return 2
    }
  fi
  prefix='{"protocol":"'"$DEPLOY_PROTOCOL"'","project":"'"$DEPLOY_PROJECT"'","fixed_source":"'"$FIXED_SOURCE"'","owner":"'
  case "$line" in
    "$prefix"*) ;;
    *)
      echo "deployment manifest fields are not the canonical object" >&2
      return 2
      ;;
  esac
  rest=${line#"$prefix"}
  owner=${rest%%\"*}
  case "$owner" in
    [a-f0-9][a-f0-9][a-f0-9][a-f0-9]) ;;
    *)
      echo "owner is not four lowercase hex digits" >&2
      return 2
      ;;
  esac
  quote='"'
  expected="${owner}${quote},${quote}run_id${quote}:${quote}"
  case "$rest" in
    "$expected"*) ;;
    *)
      echo "deployment manifest run_id field is missing" >&2
      return 2
      ;;
  esac
  rest=${rest#"$expected"}
  run_id=${rest%%\"*}
  case "$run_id" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f]-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
    *)
      echo "run_id is not a lowercase uuid" >&2
      return 2
      ;;
  esac
  if [ "$owner" != "${run_id:32:4}" ]; then
    echo "owner is not the last four digits of run_id" >&2
    return 2
  fi
  tail="${run_id}${quote},${quote}source_id${quote}:${quote}${DEPLOY_SOURCE_ID}${quote}}"
  if [ "$rest" != "$tail" ]; then
    echo "deployment manifest source_id is not public_shop" >&2
    return 2
  fi
  DEPLOY_LINE=$line
  OWNER=$owner
  RUN_ID=$run_id
  SOURCE_ID=$DEPLOY_SOURCE_ID
  PROJECT=$DEPLOY_PROJECT
}

bind_resources() {
  local repo first
  repo=$1
  if [ -z "${OWNER:-}" ] || [ -z "${RUN_ID:-}" ]; then
    echo "deployment is not loaded" >&2
    return 2
  fi
  first=${RUN_ID%%-*}
  case "$first" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
    *)
      echo "run_id prefix is not eight hex digits" >&2
      return 2
      ;;
  esac
  NET="oo-${OWNER}-${first}-net"
  CARGO_VOL="oo-${OWNER}-${first}-cargo"
  TARGET_VOL="oo-${OWNER}-${first}-target"
  PG_VOL="oo-${OWNER}-${first}-pgdata"
  PG_NAME="oo-${OWNER}-pg"
  PG_HOST="oo-${OWNER}-pg"
  ROOT="/tmp/oo-${OWNER}-${RUN_ID}"
  EVIDENCE="${repo}/evidence/${RUN_ID}/SP11/trusted-demo-identity"
  BUILD_PUBLIC="oo-${OWNER}-build"
  BUILD_SMOKE="oo-${OWNER}-smoke"
  DB_SRC="oo_${OWNER}_src"
  DB_DST="oo_${OWNER}_dst"
  ROLE_READER="oo_${OWNER}_reader"
  ROLE_WRITER="oo_${OWNER}_writer"
  ROLE_RUNTIME="oo_${OWNER}_runtime"
  ROLE_LOGIN="oo_${OWNER}_login"
  SECRET_REF="secret://pg/${OWNER}/reader"
  SHARD="${OWNER}main"
}

object_state() {
  local kind name project owner session fixed
  kind=$1
  name=$2
  case "$kind" in
    volume)
      if ! docker volume inspect "$name" >/dev/null 2>&1; then
        printf '%s\n' missing
        return 0
      fi
      project=$(docker volume inspect "$name" --format '{{index .Labels "oo.project"}}')
      owner=$(docker volume inspect "$name" --format '{{index .Labels "oo.owner"}}')
      session=$(docker volume inspect "$name" --format '{{index .Labels "oo.session"}}')
      fixed=$(docker volume inspect "$name" --format '{{index .Labels "oo.fixed"}}')
      ;;
    network)
      if ! docker network inspect "$name" >/dev/null 2>&1; then
        printf '%s\n' missing
        return 0
      fi
      project=$(docker network inspect "$name" --format '{{index .Labels "oo.project"}}')
      owner=$(docker network inspect "$name" --format '{{index .Labels "oo.owner"}}')
      session=$(docker network inspect "$name" --format '{{index .Labels "oo.session"}}')
      fixed=$(docker network inspect "$name" --format '{{index .Labels "oo.fixed"}}')
      ;;
    container)
      if ! docker container inspect "$name" >/dev/null 2>&1; then
        printf '%s\n' missing
        return 0
      fi
      project=$(docker inspect "$name" --format '{{index .Config.Labels "oo.project"}}')
      owner=$(docker inspect "$name" --format '{{index .Config.Labels "oo.owner"}}')
      session=$(docker inspect "$name" --format '{{index .Config.Labels "oo.session"}}')
      fixed=$(docker inspect "$name" --format '{{index .Config.Labels "oo.fixed"}}')
      ;;
    *)
      echo "unknown object kind" >&2
      return 2
      ;;
  esac
  if [ "$project" = "$PROJECT" ] && [ "$owner" = "$OWNER" ] && [ "$session" = "$RUN_ID" ] && [ "$fixed" = "$FIXED_SOURCE" ]; then
    printf '%s\n' ours
  else
    printf '%s\n' foreign
  fi
}

require_durable() {
  local durable have
  durable=$EVIDENCE/deployment.json
  if [ ! -f "$durable" ] || [ -L "$durable" ]; then
    echo "durable deployment manifest is absent" >&2
    return 2
  fi
  if ! printf '%s\n' "$DEPLOY_LINE" | cmp -s - "$durable"; then
    echo "durable deployment manifest does not match" >&2
    return 2
  fi
}

write_durable() {
  mkdir -p "$EVIDENCE"
  if [ -e "$EVIDENCE/deployment.json" ]; then
    require_durable
    return $?
  fi
  printf '%s\n' "$DEPLOY_LINE" > "$EVIDENCE/deployment.json"
  chmod 644 "$EVIDENCE/deployment.json"
}
