#!/bin/bash
# Cap the image heap, then exec the image FE entrypoint. The default
# JAVA_OPTS_FOR_JDK_17 is -Xmx8192m -Xms8192m, which cannot start inside the
# 1536m cgroup. This does not replace start_fe.sh.
set -euo pipefail
conf=/opt/apache-doris/fe/conf/fe.conf
: "${FE_HEAP:?FE_HEAP is required}"
sed -i "s/-Xmx8192m/-Xmx${FE_HEAP}/g; s/-Xms8192m/-Xms${FE_HEAP}/g" "$conf"
grep -q -- "-Xmx${FE_HEAP}" "$conf"
grep -q -- "-Xms${FE_HEAP}" "$conf"
if grep -q -- '-Xmx8192m\|-Xms8192m' "$conf"; then
  echo "default 8192m FE heap is still present" >&2
  exit 1
fi
echo "fe heap override applied: ${FE_HEAP}"
exec bash /usr/local/bin/init_fe.sh
