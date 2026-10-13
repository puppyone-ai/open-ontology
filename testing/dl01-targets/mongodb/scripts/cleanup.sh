#!/bin/sh
# Remove only oo-dl01w1-14 resources. Does not prune images or touch foreign containers.
set -u
# Capture the image's anonymous /data/configdb volume before the container is removed.
configdb="$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/data/configdb"}}{{.Name}}{{end}}{{end}}' oo-dl01w1-14-mongodb 2>/dev/null || true)"
# -v removes anonymous volumes of this container only. The named data volume is removed next.
docker rm -f -v oo-dl01w1-14-mongodb > /tmp/oo-dl01w1-14-cleanup-rm.txt 2>&1
rm_status=$?
docker volume rm oo-dl01w1-14-data > /tmp/oo-dl01w1-14-cleanup-volume.txt 2>&1
vol_status=$?
if [ -n "$configdb" ]; then
  docker volume rm "$configdb" > /tmp/oo-dl01w1-14-cleanup-configdb.txt 2>&1 || true
fi
docker network rm oo-dl01w1-14-net > /tmp/oo-dl01w1-14-cleanup-net.txt 2>&1
net_status=$?
echo "rm $rm_status"
echo "volume $vol_status"
echo "configdb ${configdb:-none}"
echo "network $net_status"
docker ps -a --filter name=oo-dl01w1-14 --format '{{.Names}} {{.Status}}'
exit 0
