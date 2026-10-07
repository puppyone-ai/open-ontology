#!/bin/bash
# Continue the live proof after the direct exec failed on GLIBC_2.39.
# Does not start a Rust builder, does not start a second PostgreSQL, does not pull.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a356
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-restore
HARNESS=${REPO}/testing/real-db/pg-backup-restore
ROOT=/tmp/oo-a356-${RUN_ID}
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGNAME=oo-a356-pg
BIN=/var/lib/postgresql/a356-work/pg-backup-restore
OPERATOR=/run-in/operator.json

mkdir -p "$EVIDENCE/live/phases" "$EVIDENCE/pg"
chmod 755 "$HARNESS/linux/private-glibc.sh" "$HARNESS/linux/live-phases.sh" "$HARNESS/linux/continue-live.sh"

ATT="$EVIDENCE/live/attempt-1-glibc"
mkdir -p "$ATT"
if [ -f "$EVIDENCE/live/phases/load-fixture.err" ] && [ ! -f "$ATT/load-fixture.err" ]; then
  mv "$EVIDENCE/live/phases/load-fixture.err" "$ATT/load-fixture.err"
  mv "$EVIDENCE/live/phases/load-fixture.out" "$ATT/load-fixture.out"
  mv "$EVIDENCE/live/phases/load-fixture.exit" "$ATT/load-fixture.exit"
fi
if [ ! -f "$ATT/DIAGNOSIS.txt" ]; then
  cat > "$ATT/DIAGNOSIS.txt" << 'EOF'
Attempt 1 stopped at phase load-fixture before any SQL.

Command: docker exec -u 0 oo-a356-pg /var/lib/postgresql/a356-work/pg-backup-restore load-fixture /run-in/operator.json

Exit: 1

stderr:
/var/lib/postgresql/a356-work/pg-backup-restore: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by /var/lib/postgresql/a356-work/pg-backup-restore)

The pinned Rust 1.94 image is Debian glibc 2.41. The pinned PostgreSQL 17.10 image has an older glibc. The kernel started the binary. The image dynamic linker rejected it. pg_dump, pg_restore, and psql were not invoked. No fixture SQL was applied. Dump and restore flags were not changed.

Attempt 2 runs the same binary through a private copy of the Rust image ld-linux-aarch64.so.1, libc.so.6, and libgcc_s.so.1. pg_dump, pg_restore, and psql stay the PostgreSQL image binaries at /usr/bin. Child processes still start from a cleared environment, so they do not inherit that library path.
EOF
fi

own=$(docker ps -q --filter label=oo.owner=a356 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a356 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s own_pg=%s host_cargo=%s\n' "$own" "$global_builders" "$own_pg" "$host_cargo" > "$EVIDENCE/live/continue-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/live/continue-live.exit"
  exit 3
fi
if [ "$own_pg" -ne 1 ]; then
  echo "expected exactly one owned postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/live/continue-live.exit"
  exit 3
fi
docker exec -u 0 "$PGNAME" test -x "$BIN"
docker exec -u 0 "$PGNAME" test -s "$OPERATOR"

# shellcheck source=private-glibc.sh
source "$HARNESS/linux/private-glibc.sh"
install_private_glibc
# shellcheck source=live-phases.sh
source "$HARNESS/linux/live-phases.sh"
printf '%s\n' 0 > "$EVIDENCE/live/continue-live.exit"
