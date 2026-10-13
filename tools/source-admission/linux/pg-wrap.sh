#!/bin/bash
# DBA-owned PostgreSQL entrypoint. Copy TLS material onto a postgres-owned
# path, then start the official image entrypoint. The reader role is created
# later by dba-setup.sh, not by this process.
set -eu
install -d -o postgres -g postgres -m 750 /var/lib/postgresql/tls
install -o postgres -g postgres -m 644 /tls-in/server.crt /var/lib/postgresql/tls/server.crt
install -o postgres -g postgres -m 600 /tls-in/server.key /var/lib/postgresql/tls/server.key
install -o postgres -g postgres -m 644 /tls-in/ca.crt /var/lib/postgresql/tls/ca.crt
if [ -d /var/log/pg ]; then
  chown postgres:postgres /var/log/pg
  chmod 750 /var/log/pg
else
  install -d -o postgres -g postgres -m 750 /var/log/pg
fi
# log_statement stays at its default (none) until ALTER SYSTEM. A -c value
# overrides postgresql.auto.conf, so reload cannot turn statement logging on.
exec docker-entrypoint.sh postgres \
  -c shared_buffers=64MB \
  -c max_connections=20 \
  -c ssl=on \
  -c ssl_cert_file=/var/lib/postgresql/tls/server.crt \
  -c ssl_key_file=/var/lib/postgresql/tls/server.key \
  -c ssl_ca_file=/var/lib/postgresql/tls/ca.crt \
  -c password_encryption=scram-sha-256 \
  -c listen_addresses='*' \
  -c logging_collector=on \
  -c log_destination=csvlog \
  -c log_directory=/var/log/pg \
  -c "log_filename=postgresql-%H%M%S" \
  -c log_rotation_age=0 \
  -c log_rotation_size=0 \
  -c log_connections=off \
  -c log_disconnections=off
