#!/bin/sh
# TLS entry for the isolated oo-dl01w1-71 Postgres. Copies mounted certs onto a
# postgres-owned path, then starts the official entrypoint. Does not print secrets.
set -eu
install -d -o postgres -g postgres -m 750 /var/lib/postgresql/tls
install -o postgres -g postgres -m 644 /tls-in/server.crt /var/lib/postgresql/tls/server.crt
install -o postgres -g postgres -m 600 /tls-in/server.key /var/lib/postgresql/tls/server.key
install -o postgres -g postgres -m 644 /tls-in/ca.crt /var/lib/postgresql/tls/ca.crt
exec docker-entrypoint.sh postgres \
  -c shared_buffers=64MB \
  -c max_connections=20 \
  -c ssl=on \
  -c ssl_cert_file=/var/lib/postgresql/tls/server.crt \
  -c ssl_key_file=/var/lib/postgresql/tls/server.key \
  -c ssl_ca_file=/var/lib/postgresql/tls/ca.crt \
  -c password_encryption=scram-sha-256 \
  -c listen_addresses='*' \
  -c log_statement=none \
  -c log_connections=off \
  -c log_disconnections=off
