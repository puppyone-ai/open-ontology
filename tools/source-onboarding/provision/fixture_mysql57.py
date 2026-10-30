#!/usr/bin/env python3
"""Owned MySQL 5.7 fixture for provisioning tests (never a customer database).

up     : create/refresh container oo-ob-fixture-mysql57 with GTID, ROW/FULL binlog,
         utf8mb4, TLS (CA keyCertSign,cRLSign; server cert SAN + serverAuth),
         CDC account (SELECT, REPLICATION SLAVE, REPLICATION CLIENT) and 4 tables
         with single-integer primary keys.
profile: create or rotate the SELECT-only profile reader oo_profile (no REPLICATION grants) in the running
         fixture, writing its password to SECRET_ROOT/provision/fixture-mysql57/profile.pass. The CDC account
         carries REPLICATION grants, which the network source-sql gate refuses by design.
status : print container/listener status (no secrets).
down   : remove the fixture container (its secret files are kept for audit).

Secrets are generated into 0600 files under SECRET_ROOT/provision/fixture-mysql57
(SECRET_ROOT = ~/.cache/oo-mysql-connection-whole, the trusted container's /secrets mount).
Passwords reach MySQL only via stdin or a mounted secret file, never argv.
"""
import argparse
import datetime
import json
import os
import secrets
import subprocess
import sys
import time
from pathlib import Path

NAME = 'oo-ob-fixture-mysql57'
IMAGE = 'mysql:5.7.44'
DB = 'ob_fixture'
CDC_USER = 'oo_cdc'
SECRET_ROOT = Path.home() / '.cache' / 'oo-mysql-connection-whole'
SERVER_ID = 61057
COUNTS = {'customers': 2000, 'products': 500, 'orders': 3000, 'order_items': 6000}


def fixture_dir(secret_root=SECRET_ROOT):
    return Path(secret_root) / 'provision' / 'fixture-mysql57'


def write_private(path, data):
    path = Path(path)
    if path.exists():
        path.unlink()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'wb') as f:
        f.write(data if isinstance(data, bytes) else data.encode())
    os.chmod(path, 0o600)


def run(argv, input_bytes=None, check=True):
    r = subprocess.run(argv, input=input_bytes, capture_output=True)
    if check and r.returncode != 0:
        raise SystemExit(f'fixture_command_failed: {argv[0]} {argv[1] if len(argv) > 1 else ""} exit={r.returncode}')
    return r


def make_certs(d):
    ca_key, ca_crt = d / 'ca.key', d / 'ca.pem'
    srv_key, srv_csr, srv_crt = d / 'server.key', d / 'server.csr', d / 'server.pem'
    ca_cnf, srv_cnf, srv_ext = d / 'ca.cnf', d / 'server.cnf', d / 'server.ext'
    write_private(ca_cnf, '[req]\ndistinguished_name = dn\nprompt = no\nx509_extensions = v3_ca\n'
                  '[dn]\nCN = oo-ob-fixture-ca\n[v3_ca]\nbasicConstraints = critical,CA:TRUE\n'
                  'keyUsage = critical,keyCertSign,cRLSign\nsubjectKeyIdentifier = hash\n')
    write_private(srv_cnf, '[req]\ndistinguished_name = dn\nprompt = no\n[dn]\nCN = ' + NAME + '\n')
    write_private(srv_ext, 'basicConstraints = CA:FALSE\nkeyUsage = critical,digitalSignature,keyEncipherment\n'
                  'extendedKeyUsage = serverAuth\nsubjectAltName = DNS:' + NAME + ',DNS:host.docker.internal,DNS:localhost,IP:127.0.0.1\n')
    run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(ca_key), '-out', str(ca_crt),
         '-days', '30', '-config', str(ca_cnf)])
    run(['openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(srv_key), '-out', str(srv_csr),
         '-config', str(srv_cnf)])
    run(['openssl', 'x509', '-req', '-in', str(srv_csr), '-CA', str(ca_crt), '-CAkey', str(ca_key),
         '-CAcreateserial', '-out', str(srv_crt), '-days', '30', '-extfile', str(srv_ext)])
    for path in (ca_key, srv_key):
        os.chmod(path, 0o600)
    for path in (ca_cnf, srv_cnf, srv_ext, srv_csr, d / 'ca.srl'):
        if path.exists():
            path.unlink()


def sql_setup(cdc_password):
    rows = []
    regions = ['north', 'south', 'east', 'west', '北区', '南区']
    rows.append(f"CREATE DATABASE IF NOT EXISTS {DB} DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;")
    rows.append(f"USE {DB};")
    rows.append("CREATE TABLE IF NOT EXISTS customers (id BIGINT NOT NULL PRIMARY KEY, name VARCHAR(80) NOT NULL, region VARCHAR(16) NOT NULL, credit_limit DECIMAL(12,2) NOT NULL, created_at DATETIME(3) NOT NULL, profile TEXT, is_active TINYINT(1) NOT NULL) ENGINE=InnoDB;")
    rows.append("CREATE TABLE IF NOT EXISTS products (id BIGINT NOT NULL PRIMARY KEY, sku VARCHAR(32) NOT NULL, title VARCHAR(120) NOT NULL, price DECIMAL(10,2) NOT NULL, weight_grams INT UNSIGNED NOT NULL) ENGINE=InnoDB;")
    rows.append("CREATE TABLE IF NOT EXISTS orders (id BIGINT NOT NULL PRIMARY KEY, customer_id BIGINT NOT NULL, status VARCHAR(16) NOT NULL, total DECIMAL(12,2) NOT NULL, placed_at DATETIME NOT NULL, note VARCHAR(200)) ENGINE=InnoDB;")
    rows.append("CREATE TABLE IF NOT EXISTS order_items (id BIGINT NOT NULL PRIMARY KEY, order_id BIGINT NOT NULL, product_id BIGINT NOT NULL, qty INT NOT NULL, unit_price DECIMAL(10,2) NOT NULL) ENGINE=InnoDB;")

    def batch(table, cols, tuples):
        for i in range(0, len(tuples), 250):
            chunk = tuples[i:i + 250]
            rows.append(f"INSERT INTO {table} ({cols}) VALUES " + ','.join(chunk) + ';')

    cust = []
    for i in range(1, COUNTS['customers'] + 1):
        cust.append(f"({i},'customer-{i:05d}','{regions[i % len(regions)]}',{(i * 37) % 9000 + 500}.25,"
                    f"'2026-01-{(i % 28) + 1:02d} 08:{i % 60:02d}:00.123',"
                    f"'profile {i} 测试 ok',{i % 2})")
    batch('customers', 'id,name,region,credit_limit,created_at,profile,is_active', cust)
    prod = []
    for i in range(1, COUNTS['products'] + 1):
        prod.append(f"({i},'SKU-{i:06d}','product {i} title',{(i * 13) % 900 + 10}.50,{(i * 7) % 5000 + 1})")
    batch('products', 'id,sku,title,price,weight_grams', prod)
    orders = []
    for i in range(1, COUNTS['orders'] + 1):
        orders.append(f"({i},{(i * 11) % COUNTS['customers'] + 1},'{['new', 'paid', 'shipped'][i % 3]}',"
                      f"{(i * 19) % 4000 + 20}.00,'2026-02-{(i % 27) + 1:02d} 12:00:00',"
                      f"{'NULL' if i % 4 == 0 else repr('note ' + str(i))})")
    batch('orders', 'id,customer_id,status,total,placed_at,note', orders)
    items = []
    for i in range(1, COUNTS['order_items'] + 1):
        items.append(f"({i},{(i % COUNTS['orders']) + 1},{(i * 3) % COUNTS['products'] + 1},{i % 5 + 1},{(i * 13) % 900 + 10}.50)")
    batch('order_items', 'id,order_id,product_id,qty,unit_price', items)
    rows.append(f"CREATE USER IF NOT EXISTS '{CDC_USER}'@'%' IDENTIFIED BY '{cdc_password}' REQUIRE SSL;")
    rows.append(f"ALTER USER '{CDC_USER}'@'%' IDENTIFIED BY '{cdc_password}' REQUIRE SSL;")
    rows.append(f"GRANT SELECT, REPLICATION SLAVE, REPLICATION CLIENT ON *.* TO '{CDC_USER}'@'%';")
    rows.append('FLUSH PRIVILEGES;')
    return '\n'.join(rows).encode('utf-8')


def wait_ready(root_secret_name='root'):
    for _ in range(180):
        r = run(['docker', 'exec', NAME, 'sh', '-c', f'mysql -uroot -p"$(cat /run/secrets/{root_secret_name})" -N -e "select 1" >/dev/null 2>&1'], check=False)
        if r.returncode == 0:
            return
        time.sleep(2)
    raise SystemExit('fixture_not_ready')


PROFILE_USER = 'oo_profile'


def profile_sql(profile_password):
    return (f"CREATE USER IF NOT EXISTS '{PROFILE_USER}'@'%' IDENTIFIED BY '{profile_password}' REQUIRE SSL;\n"
            f"ALTER USER '{PROFILE_USER}'@'%' IDENTIFIED BY '{profile_password}' REQUIRE SSL;\n"
            f"REVOKE ALL PRIVILEGES, GRANT OPTION FROM '{PROFILE_USER}'@'%';\n"
            f"GRANT SELECT ON {DB}.* TO '{PROFILE_USER}'@'%';\n"
            "FLUSH PRIVILEGES;\n").encode('utf-8')


def profile(port, secret_root):
    d = fixture_dir(secret_root)
    if not container_exists() or not (d / 'root.pass').exists():
        raise SystemExit('fixture_not_up')
    pw_file = d / 'profile.pass'
    if pw_file.exists():
        pw_file.unlink()
    write_private(pw_file, secrets.token_urlsafe(24) + '\n')
    password = pw_file.read_text().strip()
    run(['docker', 'exec', '-i', NAME, 'sh', '-c', 'mysql -uroot -p"$(cat /run/secrets/root)" --default-character-set=utf8mb4'],
        input_bytes=profile_sql(password))
    del password
    print(json.dumps({'container': NAME, 'profile_user': PROFILE_USER, 'grants': 'SELECT on %s.*' % DB,
                      'secret_file': str(pw_file)}))


def container_exists():
    return run(['docker', 'inspect', NAME], check=False).returncode == 0


def up(port, secret_root):
    d = fixture_dir(secret_root)
    d.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(d, 0o700)
    root_file, cdc_file = d / 'root.pass', d / 'cdc.pass'
    if not root_file.exists():
        write_private(root_file, secrets.token_urlsafe(24) + '\n')
    if not cdc_file.exists():
        write_private(cdc_file, secrets.token_urlsafe(24) + '\n')
    if not (d / 'ca.pem').exists():
        make_certs(d)
    if container_exists():
        run(['docker', 'rm', '-f', NAME])
    run(['docker', 'run', '-d', '--name', NAME, '-p', f'127.0.0.1:{port}:3306',
         '-v', f'{root_file}:/run/secrets/root:ro',
         '-e', 'MYSQL_ROOT_PASSWORD_FILE=/run/secrets/root',
         '--memory', '2g', '--cpus', '2',
         IMAGE, '--server-id=' + str(SERVER_ID), '--gtid-mode=ON', '--enforce-gtid-consistency=ON',
         '--log-bin=mysql-bin', '--binlog-format=ROW', '--binlog-row-image=FULL',
         '--character-set-server=utf8mb4', '--collation-server=utf8mb4_general_ci',
         '--ssl-ca=/etc/mysql/ssl/ca.pem', '--ssl-cert=/etc/mysql/ssl/server.pem', '--ssl-key=/etc/mysql/ssl/server.key',
         '--default-authentication-plugin=mysql_native_password', '--expire-logs-days=7'])
    wait_ready()
    run(['docker', 'exec', NAME, 'mkdir', '-p', '/etc/mysql/ssl'])
    for name in ('ca.pem', 'server.pem', 'server.key'):
        run(['docker', 'cp', str(d / name), f'{NAME}:/etc/mysql/ssl/{name}'])
    run(['docker', 'exec', NAME, 'sh', '-c', 'chown mysql:mysql /etc/mysql/ssl/* && chmod 600 /etc/mysql/ssl/server.key'])
    run(['docker', 'restart', NAME])
    wait_ready()
    cdc_password = cdc_file.read_text().strip()
    run(['docker', 'exec', '-i', NAME, 'sh', '-c', 'mysql -uroot -p"$(cat /run/secrets/root)" --default-character-set=utf8mb4'],
        input_bytes=sql_setup(cdc_password))
    expected = {'protocol': 'oo.source-onboarding.fixture/v1', 'database': DB, 'tables': COUNTS,
                'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'port': port}
    write_private(d / 'expected.json', json.dumps(expected, sort_keys=True) + '\n')
    print(json.dumps({'container': NAME, 'port': port, 'database': DB, 'expected_counts': COUNTS, 'secret_dir': str(d)}))


def status(secret_root):
    r = run(['docker', 'inspect', '--format', '{{.State.Status}} {{.Config.Image}}', NAME], check=False)
    print(json.dumps({'container': NAME, 'docker': r.stdout.decode().strip() if r.returncode == 0 else 'absent'}))


def down(secret_root):
    if container_exists():
        run(['docker', 'rm', '-f', NAME])
    print(json.dumps({'container': NAME, 'removed': True}))


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument('mode', choices=['up', 'status', 'down', 'profile'])
    p.add_argument('--port', type=int, default=23306)
    p.add_argument('--secret-root', default=str(SECRET_ROOT))
    a = p.parse_args(argv)
    if a.mode == 'up':
        up(a.port, a.secret_root)
    elif a.mode == 'profile':
        profile(a.port, a.secret_root)
    elif a.mode == 'status':
        status(a.secret_root)
    else:
        down(a.secret_root)


if __name__ == '__main__':
    sys.exit(main())
