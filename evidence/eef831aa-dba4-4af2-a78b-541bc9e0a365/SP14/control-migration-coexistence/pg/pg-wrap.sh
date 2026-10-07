#!/bin/bash
# Copy the server key onto a postgres-owned path, then start the official entrypoint.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/pg/pg-wrap.sh
# That file cites evidence/eef831aa-dba4-4af2-a78b-541bc9e0a275/SP11/session-store/pg/pg-wrap.sh
# sha256 d803bb0b0e284ea50bf1a21a9de5e141178027f06c19989f2f5c13fe004c2d00
# Change: none in the entrypoint flags. autovacuum stays off. No host port.
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
  -c autovacuum=off \
  -c track_counts=on
