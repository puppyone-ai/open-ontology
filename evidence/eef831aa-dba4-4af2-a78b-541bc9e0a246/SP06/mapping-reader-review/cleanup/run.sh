#!/bin/bash
# Remove only a246 resources for this session. No prune. No foreign containers.
# Origin: a241 cleanup/run.sh bf059534e142be7665ac6ee1dd70710c13198580.
# Changes: a246 names. Label checks use index .Config.Labels.
set -u
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a246
ROOT=/tmp/oo-a246-${RUN}
log() { printf '%s\n' "$1"; }

for name in oo-a246-smoke oo-a246-proof oo-a246-pg oo-a246-hash; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
    session=$(docker inspect -f '{{index .Config.Labels "oo.session"}}' "$name" 2>/dev/null || true)
    if [ "$owner" = "a246" ] && [ "$session" = "$RUN" ]; then
      docker rm -f "$name"
      log "removed container $name"
    else
      log "left container $name owner=${owner:-none} session=${session:-none}"
    fi
  else
    log "container $name already absent"
  fi
done

for volume in oo-a246-eef831aa-cargo oo-a246-eef831aa-target oo-a246-eef831aa-pgdata; do
  if docker volume inspect "$volume" >/dev/null 2>&1; then
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$volume")
    session=$(docker volume inspect -f '{{index .Labels "oo.session"}}' "$volume")
    if [ "$owner" = "a246" ] && [ "$session" = "$RUN" ]; then
      docker volume rm "$volume"
      log "removed volume $volume"
    else
      log "left volume $volume"
    fi
  else
    log "volume $volume already absent"
  fi
done

if docker network inspect oo-a246-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' oo-a246-eef831aa-net)
  session=$(docker network inspect -f '{{index .Labels "oo.session"}}' oo-a246-eef831aa-net)
  if [ "$owner" = "a246" ] && [ "$session" = "$RUN" ]; then
    attached=$(docker network inspect -f '{{len .Containers}}' oo-a246-eef831aa-net)
    if [ "$attached" = "0" ]; then
      docker network rm oo-a246-eef831aa-net
      log "removed network oo-a246-eef831aa-net"
    else
      log "left network oo-a246-eef831aa-net attached=${attached}"
    fi
  else
    log "left network oo-a246-eef831aa-net"
  fi
else
  log "network oo-a246-eef831aa-net already absent"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  log "removed $ROOT"
else
  log "$ROOT already absent"
fi

log "remaining a246 containers:"
docker ps -a --filter label=oo.owner=a246 --filter label=oo.session="$RUN" --format '{{.Names}} {{.Status}}' || true
log "cleanup_done"
