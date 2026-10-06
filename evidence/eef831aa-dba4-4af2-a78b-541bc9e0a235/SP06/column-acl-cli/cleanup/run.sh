#!/bin/bash
# Remove only a235 resources for this session. No prune. No foreign containers.
set -u
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a235
ROOT=/tmp/oo-a235-${RUN}
log() { printf '%s\n' "$1"; }

for name in oo-a235-smoke oo-a235-proof oo-a235-pg oo-a235-hash; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
    session=$(docker inspect -f '{{index .Config.Labels "oo.session"}}' "$name" 2>/dev/null || true)
    if [ "$owner" = "a235" ] && [ "$session" = "$RUN" ]; then
      docker rm -f "$name"
      log "removed container $name"
    else
      log "left container $name owner=${owner:-none} session=${session:-none}"
    fi
  else
    log "container $name already absent"
  fi
done

for volume in oo-a235-eef831aa-cargo oo-a235-eef831aa-target oo-a235-eef831aa-pgdata; do
  if docker volume inspect "$volume" >/dev/null 2>&1; then
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$volume")
    session=$(docker volume inspect -f '{{index .Labels "oo.session"}}' "$volume")
    if [ "$owner" = "a235" ] && [ "$session" = "$RUN" ]; then
      docker volume rm "$volume"
      log "removed volume $volume"
    else
      log "left volume $volume"
    fi
  else
    log "volume $volume already absent"
  fi
done

if docker network inspect oo-a235-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' oo-a235-eef831aa-net)
  session=$(docker network inspect -f '{{index .Labels "oo.session"}}' oo-a235-eef831aa-net)
  if [ "$owner" = "a235" ] && [ "$session" = "$RUN" ]; then
    docker network rm oo-a235-eef831aa-net
    log "removed network oo-a235-eef831aa-net"
  else
    log "left network oo-a235-eef831aa-net"
  fi
else
  log "network oo-a235-eef831aa-net already absent"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  log "removed $ROOT"
else
  log "$ROOT already absent"
fi

log "remaining a235 containers:"
docker ps -a --filter label=oo.owner=a235 --filter label=oo.session="$RUN" --format '{{.Names}} {{.Status}}' || true
log "cleanup_done"
