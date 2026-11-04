#!/bin/bash
set -euo pipefail
# Only for the owned Docker VM fixture. Never replace a real NUMA topology.
if [ -e /sys/devices/system/node ]; then
    echo "Refusing uniform-memory fixture on a NUMA-aware kernel" >&2
    exit 1
fi
mount --make-rprivate /
mkdir -p /tmp/oo-real-system
mount --bind /sys/devices/system /tmp/oo-real-system
mount -t tmpfs tmpfs /sys/devices/system
for p in /tmp/oo-real-system/*; do ln -s "$p" /sys/devices/system/; done
mkdir -p /sys/devices/system/node/node0
n=$(getconf _NPROCESSORS_CONF)
if [ "$n" -gt 32 ] || [ "$n" -lt 1 ]; then
    echo "Uniform fixture supports 1-32 virtual CPUs" >&2
    exit 1
fi
printf '%x\n' "$(( (1 << n) - 1 ))" > /sys/devices/system/node/node0/cpumap
printf '0-%s\n' "$((n - 1))" > /sys/devices/system/node/node0/cpulist
printf '0\n' > /sys/devices/system/node/online
printf '0\n' > /sys/devices/system/node/possible
awk '/MemTotal:/ {print "Node 0 MemTotal: " $2 " kB"}' /proc/meminfo > /sys/devices/system/node/node0/meminfo
export LD_PRELOAD=/tmp/oo-uniform-vm.so
exec /usr/local/bin/entrypoint.sh gaussdb
