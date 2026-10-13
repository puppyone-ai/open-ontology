#!/usr/bin/env python3
"""Prepare this source's separate Gateway token database and fixed principals."""
import json
import os
from pathlib import Path
import secrets
import subprocess

BASE = Path('/tmp/oo-dl01w1-44-gateway-preview')
PG = 'oo-dl01w1-44-native-fresh-pg'
DB = 'oo_dl01w1_44_gateway_token'


def sql(database, statement):
    proc = subprocess.run(['docker','exec','-i',PG,'psql','-X','-q','-U','postgres',
        '-d',database,'-v','ON_ERROR_STOP=1'],input=statement,text=True,capture_output=True)
    if proc.returncode:
        raise RuntimeError('token DB bootstrap failed: '+proc.stderr.strip())


def secret(name):
    path=BASE/name
    fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
    value=secrets.token_hex(32)
    with os.fdopen(fd,'w') as stream: stream.write(value+'\n')
    return value


def main():
    BASE.mkdir(mode=0o700,parents=True,exist_ok=False)
    admin=secret('token-admin.pass')
    runtime=secret('token-runtime.pass')
    sql('postgres',"CREATE ROLE oo_token_admin LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE "
        "NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '"+admin+"';\n"
        "CREATE ROLE oo_token_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE "
        "NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '"+runtime+"';\n"
        'CREATE DATABASE '+DB+' OWNER oo_token_admin;\n')
    (BASE/'token-endpoint.json').write_text(json.dumps({'host':'oo-dl01w1-44-opensearch',
        'port':5432,'database':DB})+'\n')
    print('separate token DB and fixed restricted principals prepared')


if __name__ == '__main__': main()
