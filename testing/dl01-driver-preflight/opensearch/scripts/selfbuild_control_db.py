#!/usr/bin/env python3
"""Create an isolated OpenSearch Selfbuild control database through fixture DBA."""
import os
from pathlib import Path
import secrets
import subprocess

PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
SECRET = PRIVATE / 'selfbuild-control.pw'
CONTAINER = 'oo-dl01w1-44-native-fresh-pg'
ROLE = 'oo_dl01w1_44_selfbuild_admin'
DB = 'oo_dl01w1_44_selfbuild_control'


def psql(database, statement):
    result = subprocess.run(
        ['docker', 'exec', '-i', CONTAINER, 'psql', '-X', '-q', '-U', 'postgres',
         '-d', database, '-v', 'ON_ERROR_STOP=1'],
        input=statement, text=True, capture_output=True,
    )
    if result.returncode:
        raise RuntimeError('psql bootstrap failed: ' + result.stderr.strip())


def main():
    if SECRET.exists():
        raise SystemExit('control credential already exists; refusing replacement')
    password = secrets.token_hex(32)
    fd = os.open(SECRET, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as stream:
        stream.write(password + '\n')
    psql('postgres', "CREATE ROLE " + ROLE + " LOGIN NOSUPERUSER NOCREATEDB "
         "NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '" + password + "';\n")
    psql('postgres', 'CREATE DATABASE ' + DB + ' OWNER ' + ROLE + ';\n')
    psql(DB, 'REVOKE ALL ON DATABASE ' + DB + ' FROM PUBLIC;\n')
    print('isolated control DB and owner login created; credential mode 0600')


if __name__ == '__main__':
    main()
