#!/bin/bash
# Create this task's MariaDB volume and container. Called under the resource lock.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

if ! docker network inspect "$OO_NETWORK" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=DL01-W1-39 \
    --label oo.session="$OO_SESSION" \
    --label oo.kind=network \
    --label oo.prefix="$OO_PREFIX" \
    "$OO_NETWORK"
fi
owner=$(docker network inspect "$OO_NETWORK" --format '{{index .Labels "oo.prefix"}}')
if [ "$owner" != "$OO_PREFIX" ]; then
  echo "refusing network $OO_NETWORK owned by $owner" >&2
  exit 2
fi
if docker volume inspect "$OO_VOLUME" >/dev/null 2>&1; then
  echo "refusing existing volume $OO_VOLUME" >&2
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=DL01-W1-39 \
  --label oo.session="$OO_SESSION" \
  --label oo.kind=mariadb-data \
  --label oo.prefix="$OO_PREFIX" \
  "$OO_VOLUME" >/dev/null

exec docker run -d \
  --pull never \
  --platform linux/arm64 \
  --name "$OO_CONTAINER" \
  --network "$OO_NETWORK" \
  --memory "$OO_MEMORY" \
  --memory-swap "$OO_MEMORY" \
  --cpus 1 \
  --pids-limit 256 \
  --security-opt no-new-privileges:true \
  --restart no \
  --publish "127.0.0.1:${OO_PORT}:3306" \
  --mount "type=volume,src=${OO_VOLUME},dst=/var/lib/mysql" \
  --mount "type=bind,src=${OO_SECRET_DIR},dst=/run/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=DL01-W1-39 \
  --label oo.session="$OO_SESSION" \
  --label oo.kind=mariadb-fixture \
  --label oo.task="$OO_TASK" \
  --label oo.prefix="$OO_PREFIX" \
  --env MARIADB_ROOT_PASSWORD_FILE=/run/secrets/root.pw \
  --health-cmd 'mariadb --defaults-extra-file=/run/secrets/root.cnf --protocol=socket --socket=/run/mysqld/mysqld.sock --silent --skip-column-names -e "SELECT 1"' \
  --health-interval 5s \
  --health-timeout 5s \
  --health-retries 30 \
  --health-start-period 40s \
  "$OO_IMAGE_TAG" \
  --skip-log-bin \
  --default-time-zone=+00:00 \
  --skip-name-resolve \
  --innodb-buffer-pool-size=128M \
  --sql-mode=STRICT_TRANS_TABLES,ERROR_FOR_DIVISION_BY_ZERO,NO_AUTO_CREATE_USER,NO_ENGINE_SUBSTITUTION,NO_ZERO_DATE,NO_ZERO_IN_DATE
