#!/bin/bash
# Copy the server key onto a postgres-owned path, then start the official entrypoint.
# Origin: deploy/trusted-demo/pg/pg-wrap.sh
# origin_commit 10b78993641c88154c75c22219c79c1566e2cecd
# origin_sha256 05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a
# Deviation: max_connections=128 so 100 runtime sessions fit. The reviewed file uses 20.
# Other server flags are unchanged.
set -eu
install -d -o postgres -g postgres -m 750 /var/lib/postgresql/tls
install -o postgres -g postgres -m 644 /tls-in/server.crt /var/lib/postgresql/tls/server.crt
install -o postgres -g postgres -m 600 /tls-in/server.key /var/lib/postgresql/tls/server.key
install -o postgres -g postgres -m 644 /tls-in/ca.crt /var/lib/postgresql/tls/ca.crt
exec docker-entrypoint.sh postgres \
  -c shared_buffers=64MB \
  -c max_connections=128 \
  -c ssl=on \
  -c ssl_cert_file=/var/lib/postgresql/tls/server.crt \
  -c ssl_key_file=/var/lib/postgresql/tls/server.key \
  -c ssl_ca_file=/var/lib/postgresql/tls/ca.crt \
  -c password_encryption=scram-sha-256 \
  -c listen_addresses='*' \
  -c autovacuum=off \
  -c track_counts=on
