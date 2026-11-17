#!/bin/sh
# docs-db entrypoint (10 §2.7): TLS and SCRAM only, same pattern as
# deploy/lite/postgres.sh. The PKI directory is mounted read-only at /pki
# (docs-db-key, docs-db-cert, ca-cert, admin-password); Postgres needs the key
# owned by `postgres` with mode 0600, so it is copied to /tmp (tmpfs) first.
set -eu
cp /pki/docs-db-key /tmp/server.key
cp /pki/docs-db-cert /tmp/server.crt
cp /pki/ca-cert /tmp/ca.crt
cp /pki/admin-password /tmp/admin-password
chown postgres:postgres /tmp/server.key /tmp/server.crt /tmp/ca.crt /tmp/admin-password
chmod 600 /tmp/server.key /tmp/admin-password
cat > /tmp/pg_hba.conf <<'HBA'
local all all trust
hostssl all all 0.0.0.0/0 scram-sha-256
hostssl all all ::/0 scram-sha-256
HBA
exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.crt \
  -c ssl_key_file=/tmp/server.key -c ssl_ca_file=/tmp/ca.crt -c hba_file=/tmp/pg_hba.conf \
  -c password_encryption=scram-sha-256 -c listen_addresses='*'
