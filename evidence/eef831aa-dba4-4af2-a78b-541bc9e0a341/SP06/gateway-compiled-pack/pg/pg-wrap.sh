#!/bin/bash
# Copy the server key onto a postgres-owned path, then start the official entrypoint.
# Statement logging is off so a bound token cannot land in the server log.
# Origin: a326 pg/pg-wrap.sh, which records a314 and a304. No statement text was changed.
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
  -c log_min_duration_statement=-1 \
  -c log_parameter_max_length=0 \
  -c log_min_error_statement=panic \
  -c log_connections=off \
  -c log_disconnections=off \
  -c idle_in_transaction_session_timeout=0
