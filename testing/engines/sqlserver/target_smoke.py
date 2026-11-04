"""Real oo-load/Agent DB comparison of two committed source states, not scale QA."""
import json
import os
from pathlib import Path
import secrets
import subprocess
import threading
import time
import uuid


def check(root, out, source_port, source_password, source_ca, source_sql):
    name = 'oo-e-agent-' + uuid.uuid4().hex[:12]
    password = 'C7!' + secrets.token_hex(24)
    created = False
    stop = threading.Event()
    thread = None
    writer = {'commits': 0, 'errors': []}

    def run(cmd, data=None, timeout=300):
        proc = subprocess.run(cmd, input=data, text=True, capture_output=True, timeout=timeout)
        if proc.returncode:
            raise RuntimeError('target fixture command failed: ' + cmd[0])
        return proc.stdout

    def private(path, text):
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'w') as file:
            file.write(text)

    def pg(query):
        return run(['docker', 'exec', '-i', name, 'psql', '-X', '-U', 'postgres', '-v', 'ON_ERROR_STOP=1', '-A', '-t'], query)

    def source_rows():
        query = """SET NOCOUNT ON; USE oo_fixture;
SELECT id,value,CONVERT(nvarchar(60),amount) AS amount,
LOWER(CONVERT(varchar(max),bytes,2)) AS bytes,
CONVERT(nvarchar(30),DATEDIFF_BIG(day,CONVERT(date,'00010101',112),CONVERT(date,instant))*864000000000 + DATEDIFF_BIG(nanosecond,CONVERT(datetime2(7),CONVERT(date,instant)),instant)/100) AS instant
FROM dbo.fixture ORDER BY id FOR JSON PATH,INCLUDE_NULL_VALUES;
GO
"""
        raw = source_sql(query, full=True).stdout
        rows = json.loads(raw[raw.index('[{'):raw.rindex('}]')+2])
        return [[r[k] for k in ['id', 'value', 'amount', 'bytes', 'instant']] for r in rows]

    forward = "USE oo_fixture; BEGIN TRANSACTION; UPDATE dbo.fixture SET value=N'target-writer' WHERE id=2; UPDATE dbo.fixture SET id=11 WHERE id=10; INSERT dbo.fixture VALUES(12,N'writer',12,NULL,NULL); DELETE dbo.fixture WHERE id=1; COMMIT;\nGO\n"
    backward = "USE oo_fixture; BEGIN TRANSACTION; UPDATE dbo.fixture SET value=N'changed' WHERE id=2; UPDATE dbo.fixture SET id=10 WHERE id=11; INSERT dbo.fixture VALUES(1,N'A',1.12345678,0x0001,'2026-11-04T01:02:03.1234567'); DELETE dbo.fixture WHERE id=12; COMMIT;\nGO\n"

    def mutate():
        try:
            while not stop.is_set():
                source_sql(forward if writer['commits'] % 2 == 0 else backward)
                writer['commits'] += 1
                stop.wait(.02)
        except Exception as error:
            writer['errors'].append(type(error).__name__)

    try:
        directory = out / 'target-private'
        directory.mkdir(mode=0o700)
        store = directory / 'secrets'
        store.mkdir(mode=0o700)
        for file, value in [('source', source_password), ('source-ca', Path(source_ca).read_text()), ('target', password)]:
            private(store / file, value)
        private(directory / 'container.env', 'POSTGRES_PASSWORD=' + password + '\n')
        arch = run(['docker', 'info', '--format', '{{.Architecture}}']).strip()
        platform = 'linux/arm64' if arch in ('arm64', 'aarch64') else 'linux/amd64'
        pin = next(line.split()[2] for line in (root / 'testing/infra/images.pins').read_text().splitlines()
                   if line.startswith('postgres-17.10-bookworm ' + platform + ' '))
        image = 'postgres@' + pin
        run(['docker', 'pull', '--platform', platform, image], timeout=600)
        run(['docker', 'create', '--name', name, '--label', 'oo.lane=E', '--platform', platform,
             '--env-file', str(directory / 'container.env'), '--memory', '512m', '-p', '127.0.0.1::5432',
             '--tmpfs', '/var/lib/postgresql/data', image])
        created = True
        run(['docker', 'start', name])
        for _ in range(60):
            probe = subprocess.run(['docker', 'exec', name, 'pg_isready', '-U', 'postgres'], capture_output=True)
            if probe.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError('target readiness timeout')
        port = int(run(['docker', 'port', name, '5432/tcp']).strip().rsplit(':', 1)[1])
        stack = {'version': 'oo.stack/v1', 'sources': [{'id': 'fixture', 'engine': 'sqlserver', 'host': 'localhost',
                 'port': source_port, 'database': 'oo_fixture', 'user': 'oo_reader', 'password': {'secret_ref': 'source'},
                 'tls_ca': {'secret_ref': 'source-ca'}, 'scope': {'include': ['dbo.fixture'], 'exclude': []}}],
                 'model': {'provider': 'openai-compatible', 'base_url': 'https://example.invalid/v1', 'model': 'fixture', 'key': {'secret_ref': 'model'}}}
        (directory / 'stack.json').write_text(json.dumps(stack))
        target = {'host': '127.0.0.1', 'port': port, 'database': 'postgres', 'user': 'postgres',
                  'password': {'secret_ref': 'target'}, 'local_fixture': True, 'run_id': 'sqlserver-concurrent'}
        (directory / 'target.json').write_text(json.dumps(target))
        run(['cargo', 'build', '--manifest-path', str(root / 'tools/oo-load/Cargo.toml'), '--locked', '--bin', 'oo-load'], timeout=600)
        a = source_rows()
        source_sql(forward)
        b = source_rows()
        source_sql(backward)
        (out / 'source-gold-states.json').write_text(json.dumps([a, b]))
        thread = threading.Thread(target=mutate)
        thread.start()
        command = [str(root / 'tools/oo-load/target/debug/oo-load'), '--stack', str(directory / 'stack.json'),
                   '--source', 'fixture', '--target', str(directory / 'target.json'), '--secrets-dir', str(store), '--chunk-rows', '2']
        proc = subprocess.run(command, text=True, capture_output=True, timeout=180)
        stop.set()
        thread.join(timeout=180)
        (out / 'oo-load.log').write_text((proc.stdout + proc.stderr).replace(password, '[redacted]').replace(source_password, '[redacted]'))
        if proc.returncode or thread.is_alive() or writer['errors'] or not writer['commits']:
            raise RuntimeError('concurrent oo-load or writer failed')
        receipt = json.loads(proc.stdout.strip())
        relation = receipt['target']
        if not relation.startswith('oo_load_data.t_') or not relation.replace('_', '').replace('.', '').isalnum():
            raise RuntimeError('target relation identity')
        actual = json.loads(pg(f"SELECT json_agg(json_build_array(id,value,amount::text,encode(bytes,'hex'),instant::text) ORDER BY id)::text FROM {relation};"))
        (out / 'agent-db-rows.json').write_text(json.dumps(actual))
        if actual not in [a, b]:
            raise RuntimeError('Agent DB differs from both committed source states')
        replay = json.loads(run(command).strip())
        assert replay['replay'] is True
        assert replay['target_write_bytes'] == receipt['target_write_bytes']
        return {'rows': len(actual), 'writer_commits': writer['commits'], 'target_write_bytes': receipt['target_write_bytes'],
                'zero_difference': True, 'resume_no_rewrite': True, 'fixture_only': True}
    finally:
        stop.set()
        if thread:
            thread.join(timeout=180)
        if created:
            labels = json.loads(run(['docker', 'inspect', '--format', '{{json .Config.Labels}}', name]))
            if labels.get('oo.lane') != 'E':
                raise RuntimeError('target cleanup ownership mismatch')
            run(['docker', 'rm', '-f', '-v', name])
