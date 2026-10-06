#!/bin/bash
# Record what --network none puts in a fresh netns on this host.
# No Pi process. No product build.
set -u
ls -1 /sys/class/net > /proof/net-ifaces.txt
cat /proc/net/route > /proof/proc-net-route.txt
cat /proc/net/ipv6_route > /proof/proc-net-ipv6-route.txt
cat /proc/net/dev > /proof/proc-net-dev.txt
{
  for entry in /sys/class/net/*; do
    name=$(basename "$entry")
    if [ -d "$entry" ]; then
      printf 'iface %s\n' "$name"
      if [ -f "$entry/address" ]; then
        printf 'address '
        cat "$entry/address"
      fi
      if [ -f "$entry/operstate" ]; then
        printf 'operstate '
        cat "$entry/operstate"
      fi
    else
      printf 'file %s\n' "$name"
    fi
  done
} > /proof/iface-details.txt
echo probe-ok
