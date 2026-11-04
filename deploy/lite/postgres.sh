#!/bin/sh
set -eu
case "$OO_DB_IDENTITY" in agent-db|control-db) ;; *) exit 1;; esac
cp "/pki/lite-$OO_DB_IDENTITY-key" /tmp/server.key
cp "/pki/lite-$OO_DB_IDENTITY-cert" /tmp/server.crt
cp /pki/lite-ca-cert /tmp/ca.crt
cp /pki/lite-admin-password /tmp/admin-password
chown postgres:postgres /tmp/server.key /tmp/server.crt /tmp/ca.crt /tmp/admin-password
chmod 600 /tmp/server.key /tmp/admin-password
cat > /tmp/pg_hba.conf <<'HBA'
local all all trust
hostssl all all 0.0.0.0/0 scram-sha-256
hostssl all all ::/0 scram-sha-256
HBA
exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.crt \
  -c ssl_key_file=/tmp/server.key -c ssl_ca_file=/tmp/ca.crt -c hba_file=/tmp/pg_hba.conf
