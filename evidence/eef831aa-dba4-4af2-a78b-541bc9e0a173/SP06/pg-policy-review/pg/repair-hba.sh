#!/bin/bash
# Repair the empty pg_hba left by up.sh. docker exec without -i dropped the heredoc.
# The server log says the empty file was not reloaded. This write uses -i.
set -euo pipefail
NAME=oo-a173-pg
PG=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a173/SP06/pg-policy-review/pg
mkdir -p "$PG/attempt1-hba"
cp "$PG/pg-hba-after.txt" "$PG/attempt1-hba/pg-hba-after-empty.txt"
printf '%s\n' 'psql default role postgres does not exist; empty pg_hba was not reloaded' > "$PG/attempt1-hba/note.txt"
docker exec -u postgres -i "$NAME" sh -c 'cat > /var/lib/postgresql/data/pg_hba.conf' <<'EOF'
local all all trust
hostssl all all all scram-sha-256
EOF
docker exec -u postgres "$NAME" pg_ctl -D /var/lib/postgresql/data reload > "$PG/reload-repair.txt"
docker exec -u postgres "$NAME" cat /var/lib/postgresql/data/pg_hba.conf > "$PG/pg-hba-after.txt"
docker exec -u postgres "$NAME" psql -U oo_a173_grantor -d postgres -Atc 'SHOW server_version' > "$PG/server-version.txt"
docker exec -u postgres "$NAME" psql -U oo_a173_grantor -d postgres -Atc 'SHOW ssl' > "$PG/ssl-on.txt"
docker logs "$NAME" 2>&1 | tail -n 20 > "$PG/log-tail.txt"
printf '%s\n' 0 > "$PG/repair.exit"
echo "hba_repaired"
