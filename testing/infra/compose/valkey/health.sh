#!/bin/sh
# Fail when the client or the expected fsync mode is missing, and when the
# server process is still root.
set -eu
command -v valkey-cli >/dev/null || exit 1
command -v awk >/dev/null || exit 1
test -r /etc/oo/valkey.conf || exit 1
test -r /run/secrets/valkey_password || exit 1
expect=$(awk '$1=="appendfsync" { print $2; exit }' /etc/oo/valkey.conf)
[ -n "$expect" ] || exit 1
REDISCLI_AUTH=$(tr -d '\n' < /run/secrets/valkey_password)
export REDISCLI_AUTH
info=$(valkey-cli --no-auth-warning INFO persistence) || exit 1
printf '%s\n' "$info" | grep -q '^aof_enabled:1' || exit 1
info=$(valkey-cli --no-auth-warning INFO persistence) || exit 1
printf '%s\n' "$info" | grep -q '^aof_last_write_status:ok' || exit 1
cfg=$(valkey-cli --no-auth-warning CONFIG GET appendfsync) || exit 1
printf '%s\n' "$cfg" | grep -q "$expect" || exit 1
found=0
for cmd in /proc/[0-9]*/cmdline; do
  [ -r "$cmd" ] || continue
  text=$(tr '\0' ' ' < "$cmd" 2>/dev/null || true)
  case "$text" in
    *valkey-server*)
      pid=${cmd%/cmdline}
      pid=${pid##*/}
      uid=$(awk '/^Uid:/ { print $2; exit }' "/proc/$pid/status")
      [ "$uid" != "0" ]
      found=1
      ;;
  esac
done
[ "$found" = 1 ]
