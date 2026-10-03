#!/bin/bash
# Remove only this UUID's containers, volumes, network, and /tmp directory.
# Archive the SQL log first. Do not touch foreign containers.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a121
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-cut-supplement
LIVE=${EVIDENCE}/live
SEC=/tmp/oo-a121-${RUN_ID}/secrets
ROOT=/tmp/oo-a121-${RUN_ID}
PG=${RUN_ID}-pg
VOL=${RUN_ID}-pglog
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target

mkdir -p "${LIVE}/cleanup"
if docker ps -a --filter "name=^/${PG}$" --format '{{.ID}}' | grep -q .; then
  docker cp "$PG":/pglog/postgresql.log "${LIVE}/cleanup/postgresql.log" 2>"${LIVE}/cleanup/log-copy.err" || true
fi
if [ -s "${LIVE}/cleanup/postgresql.log" ]; then
  hits=0
  for name in reader.pass writer.pass super.pass; do
    if [ -f "$SEC/$name" ] && grep -F -q -f "$SEC/$name" "${LIVE}/cleanup/postgresql.log"; then
      hits=$((hits + 1))
    fi
  done
  printf '%s\n' "$hits" > "${LIVE}/cleanup/log-secret-files.txt"
  if [ "$hits" -ne 0 ]; then
    awk -v dir="$SEC" '
      BEGIN {
        n = 0
        split("reader.pass writer.pass super.pass", names, " ")
        for (i in names) {
          path = dir "/" names[i]
          while ((getline line < path) > 0) {
            if (length(line) > 0) secrets[++n] = line
          }
          close(path)
        }
      }
      {
        for (i = 1; i <= n; i++) gsub(secrets[i], "[redacted]")
        print
      }
    ' "${LIVE}/cleanup/postgresql.log" > "${LIVE}/cleanup/postgresql.redacted.log"
    rm -f "${LIVE}/cleanup/postgresql.log"
    mv "${LIVE}/cleanup/postgresql.redacted.log" "${LIVE}/sql-log.txt"
  else
    mv "${LIVE}/cleanup/postgresql.log" "${LIVE}/sql-log.txt"
  fi
fi

for name in "${RUN_ID}-rust-smoke" "${RUN_ID}-rust-review" "${RUN_ID}-rust-review-2" "${RUN_ID}-rust-review-3" "${RUN_ID}-rust-review-4" "${RUN_ID}-rust-review-5" "${RUN_ID}-rust-review-6" "$PG"; do
  if docker ps -a --filter "name=^/${name}$" --format '{{.ID}}' | grep -q .; then
    docker rm -f "$name" > "${LIVE}/cleanup/rm-${name}.txt" 2>&1 || true
  fi
done
for vol in "$CARGO_VOL" "$TARGET_VOL" "$VOL"; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    docker volume rm "$vol" > "${LIVE}/cleanup/vol-${vol}.txt" 2>&1 || true
  fi
done
if docker network inspect "$NET" >/dev/null 2>&1; then
  docker network rm "$NET" > "${LIVE}/cleanup/net.txt" 2>&1 || true
fi
if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
fi
{
  echo "cleaned_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "uuid=${RUN_ID}"
  docker ps -a --filter "name=${RUN_ID}" --format '{{.Names}} {{.Status}}' || true
  docker volume ls --filter "name=${RUN_ID}" --format '{{.Name}}' || true
  docker network ls --filter "name=${RUN_ID}" --format '{{.Name}}' || true
  if [ -d "$ROOT" ]; then echo "tmp=present"; else echo "tmp=absent"; fi
} > "${LIVE}/cleanup/result.txt"
echo "cleanup_recorded"
