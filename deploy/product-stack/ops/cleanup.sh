#!/bin/bash
# Remove only a395 resources. Do not prune. Do not touch foreign names.
set -u
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
foreign() {
  local out="$1"
  : > "$out"
  local name owner
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
    if [ "$owner" != a395 ]; then
      printf '%s\n' "$name" >> "$out"
    fi
  done < <(docker ps -a --format '{{.Names}}')
  sort -o "$out" "$out"
}
foreign "${EVIDENCE}/run/foreign-before.txt"
docker ps -aq --filter label=oo.owner=a395 | while read -r id; do
  [ -n "$id" ] || continue
  docker rm -f "$id" >/dev/null 2>&1 || true
done
docker volume ls -q --filter label=oo.owner=a395 | while read -r volume; do
  [ -n "$volume" ] || continue
  docker volume rm "$volume" >/dev/null 2>&1 || true
done
docker network ls -q --filter label=oo.owner=a395 | while read -r net; do
  [ -n "$net" ] || continue
  docker network rm "$net" >/dev/null 2>&1 || true
done
if [ -d "$STATE" ]; then
  rm -rf "$STATE"
fi
foreign "${EVIDENCE}/run/foreign-after.txt"
{
  docker ps -a --filter label=oo.owner=a395 --format '{{.Names}}'
  docker volume ls --filter label=oo.owner=a395 --format '{{.Name}}'
  docker network ls --filter label=oo.owner=a395 --format '{{.Name}}'
} | sed '/^$/d' | sort > "${EVIDENCE}/run/owned-after.txt"
{
  echo "containers=$(docker ps -aq --filter label=oo.owner=a395 | sed '/^$/d' | wc -l | tr -d ' ')"
  echo "volumes=$(docker volume ls -q --filter label=oo.owner=a395 | sed '/^$/d' | wc -l | tr -d ' ')"
  echo "networks=$(docker network ls -q --filter label=oo.owner=a395 | sed '/^$/d' | wc -l | tr -d ' ')"
  if [ -d "$STATE" ]; then echo state=present; else echo state=absent; fi
} > "${EVIDENCE}/run/cleanup.txt"
if cmp -s "${EVIDENCE}/run/foreign-before.txt" "${EVIDENCE}/run/foreign-after.txt"; then
  echo "foreign_unchanged" >> "${EVIDENCE}/run/cleanup.txt"
else
  echo "foreign_list_changed" >> "${EVIDENCE}/run/cleanup.txt"
fi
printf '%s\n' 0 > "${EVIDENCE}/run/cleanup.exit"
echo "cleanup_ok"
