#!/bin/bash
# Product runner entry. Starts the loopback splice, then waits.
# The splice copies bytes and records a connection count. It does not log payloads.
set -u
mkdir -p /var/lib/oo/work /var/lib/oo/knowledge /var/lib/oo/copy /usr/local/bin /opt/oo/bin
ln -sfn /target/loop/debug/local-pg-mirror /opt/oo/bin/local-pg-mirror
ln -sfn /target/loop/debug/minimal-product-loop /opt/oo/bin/minimal-product-loop
if [ ! -f /usr/local/bin/oo-pipeline ]; then
  printf '%s\n' '#!/bin/sh' 'echo sealed-path-does-not-call-catalog-pg >&2' 'exit 1' > /usr/local/bin/oo-pipeline
  chmod 755 /usr/local/bin/oo-pipeline
fi
export SPLICE_LISTEN=127.0.0.1:18317
export SPLICE_UPSTREAM=${SPLICE_UPSTREAM:-host.docker.internal:8317}
export SPLICE_COUNTER=/var/lib/oo/work/splice-connects
if [ ! -x /target/loop/splice ]; then
  echo "splice binary is absent" >&2
  exit 1
fi
/target/loop/splice >> /var/lib/oo/work/splice.log 2>&1 &
echo $! > /var/lib/oo/work/splice.pid
exec sleep infinity
