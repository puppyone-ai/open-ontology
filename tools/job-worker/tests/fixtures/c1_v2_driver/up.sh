#!/usr/bin/env bash
# Owned fixtures for the chunked-typed/v2 driver integration test (T9).
# Creates: oo-cb-T9-mysql57 (MySQL 5.7.44, TLS, ROW binlog, GTID) and oo-cb-T9-pg17 (PostgreSQL 17).
# Private files (0600 passwords, CA and server certs) live in $OO_CB_T9_HOME (default /tmp/oo-cb-T9/fixture).
# Nothing here touches another task's containers, networks or volumes.
set -euo pipefail

HOME_DIR="${OO_CB_T9_HOME:-/tmp/oo-cb-T9/fixture}"
MYSQL_NAME=oo-cb-T9-mysql57
PG_NAME=oo-cb-T9-pg17
MYSQL_PORT="${OO_CB_T9_MYSQL_PORT:-33957}"
PG_PORT="${OO_CB_T9_PG_PORT:-54917}"
MYSQL_IMAGE=mysql:5.7.44
PG_IMAGE=postgres:17
CAP=(--memory 16g --cpus 4)

mkdir -p "$HOME_DIR/secrets" "$HOME_DIR/certs"
chmod 700 "$HOME_DIR" "$HOME_DIR/secrets" "$HOME_DIR/certs"

secret() { # NAME: create once, 0600, never printed
  local f="$HOME_DIR/secrets/$1.pw"
  [[ -s "$f" ]] || { umask 077; openssl rand -hex 16 | tr -d "\n" > "$f"; }
  chmod 600 "$f"
}
for n in mysql-root cdc writer pg-super runtime; do secret "$n"; done

if [[ ! -s "$HOME_DIR/certs/ca.pem" ]]; then
  (
    cd "$HOME_DIR/certs"
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca-key.pem -out ca.pem -days 3 -subj "/CN=oo-cb-T9-ca" 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -keyout server-key.pem -out server.csr -subj "/CN=localhost" 2>/dev/null
    printf 'subjectAltName=DNS:localhost,DNS:%s,IP:127.0.0.1\n' "$MYSQL_NAME" > san.ext
    openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -out server-cert.pem -days 3 -extfile san.ext 2>/dev/null
    rm -f server.csr ca-key.pem.srl
    chmod 644 server-cert.pem server-key.pem ca.pem
    chmod 600 ca-key.pem
  )
fi

container_state() { # prints running|exited|...|absent
  local state
  state="$(docker inspect -f '{{.State.Status}}' "$1" 2>/dev/null || true)"
  state="${state//[[:space:]]/}"
  echo "${state:-absent}"
}

if [[ "$(container_state $MYSQL_NAME)" == absent ]]; then
  docker run -d --name "$MYSQL_NAME" --platform linux/amd64 --pull=never "${CAP[@]}" \
    -e MYSQL_ROOT_PASSWORD_FILE=/secrets/mysql-root.pw \
    -v "$HOME_DIR/secrets:/secrets:ro" -v "$HOME_DIR/certs:/certs:ro" \
    -p "127.0.0.1:$MYSQL_PORT:3306" "$MYSQL_IMAGE" \
    --server-id=5701 --log-bin=mysql-bin --binlog-format=ROW --binlog-row-image=FULL \
    --gtid-mode=ON --enforce-gtid-consistency=ON --expire-logs-days=7 \
    --ssl-ca=/certs/ca.pem --ssl-cert=/certs/server-cert.pem --ssl-key=/certs/server-key.pem \
    --character-set-server=utf8mb4 --collation-server=utf8mb4_bin --default-time-zone=+00:00 \
    --max-connections=100 >/dev/null
elif [[ "$(container_state $MYSQL_NAME)" != running ]]; then
  docker start "$MYSQL_NAME" >/dev/null
fi

if [[ "$(container_state $PG_NAME)" == absent ]]; then
  docker run -d --name "$PG_NAME" --platform linux/arm64 --pull=never "${CAP[@]}" \
    -e POSTGRES_PASSWORD_FILE=/secrets/pg-super.pw \
    -v "$HOME_DIR/secrets:/secrets:ro" \
    -p "127.0.0.1:$PG_PORT:5432" "$PG_IMAGE" >/dev/null
elif [[ "$(container_state $PG_NAME)" != running ]]; then
  docker start "$PG_NAME" >/dev/null
fi

for _ in $(seq 1 120); do
  docker exec "$MYSQL_NAME" sh -c 'MYSQL_PWD="$(cat /secrets/mysql-root.pw)" mysql --protocol=socket -uroot -N -e "SELECT 1" >/dev/null 2>&1' && break
  sleep 2
done
docker exec -i -e MYSQL_PWD="$(cat "$HOME_DIR/secrets/mysql-root.pw")" "$MYSQL_NAME" mysql --protocol=socket -uroot <<SQL
CREATE USER IF NOT EXISTS 'oo_cdc'@'%' IDENTIFIED BY '$(cat "$HOME_DIR/secrets/cdc.pw")';
ALTER USER 'oo_cdc'@'%' IDENTIFIED BY '$(cat "$HOME_DIR/secrets/cdc.pw")';
GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'oo_cdc'@'%';
GRANT SELECT ON oo_cb_t9.* TO 'oo_cdc'@'%';
CREATE USER IF NOT EXISTS 'oo_writer'@'%' IDENTIFIED BY '$(cat "$HOME_DIR/secrets/writer.pw")';
ALTER USER 'oo_writer'@'%' IDENTIFIED BY '$(cat "$HOME_DIR/secrets/writer.pw")';
GRANT SELECT, INSERT, UPDATE, DELETE ON oo_cb_t9.* TO 'oo_writer'@'%';
CREATE DATABASE IF NOT EXISTS oo_cb_t9 CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
SQL
echo "mysql: $MYSQL_NAME on 127.0.0.1:$MYSQL_PORT (TLS, CA at $HOME_DIR/certs/ca.pem)"

for _ in $(seq 1 60); do
  docker exec "$PG_NAME" pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 2
done
echo "postgres: $PG_NAME on 127.0.0.1:$PG_PORT"
