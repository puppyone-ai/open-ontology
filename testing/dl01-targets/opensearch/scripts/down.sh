#!/bin/bash
# Remove only this task's container, volume, and private secrets.
set -u
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/b53c75ff-62f4-4869-8428-a605d263cb43/DL01/opensearch/raw/down
mkdir -p "$EVIDENCE"
NAME=oo-dl01w1-15-opensearch
VOLUME=oo-dl01w1-15-data
PRIVATE=/tmp/oo-dl01w1-15-private
{
  echo "when=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if docker inspect "$NAME" >/dev/null 2>&1; then
    session=$(docker inspect -f '{{index .Config.Labels "session"}}' "$NAME" 2>/dev/null || true)
    echo "session=$session"
    if [ "$session" = "b53c75ff-62f4-4869-8428-a605d263cb43" ]; then
      docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} error={{.State.Error}} started={{.State.StartedAt}} finished={{.State.FinishedAt}}' "$NAME" > "$EVIDENCE/inspect.txt" || true
      docker logs "$NAME" > "$EVIDENCE/container.log" 2> "$EVIDENCE/container.err" || true
      docker stop "$NAME" > "$EVIDENCE/stop.stdout" 2> "$EVIDENCE/stop.stderr"
      echo "stop_exit=$?"
      docker rm "$NAME" > "$EVIDENCE/rm.stdout" 2> "$EVIDENCE/rm.stderr"
      echo "rm_exit=$?"
    else
      echo "refusing_foreign_container"
    fi
  else
    echo "container_absent"
  fi
  if docker volume inspect "$VOLUME" >/dev/null 2>&1; then
    docker volume rm "$VOLUME" > "$EVIDENCE/volume.stdout" 2> "$EVIDENCE/volume.stderr"
    echo "volume_rm_exit=$?"
  else
    echo "volume_absent"
  fi
  if [ -d "$PRIVATE" ]; then
    python3 - "$PRIVATE" "$EVIDENCE/container.log" <<'PY'
import sys
from pathlib import Path
private = Path(sys.argv[1])
log = Path(sys.argv[2])
ident = private / "identities.env"
users = private / "security" / "internal_users.yml"
secrets = []
if ident.exists():
    for line in ident.read_text(encoding="utf-8").splitlines():
        if "password=" in line:
            secrets.append(line.split("=", 1)[1])
if users.exists():
    for line in users.read_text(encoding="utf-8").splitlines():
        if "hash:" in line:
            secrets.append(line.split(":", 1)[1].strip().strip('"'))
paths = [log]
err = log.with_name("container.err")
if err.exists():
    paths.append(err)
for path in paths:
    if not path.exists():
        continue
    text = path.read_text(encoding="utf-8", errors="replace")
    for secret in secrets:
        if len(secret) >= 8:
            text = text.replace(secret, "[redacted]")
    path.write_text(text, encoding="utf-8")
PY
    rm -rf "$PRIVATE"
    echo "private_removed=1"
  else
    echo "private_absent"
  fi
  echo "remaining=$(docker ps -a --filter name=oo-dl01w1-15 --format '{{.Names}}' | tr '\n' ' ')"
  echo "volume_remaining=$(docker volume ls --format '{{.Name}}' | awk '$0=="oo-dl01w1-15-data"{print}')"
} > "$EVIDENCE/down.txt" 2>> "$EVIDENCE/container.err"
rc=0
if grep -E 'stop_exit=[^0]|rm_exit=[^0]|volume_rm_exit=[^0]|refusing_foreign_container' "$EVIDENCE/down.txt" >/dev/null; then
  rc=1
fi
printf '%s\n' "$rc" > "$EVIDENCE/down.exit"
echo down_recorded
