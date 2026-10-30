"""Provision a non-login Gateway role, not business grants or a database superuser."""
import argparse
import json
from pathlib import Path
import re
import subprocess


def role_sql(role, reader):
    if (not isinstance(role, str) or not re.fullmatch(r'oo_r_[a-z][a-z0-9_]{0,57}', role)
            or not isinstance(reader, str) or not re.fullmatch(r'oo_[a-z][a-z0-9_]{0,59}', reader)
            or reader in {'oo_target_owner', 'oo_target_admin'} or reader == role):
        raise ValueError('invalid_role_binding')
    return f"""BEGIN;
SET LOCAL lock_timeout='5s';
SET LOCAL statement_timeout='15s';
SELECT pg_catalog.pg_advisory_xact_lock(730021, 23);
DO $body$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{reader}' AND rolcanlogin
      AND NOT (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)) THEN
    RAISE EXCEPTION 'unsafe_or_missing_reader';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{role}') THEN
    CREATE ROLE "{role}" NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE
      NOINHERIT NOREPLICATION NOBYPASSRLS;
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{role}' AND
      (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolinherit
       OR rolreplication OR rolbypassrls)) THEN
    RAISE EXCEPTION 'unsafe_existing_role';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.roleid
      JOIN pg_roles u ON u.oid=m.member
      WHERE (r.rolname='{role}' AND
        (u.rolname<>'{reader}' OR m.admin_option OR m.inherit_option OR NOT m.set_option))
        OR u.rolname='{role}') THEN
    RAISE EXCEPTION 'unexpected_role_membership';
  END IF;
  GRANT "{role}" TO "{reader}" WITH ADMIN FALSE, INHERIT FALSE, SET TRUE;
END
$body$;
COMMIT;
"""


def run(config):
    sql = role_sql(config['role'], config['reader'])
    command = config['psql_command']
    if not isinstance(command, list) or not command or any(not isinstance(a, str) for a in command):
        raise ValueError('invalid_psql_command')
    result = subprocess.run(command + ['-X', '-w', '-v', 'ON_ERROR_STOP=1'],
                            input=sql, capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise ValueError('role_provision_failed')
    return {'protocol': 'oo.library.reader-role/v1', 'role': config['role'],
            'reader': config['reader'], 'role_created_or_verified': True,
            'business_grants_added': False, 'published': False, 'database_superuser': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    args = parser.parse_args()
    print(json.dumps(run(json.loads(Path(args.config).read_text()))))
