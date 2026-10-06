#!/bin/bash
# Copy the server key onto a postgres-owned path, then start the official entrypoint.
# autovacuum is off so reltuples stays -1 until analyze-pg.
# Origin: a274 pg/pg-wrap.sh, which records a254 pg/pg-wrap.sh
# sha256 25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f.
# Server flags are unchanged.
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
