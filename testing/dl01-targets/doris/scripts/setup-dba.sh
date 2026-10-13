#!/bin/bash
# Synthetic fixture load through the DBA identity. The reader is not used here.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

umask 077
mkdir -p "$SECRETS" "$RUNDIR"
if [[ ! -s "$SECRETS/reader.password" ]]; then
  openssl rand -hex 16 >"$SECRETS/reader.password"
  openssl rand -hex 16 >"$SECRETS/dba.password"
  openssl rand -hex 16 >"$SECRETS/root.password"
  chmod 600 "$SECRETS"/*.password
fi
READER_PW="$(cat "$SECRETS/reader.password")"
DBA_PW="$(cat "$SECRETS/dba.password")"
ROOT_PW="$(cat "$SECRETS/root.password")"
write_cnf "$SECRETS/root-bootstrap.cnf" root ""
write_cnf "$SECRETS/root.cnf" root "$ROOT_PW"
write_cnf "$SECRETS/dba.cnf" oo_dba "$DBA_PW"
write_cnf "$SECRETS/reader.cnf" oo_reader "$READER_PW"
unset READER_PW DBA_PW ROOT_PW

phase() {
  local name="$1" cnf="$2" sql="$3"
  local n=1
  while [[ -e "$RAW/${name}-attempt-${n}" ]]; do
    n=$((n + 1))
  done
  local dir="$RAW/${name}-attempt-${n}"
  mkdir -p "$dir"
  printf '%s\n' "$sql" >"$dir/sql-path.txt"
  mysql_client "$cnf" "$sql" "$dir/stdout" "$dir/stderr" "$dir/exit" ""
  redact_evidence
  cp "$dir/exit" "$RAW/${name}.exit"
  cp "$dir/stdout" "$RAW/${name}.stdout"
  cp "$dir/stderr" "$RAW/${name}.stderr"
  cat "$dir/exit"
}

expect_zero() {
  local name="$1" ec
  ec="$(cat "$RAW/${name}.exit")"
  if [[ "$ec" != "0" ]]; then
    echo "phase $name failed exit $ec" >&2
    return 1
  fi
}

expect_nonzero() {
  local name="$1" ec
  ec="$(cat "$RAW/${name}.exit")"
  if [[ "$ec" == "0" ]]; then
    echo "phase $name unexpectedly succeeded" >&2
    return 1
  fi
}

echo "wait for fe query port"
printf 'SELECT 1;\n' >"$RUNDIR/ready.sql"
fe_ready=0
for _ in $(seq 1 90); do
  mysql_client "$SECRETS/root-bootstrap.cnf" "$RUNDIR/ready.sql" \
    "$RUNDIR/wait-stdout" "$RUNDIR/wait-stderr" "$RUNDIR/wait.exit" ""
  if [[ "$(cat "$RUNDIR/wait.exit")" == "0" ]]; then
    fe_ready=1
    break
  fi
  sleep 5
done
cp "$RUNDIR/wait-stdout" "$RAW/fe-ready.stdout" || true
cp "$RUNDIR/wait-stderr" "$RAW/fe-ready.stderr" || true
cp "$RUNDIR/wait.exit" "$RAW/fe-ready.exit" || true
if [[ "$fe_ready" != "1" ]]; then
  dump_runtime || true
  echo "fe did not accept a select" >&2
  exit 1
fi

printf 'ALTER SYSTEM ADD BACKEND "%s:%s";\n' "$BE_IP" "$BE_HEARTBEAT_PORT" >"$RUNDIR/add-backend.sql"
phase add-backend "$SECRETS/root-bootstrap.cnf" "$RUNDIR/add-backend.sql" >/dev/null || true

printf 'SHOW BACKENDS;\n' >"$RUNDIR/show-backends.sql"
be_ready=0
for _ in $(seq 1 90); do
  mysql_client "$SECRETS/root-bootstrap.cnf" "$RUNDIR/show-backends.sql" \
    "$RUNDIR/be-stdout" "$RUNDIR/be-stderr" "$RUNDIR/be.exit" ""
  if [[ "$(cat "$RUNDIR/be.exit")" == "0" ]] && grep -Eiq '(^|[[:space:]])true([[:space:]]|$)' "$RUNDIR/be-stdout"; then
    be_ready=1
    break
  fi
  sleep 5
done
cp "$RUNDIR/be-stdout" "$RAW/be-ready.stdout"
cp "$RUNDIR/be-stderr" "$RAW/be-ready.stderr"
printf '%s\n' "$be_ready" >"$RAW/be-ready.flag"
if [[ "$be_ready" != "1" ]]; then
  dump_runtime || true
  echo "be did not become alive" >&2
  exit 1
fi

phase schema "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/schema.sql" >/dev/null
expect_zero schema
phase load "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/load.sql" >/dev/null
expect_zero load
phase uniq-mutate "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/uniq-mutate.sql" >/dev/null
expect_zero uniq-mutate

phase negative-varbinary "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/negative-varbinary.sql" >/dev/null
expect_nonzero negative-varbinary
phase negative-partition "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/negative-partition.sql" >/dev/null
expect_nonzero negative-partition
phase negative-streams "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/negative-streams.sql" >/dev/null
expect_nonzero negative-streams
phase nul-byte "$SECRETS/root-bootstrap.cnf" "$ROOT/fixtures/nul-byte.sql" >/dev/null || true

python3 - "$SECRETS" "$RUNDIR/create-users.sql" <<'PY'
import pathlib, sys
secrets = pathlib.Path(sys.argv[1])
reader = (secrets / "reader.password").read_text().strip()
dba = (secrets / "dba.password").read_text().strip()
root = (secrets / "root.password").read_text().strip()
path = pathlib.Path(sys.argv[2])
path.write_text(
    "CREATE USER 'oo_reader' IDENTIFIED BY '{reader}';\n"
    "GRANT SELECT_PRIV ON internal.oo_dl01w1_12.* TO 'oo_reader';\n"
    "CREATE USER 'oo_dba' IDENTIFIED BY '{dba}';\n"
    "GRANT ADMIN_PRIV ON *.*.* TO 'oo_dba';\n"
    "ALTER USER 'root' IDENTIFIED BY '{root}';\n".format(reader=reader, dba=dba, root=root)
)
path.chmod(0o600)
PY
phase create-users "$SECRETS/root-bootstrap.cnf" "$RUNDIR/create-users.sql" >/dev/null
expect_zero create-users

# Confirm the reader can select and the old empty root password no longer works.
phase reader-ping "$SECRETS/reader.cnf" "$RUNDIR/ready.sql" >/dev/null
expect_zero reader-ping
phase root-empty-rejected "$SECRETS/root-bootstrap.cnf" "$RUNDIR/ready.sql" >/dev/null
expect_nonzero root-empty-rejected
echo "setup complete"
