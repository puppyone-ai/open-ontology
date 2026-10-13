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

# Image default sys_log_mode=ASYNC batches AuditLogger and can drop events
# when the internal column_statistics burst fills the async logger. NORMAL
# writes each audit event on the calling thread. The module list is the
# documented query set, not a narrower allowlist. The queue ceiling is the
# field added for discard-when-full; a larger ceiling does not hide rows.
drop_active() {
  key="$1"
  sed -i "/^${key}[[:space:]]*=/d" "$conf"
}
drop_active sys_log_mode
drop_active audit_log_modules
drop_active audit_event_log_queue_size
printf '\nsys_log_mode = NORMAL\naudit_log_modules = slow_query, query\naudit_event_log_queue_size = 1000000\n' >>"$conf"
grep -q '^sys_log_mode = NORMAL' "$conf"
grep -q '^audit_log_modules = slow_query, query' "$conf"
grep -q '^audit_event_log_queue_size = 1000000' "$conf"
echo "fe audit override applied: sys_log_mode=NORMAL modules=slow_query,query queue=1000000"
exec bash /usr/local/bin/init_fe.sh
