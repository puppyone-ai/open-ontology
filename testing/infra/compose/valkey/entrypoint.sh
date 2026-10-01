#!/bin/sh
# Apply the mounted AOF policy and a generated requirepass. Do not print the file.
# Refuse to exec the server as root.
set -eu
command -v setpriv >/dev/null 2>&1 || { echo "setpriv missing; refusing root" >&2; exit 1; }
id valkey >/dev/null 2>&1 || { echo "valkey user missing; refusing root" >&2; exit 1; }
command -v valkey-server >/dev/null 2>&1 || { echo "valkey-server missing" >&2; exit 1; }
pass=$(tr -d '\n' < /run/secrets/valkey_password)
printf '%s' "$pass" | grep -Eq '^[0-9a-f]{48}$' || { echo "valkey password is not 48 hex" >&2; exit 1; }
umask 077
mkdir -p /data /tmp
cat /etc/oo/valkey.conf > /tmp/valkey.conf
printf '\nrequirepass %s\n' "$pass" >> /tmp/valkey.conf
unset pass
chown valkey:valkey /tmp/valkey.conf
chown valkey:valkey /data || true
echo "valkey_runtime_user=valkey valkey_runtime_via=setpriv"
exec setpriv --reuid=valkey --regid=valkey --init-groups valkey-server /tmp/valkey.conf
