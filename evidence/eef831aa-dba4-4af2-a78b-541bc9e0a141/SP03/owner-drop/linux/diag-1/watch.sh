#!/bin/bash
# Sample reader backends while diag-1 reproduces the blocked Drop. No cargo.
set -u
OUT=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a141/SP03/owner-drop/linux/diag-1/stacks.txt
: > "$OUT"
for _ in $(seq 1 40); do
  date -u +'--- %H:%M:%S ---' >> "$OUT"
  docker exec -u root oo-a141-pg bash -lc 'ps -eo pid,stat,wchan:24,args | grep "[p]ostgres: oo_a141_reader"' >> "$OUT" 2>&1
  docker exec -u postgres oo-a141-pg bash -lc 'export PGPASSWORD=$(cat /secrets/super.pass); psql -U postgres -d oo_a141 -Atc "SELECT pid::text, COALESCE(state,'\'''\''), COALESCE(wait_event_type,'\'''\''), COALESCE(wait_event,'\'''\''), (xact_start IS NOT NULL)::text, COALESCE(query_start::text,'\'''\''), COALESCE(state_change::text,'\'''\''), left(regexp_replace(query, E'\''[\\n\\r]+'\'', '\'' '\'', '\''g'\''), 60) FROM pg_stat_activity WHERE usename::text = '\''oo_a141_reader'\'' ORDER BY pid"' >> "$OUT" 2>&1
  docker exec -u root oo-a141-pg bash -lc 'for d in /proc/[0-9]*; do cmd=$(tr "\0" " " < "$d/cmdline" 2>/dev/null || true); case "$cmd" in *"oo_a141_reader"*) echo "STACK ${d##*/}"; cat "$d/stack" 2>/dev/null || echo "no-stack"; echo "WCHAN $(cat "$d/wchan" 2>/dev/null)"; esac; done' >> "$OUT" 2>&1
  sleep 0.25
done
echo DONE >> "$OUT"
