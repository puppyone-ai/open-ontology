#!/bin/bash
# Cancel one reader statement from the DBA session. The reader holds the only
# business connection. The DBA connection is the operator, not a second reader.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DIR="$RAW/cancel"
mkdir -p "$DIR"
NAME="oo-dl01w1-12-cancel-client"
cleanup_client() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup_client EXIT
docker rm -f "$NAME" >"$DIR/pre-rm.stdout" 2>"$DIR/pre-rm.stderr" || true

printf 'SHOW PROCESSLIST;\n' >"$RUNDIR/processlist.sql"
start_epoch="$(date +%s)"
set +e
docker run -i --name "$NAME" \
  --platform "$CLIENT_PLATFORM" \
  --network "$NETWORK" \
  --memory "$CLIENT_MEMORY" \
  "${labels[@]}" \
  --label "oo.kind=doris-protocol-client" \
  --entrypoint mysql \
  -v "$SECRETS/reader.cnf:/client.cnf:ro" \
  "$CLIENT_REF" \
  --defaults-extra-file=/client.cnf \
  --batch \
  --raw \
  --default-character-set=utf8mb4 \
  --connect-timeout=15 \
  "$DB_NAME" <"$ROOT/reader/queries/sleep.sql" >"$DIR/client.stdout" 2>"$DIR/client.stderr" &
client_pid=$!
set -e

found=0
for _ in $(seq 1 30); do
  if ! kill -0 "$client_pid" 2>/dev/null; then
    break
  fi
  mysql_client "$SECRETS/dba.cnf" "$RUNDIR/processlist.sql" \
    "$DIR/processlist-during.stdout" "$DIR/processlist-during.stderr" \
    "$DIR/processlist-during.exit"
  if grep -Eiq 'sleep' "$DIR/processlist-during.stdout"; then
    found=1
    break
  fi
  sleep 1
done
printf '%s\n' "$found" >"$DIR/seen.flag"
if [[ "$found" != "1" ]]; then
  wait "$client_pid" || true
  echo $? >"$DIR/client.exit"
  echo "sleep statement was not visible; cancel not claimed" >"$DIR/note.txt"
  docker rm -f "$NAME" >"$DIR/post-rm.stdout" 2>"$DIR/post-rm.stderr" || true
  redact_evidence
  exit 1
fi

python3 - "$DIR/processlist-during.stdout" "$RUNDIR/kill.sql" <<'PY'
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
header = None
target = None
for line in text.splitlines():
    parts = line.split("\t")
    if header is None:
        folded = [item.lower() for item in parts]
        if "user" in folded and ("id" in folded or "queryid" in folded or "query id" in folded):
            header = folded
        continue
    if len(parts) != len(header):
        continue
    row = dict(zip(header, parts))
    blob = " ".join(parts).lower()
    user = row.get("user", "")
    if "sleep" in blob and user in {"oo_reader", "root"}:
        target = row
        break
if target is None:
    raise SystemExit("no sleep row to kill")
query_id = target.get("queryid") or target.get("query id")
conn_id = target.get("id")
path = pathlib.Path(sys.argv[2])
if query_id and __import__("re").fullmatch(r"[A-Za-z0-9_-]+", query_id):
    path.write_text('KILL QUERY "' + query_id + '";\n')
elif conn_id and conn_id.isdigit():
    path.write_text("KILL " + conn_id + ";\n")
else:
    raise SystemExit("sleep row has no usable id")
PY

mysql_client "$SECRETS/dba.cnf" "$RUNDIR/kill.sql" \
  "$DIR/kill.stdout" "$DIR/kill.stderr" "$DIR/kill.exit"
set +e
wait "$client_pid"
echo $? >"$DIR/client.exit"
set -e
end_epoch="$(date +%s)"
elapsed="$((end_epoch - start_epoch))"
printf '%s\n' "$elapsed" >"$DIR/elapsed.seconds"

mysql_client "$SECRETS/dba.cnf" "$RUNDIR/processlist.sql" \
  "$DIR/processlist-after.stdout" "$DIR/processlist-after.stderr" \
  "$DIR/processlist-after.exit"
if grep -Eiq 'sleep' "$DIR/processlist-after.stdout"; then
  printf 'still-visible\n' >"$DIR/after.flag"
else
  printf 'gone\n' >"$DIR/after.flag"
fi
docker rm -f "$NAME" >"$DIR/post-rm.stdout" 2>"$DIR/post-rm.stderr" || true
redact_evidence

kill_ec="$(cat "$DIR/kill.exit")"
after="$(cat "$DIR/after.flag")"
client_ec="$(cat "$DIR/client.exit")"
if [[ "$kill_ec" == "0" && "$after" == "gone" && "$client_ec" != "0" && "$elapsed" -lt 25 ]]; then
  printf 'pass\n' >"$DIR/result.txt"
  echo "cancel pass elapsed=${elapsed}s"
else
  printf 'fail\n' >"$DIR/result.txt"
  echo "cancel fail kill=${kill_ec} after=${after} client=${client_ec} elapsed=${elapsed}s" >&2
  exit 1
fi
