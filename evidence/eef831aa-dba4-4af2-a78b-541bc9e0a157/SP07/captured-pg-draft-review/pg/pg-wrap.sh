#!/bin/bash
# Install the server key, then the official entrypoint. Statement logging stays off until init.
set -eu
install -d -o postgres -g postgres -m 750 /var/lib/postgresql/tls
install -o postgres -g postgres -m 644 /tls-in/server.crt /var/lib/postgresql/tls/server.crt
install -o postgres -g postgres -m 600 /tls-in/server.key /var/lib/postgresql/tls/server.key
install -o postgres -g postgres -m 644 /tls-in/ca.crt /var/lib/postgresql/tls/ca.crt
if [ -d /pglog ]; then
  chown postgres:postgres /pglog
  chmod 755 /pglog
fi
exec docker-entrypoint.sh postgres \
  -c shared_buffers=64MB \
  -c max_connections=40 \
  -c ssl=on \
  -c ssl_cert_file=/var/lib/postgresql/tls/server.crt \
  -c ssl_key_file=/var/lib/postgresql/tls/server.key \
  -c ssl_ca_file=/var/lib/postgresql/tls/ca.crt \
  -c logging_collector=on \
  -c log_destination=stderr \
  -c log_directory=/pglog \
  -c log_filename=postgresql.log \
  -c log_file_mode=0644 \
  -c "log_line_prefix=%m pid=%p app=%a user=%u db=%d " \
  -c timezone=UTC \
  -c datestyle=ISO,MDY \
  -c idle_in_transaction_session_timeout=0 \
  -c listen_addresses='*'
