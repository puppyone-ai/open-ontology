#!/bin/sh
set -eu
umask 077
cp /pki/server.key /tmp/server.key
cp /pki/server.crt /tmp/server.crt
cp /pki/ca.crt /tmp/ca.crt
cp /pki/admin-password /tmp/admin-password
chown postgres:postgres /tmp/server.key /tmp/server.crt /tmp/ca.crt /tmp/admin-password
chmod 600 /tmp/server.key /tmp/admin-password
cat > /tmp/pg_hba.conf <<'HBA'
local all all trust
hostssl all all 0.0.0.0/0 scram-sha-256
hostssl all all ::/0 scram-sha-256
HBA
chown postgres:postgres /tmp/pg_hba.conf
exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.crt \
  -c ssl_key_file=/tmp/server.key -c ssl_ca_file=/tmp/ca.crt -c hba_file=/tmp/pg_hba.conf
