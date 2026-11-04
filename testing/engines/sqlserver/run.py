#!/usr/bin/env python3
"""Sequential lane-owned synthetic smoke runner; not full T0-T8 acceptance."""
import argparse
import json
import os
import re
from pathlib import Path
import secrets
import subprocess
import time
import uuid
import traceback
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[3]


def decode_audit(raw):
    # sqlcmd wraps long cells; transport UTF-16 XML as hex to preserve CDATA exactly.
    parts = []
    for line in raw.splitlines():
        line = line.strip()
        if not line or line == 'audit_hex' or set(line) == {'-'}:
            continue
        if not re.fullmatch('[0-9a-fA-F]+', line):
            raise ValueError('unexpected audit transport output')
        parts.append(line)
    return bytes.fromhex(''.join(parts)).decode('utf-16-le')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--version', choices=['2019', '2022', '2025'], required=True)
    parser.add_argument('--mode', choices=['ct', 'cdc'], default='ct')
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    if not args.run_id.replace('-', '').replace('_', '').isalnum():
        raise SystemExit('invalid run-id')
    out = ROOT / 'evidence' / args.run_id / (args.version + '-' + args.mode)
    out.mkdir(parents=True, exist_ok=False)
    os.chmod(out, 0o700)
    sa, password = 'A9!' + secrets.token_hex(24), 'B8!' + secrets.token_hex(24)
    name = 'oo-e-sqlserver-' + uuid.uuid4().hex[:12]
    created = False
    connection_peak = 0
    results = {'version': args.version, 'mode': args.mode, 'status': 'failed', 'container': name}

    def private(name, text):
        path = out / name
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        with os.fdopen(fd, 'w') as file:
            file.write(text)
        return path

    def run(command, *, input=None, timeout=180, check=True, log=None):
        proc = subprocess.run(command, input=input, text=True, capture_output=True, timeout=timeout)
        safe = (proc.stdout + proc.stderr).replace(sa, '[redacted]').replace(password, '[redacted]')
        if log:
            (out / log).write_text(safe)
        if check and proc.returncode:
            raise RuntimeError(f'{command[0]} failed ({proc.returncode}); see {log or "private evidence"}')
        return proc

    def sql(statement, log=None, check=True, full=False):
        return run(['docker', 'exec', '-i', name, 'sh', '-c',
                    'export SQLCMDPASSWORD="$MSSQL_SA_PASSWORD"; '
                    'if [ -x /opt/mssql-tools18/bin/sqlcmd ]; then c=/opt/mssql-tools18/bin/sqlcmd; '
                    'else c=/opt/mssql-tools/bin/sqlcmd; fi; '
                    'exec "$c" -S localhost -U sa -C -b -r1 ' + ('-y0 -w65535' if full else '-h-1 -W')],
                   input=statement, log=log, check=check)

    def reader(label, prior=None, barrier=False):
        path = out / label
        config = {'port': port, 'password_file': str(secret_file), 'ca_file': str(out / 'ca.pem'),
                  'output': str(path), 'mode': args.mode, 'prior': str(prior) if prior else None,
                  'barrier': barrier}
        config_file = out / (label + '.json')
        config_file.write_text(json.dumps(config))
        log = open(out / (label + '.log'), 'w')
        proc = subprocess.Popen([str(binary), str(config_file)], stdout=log, stderr=log)
        return proc, log, path

    def finish(proc, log, label):
        nonlocal connection_peak
        deadline = time.monotonic() + 180
        while proc.poll() is None:
            sample = sql("SET NOCOUNT ON; SELECT COUNT_BIG(*) FROM sys.dm_exec_sessions WHERE program_name=N'oo-sqlserver-source';\nGO\n")
            connection_peak = max(connection_peak, int(sample.stdout.strip()))
            if time.monotonic() > deadline:
                proc.kill()
                proc.wait()
                log.close()
                raise RuntimeError('reader timeout')
            time.sleep(.02)
        code = proc.wait()
        log.close()
        if code:
            raise RuntimeError(f'reader {label} failed ({code})')
        return json.loads((out / (label + '.log')).read_text())

    try:
        pin = next(line.split() for line in (ROOT / 'testing/engines/sqlserver/images.pins').read_text().splitlines()
                   if line.startswith('mssql-' + args.version + '-'))
        image = 'mcr.microsoft.com/mssql/server@' + pin[2]
        run(['docker', 'pull', '--platform', 'linux/amd64', image], timeout=900, log='pull.log')
        secret_file = private('reader.secret', password)
        env = private('container.env', f'ACCEPT_EULA=Y\nMSSQL_PID=Developer\nMSSQL_SA_PASSWORD={sa}\nMSSQL_AGENT_ENABLED=true\n')
        run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2',
             '-subj', '/CN=OO fixture CA', '-keyout', str(out / 'ca-key.pem'),
             '-out', str(out / 'ca.pem')], log='ca.log')
        run(['openssl', 'req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
             '-keyout', str(out / 'key.pem'), '-out', str(out / 'server.csr')], log='csr.log')
        ext = private('server.ext', 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n')
        run(['openssl', 'x509', '-req', '-in', str(out / 'server.csr'), '-CA', str(out / 'ca.pem'),
             '-CAkey', str(out / 'ca-key.pem'), '-CAcreateserial', '-days', '2', '-extfile', str(ext),
             '-out', str(out / 'cert.pem')], log='certificate.log')
        os.chmod(out / 'key.pem', 0o600)
        os.chmod(out / 'ca-key.pem', 0o600)
        private('mssql.conf', '[network]\ntlscert = /fixture/cert.pem\ntlskey = /fixture/key.pem\ntlsprotocols = 1.2\nforceencryption = 1\n')
        run(['docker', 'create', '--name', name, '--platform', 'linux/amd64', '--label', 'oo.lane=E',
             '--label', 'oo.run_id=' + args.run_id, '--user', '0', '--memory', '3g',
             '--env-file', str(env), '-p', '127.0.0.1::1433',
             '-v', str(out) + ':/fixture:ro', '-v', str(out / 'mssql.conf') + ':/var/opt/mssql/mssql.conf:ro',
             image], log='create.log')
        created = True
        run(['docker', 'start', name], log='start.log')
        deadline = time.monotonic() + 300
        while True:
            if sql('SELECT 1;\nGO\n', check=False).returncode == 0:
                break
            if time.monotonic() > deadline:
                raise RuntimeError('SQL Server startup timeout')
            time.sleep(2)
        port = int(run(['docker', 'port', name, '1433/tcp']).stdout.strip().rsplit(':', 1)[1])
        seed = f"""CREATE DATABASE oo_fixture;
GO
ALTER DATABASE oo_fixture SET ALLOW_SNAPSHOT_ISOLATION ON;
ALTER DATABASE oo_fixture SET CHANGE_TRACKING = ON (CHANGE_RETENTION = 7 DAYS, AUTO_CLEANUP = ON);
CREATE LOGIN oo_reader WITH PASSWORD = '{password}';
GRANT VIEW SERVER STATE TO oo_reader;
GO
USE oo_fixture;
CREATE USER oo_reader FOR LOGIN oo_reader;
GRANT SELECT TO oo_reader;
GRANT VIEW DEFINITION TO oo_reader;
CREATE TABLE dbo.fixture(id int NOT NULL PRIMARY KEY, value nvarchar(100) COLLATE Latin1_General_CI_AS NULL, amount decimal(38,8), bytes varbinary(16), instant datetime2(7));
ALTER TABLE dbo.fixture ENABLE CHANGE_TRACKING;
GRANT VIEW CHANGE TRACKING ON dbo.fixture TO oo_reader;
INSERT dbo.fixture VALUES (1,N'A',1.12345678,0x0001,'2026-11-04T01:02:03.1234567'),(2,N'b',NULL,NULL,NULL),(3,N'',0,0x,NULL),(4,NULL,-9.5,0xFF,NULL),(5,N'e',5,NULL,NULL),(6,N'f',6,NULL,NULL),(7,N'g',7,NULL,NULL),(8,N'h',8,NULL,NULL),(9,N'i',9,NULL,NULL);
GO
"""
        if args.mode == 'cdc':
            seed += "EXEC sys.sp_cdc_enable_db;\nEXEC sys.sp_cdc_enable_table @source_schema=N'dbo',@source_name=N'fixture',@role_name=NULL,@supports_net_changes=0;\nGO\n"
        sql(seed, log='seed.log')
        if args.mode == 'cdc':
            time.sleep(10)
        binary = ROOT / 'testing/engines/sqlserver/target/debug/oo-sqlserver-conformance'
        run(['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'testing/engines/sqlserver/Cargo.toml')], timeout=600, log='build.log')
        proc, log, path = reader('snapshot', barrier=True)
        until = time.monotonic() + 90
        while not (path / 'ready').exists():
            if proc.poll() is not None or time.monotonic() > until:
                proc.kill() if proc.poll() is None else None
                proc.wait()
                log.close()
                raise RuntimeError('snapshot did not reach writer barrier')
            time.sleep(.05)
        sample = sql("SET NOCOUNT ON; SELECT COUNT_BIG(*) FROM sys.dm_exec_sessions WHERE program_name=N'oo-sqlserver-source';\nGO\n", log='reader-connections-before-writer.log')
        connection_peak = max(connection_peak, int(sample.stdout.strip()))
        sql("USE oo_fixture; UPDATE dbo.fixture SET value=N'changed' WHERE id=2; DELETE dbo.fixture WHERE id=3; INSERT dbo.fixture VALUES(10,N'new',10,NULL,NULL);\nGO\n", log='writer.log')
        (path / 'go').write_text('go')
        snapshot = finish(proc, log, 'snapshot')
        assert sum(r['rows'] for r in snapshot['receipts']) == 9
        if args.mode == 'ct':
            assert len(snapshot['same_session_changes']['changes']) == 3
        assert snapshot['values'] == [[1,'A'],[2,'b'],[3,''],[4,None],[5,'e'],[6,'f'],[7,'g'],[8,'h'],[9,'i']]
        if args.mode == 'cdc':
            time.sleep(10)
        proc, log, _ = reader('changes', path / 'position.json')
        changes = finish(proc, log, 'changes')
        assert changes['changes'] is not None
        assert len(changes['changes']['changes']) == (3 if args.mode == 'ct' else 4)
        proc, log, _ = reader('idle', out / 'changes/position.json')
        idle = finish(proc, log, 'idle')
        assert idle['changes'] is None
        sql("CREATE EVENT SESSION oo_reader_audit ON SERVER ADD EVENT sqlserver.sql_batch_completed(ACTION(sqlserver.client_app_name,sqlserver.username) WHERE ([sqlserver].[client_app_name]=N'oo-sqlserver-source')) ADD TARGET package0.ring_buffer WITH (MAX_MEMORY=4096 KB,MAX_DISPATCH_LATENCY=1 SECONDS); ALTER EVENT SESSION oo_reader_audit ON SERVER STATE=START;\nGO\n", log='audit-setup.log')
        unchanged_sql = "SET NOCOUNT ON; USE oo_fixture; SELECT * FROM dbo.fixture ORDER BY id FOR JSON PATH, INCLUDE_NULL_VALUES;\nGO\n"
        before = sql(unchanged_sql, full=True, log='readonly-before.log').stdout
        proc, log, _ = reader('after')
        after = finish(proc, log, 'after')
        proc, log, _ = reader('after-idle')
        after_idle = finish(proc, log, 'after-idle')
        assert after['digests'] == after_idle['digests']
        assert snapshot['digests'] != after['digests']
        assert before == sql(unchanged_sql, full=True, log='readonly-after.log').stdout
        time.sleep(2)
        audit = sql("SET NOCOUNT ON; SELECT CONVERT(varchar(max),CONVERT(varbinary(max),CONVERT(nvarchar(max),t.target_data)),2) AS audit_hex FROM sys.dm_xe_session_targets t JOIN sys.dm_xe_sessions s ON t.event_session_address=s.address WHERE s.name=N'oo_reader_audit';\nGO\n", full=True, log='reader-audit-hex.log').stdout
        audit = decode_audit(audit)
        (out / 'reader-audit.xml').write_text(audit)
        root = ET.fromstring(audit)
        assert root.attrib.get('truncated', '0') == '0'
        events = root.findall('event')
        assert events, 'empty audit is not proof'
        classes = {'select': 0, 'snapshot_session': 0}
        for event in events:
            assert event.find("action[@name='username']/value").text == 'oo_reader'
            statement = event.find("data[@name='batch_text']/value").text.strip()
            if statement in ('SET TRANSACTION ISOLATION LEVEL SNAPSHOT; BEGIN TRANSACTION', 'ROLLBACK TRANSACTION'):
                classes['snapshot_session'] += 1
            else:
                assert statement.upper().startswith('SELECT '), 'unattributed reader statement'
                code = re.sub(r"N?'(?:''|[^'])*'|\[(?:\]\]|[^\]])*\]", ' identifier ', statement)
                assert not re.search(r'\b(INTO|INSERT|UPDATE|DELETE|MERGE|CREATE|ALTER|DROP|EXEC|EXECUTE|GRANT|REVOKE|DENY|TRUNCATE|BACKUP|RESTORE|DBCC|OPENROWSET|OPENDATASOURCE)\b', code, re.I), 'reader write or unapproved call'
                classes['select'] += 1
        dropped = sql("SET NOCOUNT ON; SELECT dropped_event_count FROM sys.dm_xe_sessions WHERE name=N'oo_reader_audit';\nGO\n", log='audit-dropped.log').stdout.strip()
        assert dropped == '0'
        results['reader_connection_peak'] = connection_peak
        assert connection_peak == 1, f'reader connection peak {connection_peak}'
        for read in [snapshot, after, after_idle]:
            timing = read['resources']
            assert timing['pacing_ns'] >= 3 * timing['read_ns']
        results.update(reader_audit=classes, reader_connection_peak=connection_peak, pacing_budget_pass=True)
        sql("SELECT CONVERT(nvarchar(128),SERVERPROPERTY('ProductVersion'));\nGO\n", log='version.log')
        from target_smoke import check
        results['agent_db'] = check(ROOT, out, port, password, out / 'ca.pem', sql)
        results.update(status='smoke_pass', snapshot_rows=9, changes=len(changes['changes']['changes']),
                       idle_changes=0, c5_changed=True, c5_idle_equal=True,
                       full_acceptance=False, missing=['all fixture shapes and scale', 'full T2 cycle/target checks', 'model T8 not applicable'])
    except Exception as error:
        results['error'] = (type(error).__name__ + ': ' + str(error)).replace(sa, '[redacted]').replace(password, '[redacted]')
        (out / 'failure.log').write_text(traceback.format_exc().replace(sa, '[redacted]').replace(password, '[redacted]'))
    finally:
        if created:
            run(['docker', 'logs', name], check=False, log='server.log')
            run(['docker', 'rm', '-f', '-v', name], check=True, log='cleanup.log')
        (out / 'result.json').write_text(json.dumps(results, indent=2) + '\n')
        print(json.dumps(results))
    return 0 if results['status'] == 'smoke_pass' else 1


if __name__ == '__main__':
    raise SystemExit(main())
