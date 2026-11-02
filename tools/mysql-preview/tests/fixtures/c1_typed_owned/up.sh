#!/bin/bash
# New owned fixtures only. Never starts/reconfigures an existing container.
set -euo pipefail
home="${OO_C1_TYPED_HOME:-/tmp/oo-mysql-c1-20261027}"
mysql=oo-mysql-c1-20261027-mysql57
pg=oo-mysql-c1-20261027-pg17
for name in "$mysql" "$pg"; do
  if docker inspect "$name" >/dev/null 2>&1; then echo "refused: resource already exists ($name)" >&2; exit 2; fi
done
python3 - <<'PY'
import socket
for port in [23327,25487]:
    with socket.socket() as sock: sock.bind(('127.0.0.1',port))
PY
if [[ -e "$home" ]]; then echo 'refused: fixture directory already exists' >&2; exit 2; fi
umask 077
mkdir -p "$home/secrets" "$home/certs"
for name in mysql-root cdc pg-super; do openssl rand -hex 24 | tr -d '\n' > "$home/secrets/$name.pw"; done
(
  cd "$home/certs"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout ca-key.pem -out ca.pem -days 7 -subj '/CN=owned-c1-ca' 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout server-key.pem -out server.csr -subj '/CN=localhost' 2>/dev/null
  printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > san.ext
  openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -out server-cert.pem -days 7 -extfile san.ext 2>/dev/null
  chmod 644 ca.pem server-cert.pem server-key.pem
)
docker run -d --pull=never --platform linux/amd64 --name "$mysql" --label owner=mysql-c1-20261027 \
  --memory 1g --cpus 2 -p 127.0.0.1:23327:3306 \
  -e MYSQL_ROOT_PASSWORD_FILE=/secrets/mysql-root.pw \
  -v "$home/secrets:/secrets:ro" -v "$home/certs:/certs:ro" mysql:5.7.44 \
  --server-id=5727 --log-bin=mysql-bin --binlog-format=ROW --binlog-row-image=FULL \
  --gtid-mode=ON --enforce-gtid-consistency=ON --expire-logs-days=7 \
  --ssl-ca=/certs/ca.pem --ssl-cert=/certs/server-cert.pem --ssl-key=/certs/server-key.pem \
  --general-log=ON --log-output=TABLE --default-time-zone=+00:00 >/dev/null
docker run -d --pull=never --name "$pg" --label owner=mysql-c1-20261027 --memory 1g --cpus 2 \
  -p 127.0.0.1:25487:5432 -e POSTGRES_PASSWORD_FILE=/secrets/pg-super.pw \
  -v "$home/secrets:/secrets:ro" postgres:17 >/dev/null
ready=false
for _ in $(seq 1 90); do
  if [[ "$(docker exec "$mysql" sh -c 'MYSQL_PWD="$(cat /secrets/mysql-root.pw)" mysql -uroot -N -e "SELECT @@skip_networking"' 2>/dev/null || true)" == 0 ]]; then ready=true; break; fi
  sleep 1
done
$ready || { echo 'mysql not ready' >&2; exit 1; }
# Fixture administration is separate from the product reader principal.
{
  printf "CREATE DATABASE c1_typed CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;\n"
  printf "CREATE USER 'oo_cdc'@'%%' IDENTIFIED BY '%s';\n" "$(cat "$home/secrets/cdc.pw")"
  printf "GRANT SELECT ON c1_typed.* TO 'oo_cdc'@'%%';\n"
  printf "GRANT REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO 'oo_cdc'@'%%';\n"
} | docker exec -i "$mysql" sh -c 'MYSQL_PWD="$(cat /secrets/mysql-root.pw)" mysql -uroot' >"$home/admin-output.txt" 2>&1
ready=false
for _ in $(seq 1 30); do
  if [[ "$(docker exec "$pg" psql -U postgres -Atc 'SHOW listen_addresses' 2>/dev/null || true)" == '*' ]]; then ready=true; break; fi
  sleep 1
done
$ready || { echo 'postgres not ready' >&2; exit 1; }
printf 'owned MySQL 5.7 on 23327; owned PostgreSQL on 25487; private handles: %s\n' "$home"
