#!/usr/bin/env bash
# Host-side fault agent for tools/mysql-preview/tests/c1_replica_e2e.rs.
#
# The live tests run in a container, which cannot reach docker on the host. They
# ask this agent to run one replica action by writing a request file:
#   DIR/req-<id>   one action, for example "stop-sql" or "filter-ignore oo_c1.cb_hdr"
# The agent writes DIR/res-<id>: the exit status on the first line, then the output.
# Actions go only to the owned T12 replica container, through
# testing/chunked-baseline/faults/replica_ctl.sh. Anything else is refused.
#
# Usage: replica_fault_agent.sh --variant 57|84 --dir DIR
# Stop:  touch DIR/STOP
set -u
export LC_ALL=C

variant=""; dir=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --variant) variant="$2"; shift 2 ;;
    --dir) dir="$2"; shift 2 ;;
    *) echo "usage: $0 --variant 57|84 --dir DIR" >&2; exit 2 ;;
  esac
done
[[ "$variant" == 57 || "$variant" == 84 ]] || { echo "variant must be 57 or 84" >&2; exit 2; }
[[ -n "$dir" && -d "$dir" ]] || { echo "--dir must be an existing directory" >&2; exit 2; }

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ctl="$here/../../../testing/chunked-baseline/faults/replica_ctl.sh"
[[ -x "$ctl" ]] || { echo "missing $ctl" >&2; exit 2; }

allowed='^(status|stop-io|start-io|stop-sql|start-sql|restart|reset-identity|filter-clear|filter-ignore [A-Za-z0-9_]+\.[A-Za-z0-9_]+)$'
echo "fault agent: variant $variant, dir $dir" >&2
while [[ ! -e "$dir/STOP" ]]; do
  for req in "$dir"/req-*; do
    [[ -f "$req" ]] || continue
    case "$req" in *.tmp) continue ;; esac
    id="${req##*/req-}"
    action="$(cat "$req")"
    rm -f "$req"
    if [[ ! "$action" =~ $allowed ]]; then
      printf '2\nrefused: %s\n' "$action" > "$dir/res-$id.tmp"
    else
      # shellcheck disable=SC2086
      out="$("$ctl" --variant "$variant" $action 2>&1)"
      rc=$?
      printf '%s\n%s\n' "$rc" "$out" > "$dir/res-$id.tmp"
      echo "fault agent: $action -> $rc" >&2
    fi
    mv "$dir/res-$id.tmp" "$dir/res-$id"
  done
  sleep 0.2
done
echo "fault agent: stopped" >&2
