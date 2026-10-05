#!/bin/bash
# Remove only a228 resources for this session. Does not prune. Does not delete images.
set -u
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a228
OWNER=a228
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${SESSION}/SP07/reviewed-effective-review
OUT=${EVIDENCE}/CLEANUP.txt
TMP=/tmp/oo-a228-${SESSION}
CARGO_VOL=oo-a228-eef831aa-cargo
TARGET_VOL=oo-a228-eef831aa-target
NET=oo-a228-eef831aa-net

label_of() {
  docker inspect -f "{{index .Config.Labels \"$1\"}}" "$2" 2>/dev/null || true
}

{
  echo "cleanup_session=${SESSION}"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "=== containers before ==="
  docker ps -a --filter label=oo.owner=${OWNER} --filter label=oo.session=${SESSION} --format '{{.Names}} {{.Status}} {{.ID}}'
  echo "=== volume labels ==="
  for vol in "$CARGO_VOL" "$TARGET_VOL"; do
    if docker volume inspect "$vol" >/dev/null 2>&1; then
      owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol")
      session=$(docker volume inspect -f '{{index .Labels "oo.session"}}' "$vol")
      kind=$(docker volume inspect -f '{{index .Labels "oo.kind"}}' "$vol")
      echo "$vol owner=$owner session=$session kind=$kind"
      if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
        echo "refusing volume $vol" >&2
        exit 2
      fi
    else
      echo "$vol absent"
    fi
  done
  echo "=== network label ==="
  if docker network inspect "$NET" >/dev/null 2>&1; then
    owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' "$NET")
    session=$(docker network inspect -f '{{index .Labels "oo.session"}}' "$NET")
    echo "$NET owner=$owner session=$session"
    if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
      echo "refusing network $NET" >&2
      exit 2
    fi
  else
    echo "$NET absent"
  fi
} > "$OUT"

for name in oo-a228-smoke oo-a228-overlay oo-a228-hash; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(label_of oo.owner "$name")
    session=$(label_of oo.session "$name")
    if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
      echo "refusing container $name" >&2
      exit 2
    fi
    docker rm -f "$name" >> "$OUT"
  fi
done

{
  echo "=== rm volumes ==="
} >> "$OUT"
for vol in "$CARGO_VOL" "$TARGET_VOL"; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    docker volume rm "$vol" >> "$OUT"
  fi
done
{
  echo "=== rm network ==="
} >> "$OUT"
if docker network inspect "$NET" >/dev/null 2>&1; then
  docker network rm "$NET" >> "$OUT"
fi
{
  echo "=== rm private tmp ==="
  if [ -d "$TMP" ]; then
    rm -rf "$TMP"
    echo "tmp_removed"
  else
    echo "tmp_absent"
  fi
  echo "=== after ==="
  docker ps -a --filter label=oo.owner=${OWNER} --filter label=oo.session=${SESSION} --format '{{.Names}} {{.Status}}'
  docker volume ls --filter label=oo.owner=${OWNER} --filter label=oo.session=${SESSION} --format '{{.Name}}'
  docker network ls --filter label=oo.owner=${OWNER} --filter label=oo.session=${SESSION} --format '{{.Name}}'
  echo "cleanup_done"
} >> "$OUT"
