#!/usr/bin/env python3
"""One onboard step for the Doris owned fixture.

The six-field entry attaches to the serving pack and the finished cycles.
It does not replay fixture DML, import, GRANT, or a new product chain.
A copied historical receipt is not a result. C1 stays the reader's
refused_unpublished binlog.
"""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = Path('/tmp/oo-doris-connection-whole')
RECEIPT_PY = ROOT / 'testing/dl01-driver-preflight/doris/scripts/capture_receipt.py'
COMPARE = Path(__file__).with_name('compare_current.py')
CAPTURE = Path(__file__).with_name('connection_capture.py')
PRODUCT_FLOW = ROOT / 'tools/source-onboarding/product_flow.py'
DIALOGUE = ROOT / 'tools/agent-product-verifier/dialogue.py'
# build4 is the tested ELF. The pinned loader reads a driver only when it is on
# the allowlist or at most 32MiB. strip --strip-debug of that same ELF is 18MiB.
READER = BASE / 'driver-stripped/doris-reader'
READER_SHA = 'b874654f023e4beb16b4376161f04a4fb33113ad337591ec05dc68bb8b592aba'
ADM = Path('/tmp/oo-pg46-runtime-finish/continuous/oo-source-admission')
PIPE = Path('/tmp/oo-pg46-runtime-finish/oo-pipeline')
SCHEMA = ROOT / 'evidence/5983d507-bd78-474b-9417-32909ed054eb/DL01/admission/doris-g2/inputs/schema-preimage.json'
TYPE = ROOT / 'evidence/5983d507-bd78-474b-9417-32909ed054eb/DL01/admission/doris-g2/inputs/type-preimage.json'
ADMISSION = BASE / 'import-inputs/admission-config.json'
RUST = 'docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
PG = 'oo-doris-connection-whole-pg'
FE = 'oo-dl01w1-43-ns2-fe'
OWNER = '5106fed0-5bcd-4aaf-8df7-838c95a7d7e2'
WORKER = 'doris-connection-whole'
EXPECTED = {
    'type': 'doris', 'database': 'oo_dl01w1_43', 'host': '172.16.143.10',
    'port': 9030, 'user': 'oo_reader', 'secret_ref': 'secret://doris-connection-whole/reader',
}
CHANGED = ('dup_events', 'uniq_acct', 'part_day', 'nz_obs')
SOCK = Path('/Users/redux/.docker/run/docker.sock')

_spec = importlib.util.spec_from_file_location('doris_capture_receipt', RECEIPT_PY)
_receipt = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_receipt)
TABLES = _receipt.TABLES
column_decl = _receipt.column_decl


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')
    os.chmod(path, 0o600)


def kept(proc, directory):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / 'stdout').write_bytes(proc.stdout or b'')
    (directory / 'stderr').write_bytes(proc.stderr or b'')
    (directory / 'exit').write_text(str(proc.returncode) + '\n')
    return proc


def psql(sql, timeout=60):
    return subprocess.run(
        ['docker', 'exec', '-i', PG, 'psql', '-X', '-U', 'postgres', '-d', 'dorisconnection2',
         '-v', 'ON_ERROR_STOP=1', '-At'],
        input=sql.encode(), capture_output=True, timeout=timeout)


def mysql(sql):
    return subprocess.run(
        ['docker', 'exec', FE, 'mysql', '-h127.0.0.1', '-P9030', '-uroot',
         '--database', 'oo_dl01w1_43', '-N', '-B', '--raw', '-e', sql],
        capture_output=True, timeout=60)


def one(proc, label):
    if proc.returncode:
        raise RuntimeError(label)
    return proc.stdout.decode().strip()


def generation(table):
    text = one(psql(
        "SELECT COALESCE(MAX(source_generation),0) FROM _oo.table_freshness "
        "WHERE source='dorispreview' AND schema_name='oo_dl01w1_43' "
        f"AND table_name='{table}'"), 'freshness')
    return int(text or '0')


def watermark():
    text = one(psql(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object('source',source,'generation',source_generation,'state',state)),'[]'::jsonb)::text "
        "FROM _oo.source_watermark"), 'watermark')
    return json.loads(text)


def count(sql):
    return int(one(mysql(sql), 'mysql'))


def partition_name(name):
    if not name or not all(c.isalnum() or c == '_' for c in name):
        return False
    if name.startswith('p'):
        return True
    rest = name[5:] if name.startswith('cycle') else ''
    return rest.isdigit() and int(rest) > 0


def parent_for(generation_value):
    if generation_value < 1:
        raise ValueError('generation')
    return None if generation_value == 1 else generation_value - 1


def require_connection(cfg):
    for key, value in EXPECTED.items():
        if cfg.get(key) != value:
            raise ValueError('connection is not the authorized owned fixture')
    password = Path(cfg['password_file'])
    if not password.is_file() or password.stat().st_mode & 0o077:
        raise ValueError('secret ref must resolve a private file')
    if sha256(cfg['reader']) != READER_SHA or sha256(READER) != READER_SHA:
        raise ValueError('reader pin')
    if sha256(ADM) != '07e7651a98b4ef76582f67666e1245065546db204a50899554981619efc05dbf':
        raise ValueError('admission pin')
    if sha256(PIPE) != '2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4':
        raise ValueError('loader pin')


def capture_all(config_path, directory):
    if (directory / 'capture-receipt.json').exists():
        return json.loads((directory / 'capture-receipt.json').read_text())
    directory.mkdir(mode=0o700, exist_ok=True)
    proc = subprocess.run(
        [sys.executable, str(CAPTURE), '--config', str(config_path), '--run', str(directory)],
        capture_output=True, timeout=420)
    kept(proc, directory / 'call')
    if proc.returncode:
        raise RuntimeError('capture_refused')
    return json.loads((directory / 'capture-receipt.json').read_text())


def capture_partition(cfg, directory, partition='p202604'):
    tick = directory / 'evidence' / 'part.json'
    if tick.exists():
        body = json.loads(tick.read_text())
    else:
        if (directory / 'evidence').exists():
            raise RuntimeError('c4_failure_retained')
        directory.parent.mkdir(mode=0o700, exist_ok=True)
        directory.mkdir(mode=0o700, exist_ok=True)
        evidence = directory / 'evidence'
        evidence.mkdir(mode=0o700)
        command = ['docker', 'run', '--rm', '--network', cfg['network'], '--pull', 'never',
                   '--platform', 'linux/arm64', '--memory', '512m',
                   '-v', f'{cfg["reader"]}:/opt/doris-reader:ro', '-v', f'{evidence}:/evidence',
                   '-v', f'{cfg["password_file"]}:/secrets/reader.password:ro',
                   '--entrypoint', '/opt/doris-reader', RUST,
                   'capture-connection-partition', '/evidence', '/secrets', cfg['host'], str(cfg['port']),
                   cfg['database'], cfg['user'], str(cfg['period_ms']), 'part', str(cfg['max_rows_per_table']), partition]
        proc = subprocess.run(command, capture_output=True, timeout=240)
        kept(proc, directory / 'call')
        if proc.returncode or not tick.exists():
            raise RuntimeError('c4_capture_refused')
        body = json.loads(tick.read_text())
    if body.get('partition_scope', {}).get('partition_id') != partition:
        raise RuntimeError('c4_partition')
    if body.get('product_reader_write_attempts') != 0:
        raise RuntimeError('reader_wrote')
    table = next(item for item in body['tables'] if item['name'] == 'part_day')
    if table.get('route') != 'C4' or table.get('delete_mode') != 'partition_replace' or not table.get('arrow'):
        raise RuntimeError('c4_route')
    return tick


def linux(out, name, binary, args, payload=None, env=None):
    # Default onboard runs live under ~/.cache, outside BASE. The loader opens those paths.
    run_root = out.resolve()
    for candidate in out.resolve().parents:
        if candidate.name == 'import':
            run_root = candidate.parent
            break
    cmd = ['docker', 'run', '--rm', '-i', '--network', 'oo-dl01w1-43-ns2', '--platform', 'linux/arm64',
           '--pull', 'never', '-v', f'{BASE}:{BASE}', '-v', f'{BASE.resolve()}:{BASE.resolve()}',
           '-v', f'{ROOT}:{ROOT}:ro', '-v', f'{Path("/tmp/oo-pg46-runtime-finish")}:/tmp/oo-pg46-runtime-finish:ro',
           '-v', f'{run_root}:{run_root}']
    if run_root.resolve() != run_root:
        cmd += ['-v', f'{run_root.resolve()}:{run_root.resolve()}']
    for key, value in (env or {}).items():
        cmd += ['-e', key + '=' + value]
    cmd += ['--entrypoint', binary, RUST, *args]
    proc = subprocess.run(cmd, input=payload, capture_output=True, timeout=180)
    kept(proc, out / name)
    if proc.returncode:
        raise RuntimeError(name + '_refused')
    return proc.stdout


def claim(table, generation_value, arrow_sha):
    digest = arrow_sha
    # Failed rows stay: build4 key without a suffix, stripped key ending b874654f.
    # enqueue replays a failed task instead of queuing it, so this key differs.
    dedupe = f'{table}-g{generation_value}-{arrow_sha[:16]}-{READER_SHA[:8]}p'
    queued = one(psql(
        "SELECT status||'|'||task_id::text||'|'||generation::text FROM jobs.enqueue("
        f"'{OWNER}'::uuid, 'native_update', 1, '{{}}', 'doris-conn', '{digest}', "
        f"'{dedupe}', clock_timestamp(), 3)"), 'enqueue')
    status, task, _gen = queued.split('|')
    if status not in {'enqueued', 'replayed'}:
        raise RuntimeError('enqueue_' + status)
    claimed = one(psql(
        "SELECT status||'|'||task_id::text||'|'||generation::text||'|'||state FROM jobs.claim("
        f"'{OWNER}'::uuid, ARRAY['native_update'], '{WORKER}')"), 'claim')
    parts = claimed.split('|')
    if parts[0] != 'claimed' or parts[1] != task:
        raise RuntimeError('claim_' + parts[0])
    return task, int(parts[2])


def beat(task, generation_value, stop):
    while not stop.wait(10):
        psql("SELECT status FROM jobs.heartbeat("
             f"'{OWNER}'::uuid, '{task}'::uuid, '{WORKER}', {generation_value})")


def finish_task(task, generation_value, success):
    if success:
        sql = ("SELECT status FROM jobs.complete_success("
               f"'{OWNER}'::uuid, '{task}'::uuid, '{WORKER}', {generation_value})")
    else:
        sql = ("SELECT status FROM jobs.complete_failure("
               f"'{OWNER}'::uuid, '{task}'::uuid, '{WORKER}', {generation_value}, 'payload_rejected')")
    psql(sql)


def ack_document(result):
    receipt = result['partition_target_receipt']
    return {
        'protocol': 'oo.pipeline.native-partition-ack/v1',
        'source_id': result['source_id'],
        'logical_schema': result['logical_schema'],
        'logical_table': result['logical_table'],
        'partition_id': receipt['partition_scope']['partition_id'],
        'run_id': result['run_id'],
        'batch_id': receipt['batch_id'],
        'payload_digest': result['payload_digest'],
        'job_task_id': receipt['job_task_id'],
        'job_generation': receipt['job_generation'],
    }


def ensure_ack_target():
    path = BASE / 'ack-target.json'
    if not path.exists():
        write(path, {
            'host': PG, 'port': 5432, 'database': 'dorisconnection2',
            'runtime_principal': 'oo_doris_loader',
            'password_file': str(BASE / 'loader.password'),
            'ca_file': str(BASE / 'tls/ca.crt'), 'ssl': 'verify-full'})
    if path.stat().st_mode & 0o077:
        raise RuntimeError('ack_target_mode')
    return path


def grant_ack_execute():
    priv = one(psql(
        "SELECT has_function_privilege('oo_doris_loader',"
        "'_oo.native_partition_ack(text,text,text,text,uuid,text,text,uuid,bigint)','EXECUTE')"), 'ack_priv')
    if priv != 't':
        one(psql(
            'GRANT EXECUTE ON FUNCTION _oo.native_partition_ack(text,text,text,text,uuid,text,text,uuid,bigint) '
            'TO oo_doris_loader'), 'ack_grant')


def ack_native(dest, result):
    path = ensure_ack_target()
    digest = sha256(path)
    env = {
        'OO_NATIVE_CONTROL_TARGET': str(path),
        'OO_NATIVE_CONTROL_TARGET_SHA256': digest,
        'OO_NATIVE_ACK_TARGET': str(path),
        'OO_NATIVE_ACK_TARGET_SHA256': digest,
    }
    stored = json.loads(linux(dest, 'ack', str(PIPE), ['ack-native-partition'],
                              json.dumps(ack_document(result)).encode(), env))
    if stored.get('acknowledged') is not True:
        raise RuntimeError('ack_refused')
    return stored


def binding_env(request):
    registry = request['registry_path']
    return {'OO_NATIVE_ADMISSION_BIN': str(ADM), 'OO_NATIVE_BINDING_REGISTRY': registry,
            'OO_NATIVE_BINDING_REGISTRY_SHA256': sha256(registry)}


def catalog_request(request, result):
    catalog = {key: result.get(key, request.get(key)) for key in (
        'run_id', 'source_id', 'source_generation', 'target_generation', 'config_digest',
        'schema_digest', 'type_digest', 'input_digest', 'snapshot')}
    catalog.update(protocol='oo.pipeline.native-sealed-catalog/v1', owner=OWNER,
                   target={key: value for key, value in request['target'].items() if key not in {'shard', 'owner', 'ssl'}})
    return catalog


def next_generation(table_generation, watermark_generation):
    return max(table_generation, watermark_generation) + 1


def prior_binding(table):
    # Unchanged tables keep the committed generation. This run does not reload them.
    current = generation(table)
    root = BASE / 'whole-3' / 'import'
    for phase in ('c4', 'c4s', 'after', 'mid', 'before'):
        path = root / phase / table / 'catalog.json'
        if not path.is_file():
            continue
        body = json.loads(path.read_text())
        if body.get('source_generation') == current and body.get('state') == 'awaiting_publish':
            return body
    raise RuntimeError('prior_binding_missing')


def load_table(run, phase, tick_path, table, c4=False):
    tick = json.loads(Path(tick_path).read_text())
    spec = next(item for item in tick['tables'] if item['name'] == table)
    if c4 and spec.get('route') != 'C4':
        raise RuntimeError('c4_required')
    arrow_name = spec.get('arrow') or f'{tick.get("label", "current")}-{table}.arrow'
    arrow = Path(tick_path).parent / arrow_name
    if not arrow.exists():
        arrow = Path(tick_path).with_name(arrow_name)
    if not arrow.exists():
        raise RuntimeError('arrow_missing')
    current = generation(table)
    mark = next(item['generation'] for item in watermark() if item['source'] == 'dorispreview')
    target = next_generation(current, mark)
    dest = run / 'import' / phase / table
    done = dest / 'catalog.json'
    if done.exists():
        return json.loads(done.read_text())
    # Append-only mappings: a load past the bound generation cannot be deleted.
    if not official_current_pack():
        raise RuntimeError('current_generation_pack_missing')
    dest.mkdir(parents=True, exist_ok=True)
    prior = dest / 'load.json'
    if prior.exists():
        result = json.loads(prior.read_text())
        if result.get('load_state') in {'awaiting_publish', 'replay'}:
            request = json.loads((dest / 'request.json').read_text())
            if c4:
                ack_native(dest, result)
            stored = json.loads(linux(dest, 'catalog-resume', str(PIPE), ['catalog-import-native'],
                                      json.dumps(catalog_request(request, result)).encode(), binding_env(request)))
            write(done, stored)
            return stored
    label = tick.get('label') or Path(tick_path).stem
    scope = None
    capturer = Path(tick_path).resolve()
    if c4:
        scope = dict(tick['partition_scope'])
        scope.update({'source_id': 'dorispreview', 'source_generation': target,
                      'logical_schema': 'oo_dl01w1_43', 'logical_table': table,
                      'schema_digest': sha256(SCHEMA), 'type_digest': sha256(TYPE),
                      'arrow_sha256': sha256(arrow)})
        capturer = dest / 'capturer.json'
        body = dict(tick)
        body.update({'source_id': 'dorispreview', 'source_generation': target,
                     'logical_schema': 'oo_dl01w1_43', 'logical_table': table,
                     'partition_scope': scope, 'reader_capture_sha256': sha256(tick_path)})
        write(capturer, body)
    witness = {
        'protocol': 'oo.source-admission.capture-witness/v1', 'source_engine': 'doris',
        'source_id': 'dorispreview', 'source_generation': target,
        'snapshot': {'kind': 'capture-tick', 'value': label},
        'run_id': f'dorispreview-{label}-{table}-{tick.get("finished_unix_ms", 0)}',
        'logical_schema': 'oo_dl01w1_43', 'logical_table': table,
        'capturer_path': str(capturer), 'arrow_path': str(arrow.resolve()),
        'driver_path': str(READER), 'schema_preimage_path': str(SCHEMA),
        'type_preimage_path': str(TYPE)}
    write(dest / 'witness.json', witness)
    report = {
        'protocol': 'oo.source-admission.report/v1', 'target_id': 'dorispreview',
        'source_id': 'dorispreview', 'secret_ref': EXPECTED['secret_ref'],
        'driver': {'kind': 'native', 'name': 'doris-reader', 'version': '0.1.0', 'digest': READER_SHA},
        'compat_mode': 'doris', 'license_flags': ['Apache-2.0'],
        'permissions': [{'name': 'SELECT', 'status': 'known', 'granted': True}],
        'capabilities': [{'name': 'BatchFull', 'cursor': None, 'feed': None, 'cdc': None,
                          'token_order': None, 'budget_unit': None, 'level': 'experimental',
                          'write_class': 'R0', 'consistency': 'S0'}],
        'capture_methods': [], 'r1w': None,
        'cost_model': {'unit': 'Rows', 'confidence': 'unknown',
                       'estimate': 'Doris billing unpublished; configured source row cap'},
        'capture_witness': str((dest / 'witness.json').resolve())}
    write(dest / 'report.json', report)
    sealed = dest / 'sealed'
    if not (sealed / 'admission-receipt.json').exists():
        linux(dest, 'seal', str(ADM), ['seal-native', '--report', str((dest / 'report.json').resolve()),
                                       '--config', str(ADMISSION.resolve()), '--out', str(sealed.resolve())])
    receipt = json.loads((sealed / 'admission-receipt.json').read_text())
    if receipt.get('source_bound') is not True or receipt.get('write_attempts') != 0:
        raise RuntimeError('seal_unbound')
    registry = str((sealed / 'registry.json').resolve())
    info = TABLES[table]
    request = {
        'protocol': 'oo.pipeline.native-partition/v1' if c4 else 'oo.pipeline.native-update/v1',
        'source_engine': 'doris', 'source_id': 'dorispreview', 'source_generation': target,
        'parent_source_generation': parent_for(target),
        'snapshot': witness['snapshot'], 'logical_schema': 'oo_dl01w1_43', 'logical_table': table,
        'batch_id': f'{phase}-{table}-g{target}',
        'capture_method': 'c4_partition' if c4 else 'c5_diff',
        'delete_detection': spec['delete_mode'], 'consistency_grade': spec['consistency'],
        'cross_table_consistent': False, 'full_snapshot': not c4, 'mirror': not c4,
        'row_identity': info['identity'], 'key_columns': ['day'] if c4 else info['keys'],
        'position_codec': 'partition-signature/v1' if c4 else 'snapshot-zero/v1',
        'position_lineage': ('doris.partition.' if c4 else 'doris.snapshot.') + table,
        'applied_pos': '0', 'schema_digest': receipt['schema_digest'],
        'type_digest': receipt['type_digest'], 'config_digest': receipt['config_digest'],
        'input_digest': sha256(arrow), 'audit_digest': sha256(capturer),
        'arrow_path': str(arrow.resolve()), 'admission_path': str((sealed / 'admission-receipt.json').resolve()),
        'registry_path': registry, 'columns': [column_decl(item) for item in info['columns']],
        'tombstones': [],
        'target': {'shard': 'dorispreview', 'owner': OWNER, 'host': PG, 'port': 5432,
                   'database': 'dorisconnection2', 'runtime_principal': 'oo_doris_loader',
                   'password_file': str(BASE / 'loader.password'),
                   'ca_file': str(BASE / 'tls/ca.crt'), 'ssl': 'verify-full'}}
    task, lease_generation = claim(table, target, sha256(arrow))
    request['lease_id'] = task
    request['lease_generation'] = lease_generation
    write(dest / 'request.json', request)
    stop = threading.Event()
    thread = threading.Thread(target=beat, args=(task, lease_generation, stop), daemon=True)
    thread.start()
    sealed_job = False
    try:
        env = binding_env(request)
        raw = linux(dest, 'load', str(PIPE), ['import-native-update'], json.dumps(request).encode(), env)
        result = json.loads(raw)
        write(dest / 'load.json', result)
        if result.get('load_state') not in {'awaiting_publish', 'replay'}:
            raise RuntimeError('load_' + str(result.get('reason')))
        finish_task(task, lease_generation, True)
        sealed_job = True
        if c4:
            ack_native(dest, result)
        stored = json.loads(linux(dest, 'catalog', str(PIPE), ['catalog-import-native'],
                                  json.dumps(catalog_request(request, result)).encode(), env))
        write(done, stored)
        return stored
    except Exception:
        if not sealed_job:
            finish_task(task, lease_generation, False)
        raise
    finally:
        stop.set()


def acknowledged(partition):
    text = one(psql(
        "SELECT acknowledged FROM _oo.native_partition_state "
        "WHERE source='dorispreview' AND schema_name='oo_dl01w1_43' "
        "AND table_name='part_day' AND partition_id=" + "'" + partition + "'"), 'prior_ack')
    return text == 't'


def finish_c4(run, cfg):
    # A new partition changes table_epoch. The installed guard rejects that while
    # p202603 and p202604 stay acknowledged, so this cycle adds a row inside p202604.
    grant_ack_execute()
    for partition in ('p202603', 'p202604'):
        if not acknowledged(partition):
            raise RuntimeError('prior_partition_unacked')
    tick = run / 'c4' / 'evidence' / 'part.json'
    if not tick.exists():
        tick = capture_partition(cfg, run / 'c4', 'p202604')
    body = json.loads(tick.read_text())
    signature = body.get('partition_scope', {}).get('current_signature')
    stored_sig = one(psql(
        "SELECT current_signature FROM _oo.native_partition_state "
        "WHERE source='dorispreview' AND schema_name='oo_dl01w1_43' "
        "AND table_name='part_day' AND partition_id='p202604'"), 'prior_sig')
    catalog = run / 'import' / 'c4' / 'part_day' / 'catalog.json'
    if signature == stored_sig:
        if catalog.is_file() or (catalog.parent / 'load.json').is_file():
            return load_table(run, 'c4', tick, 'part_day', True)
        raise RuntimeError('c4_replay')
    stored = load_table(run, 'c4', tick, 'part_day', True)
    write(catalog, stored)
    return stored


def mutate_present():
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=16 AND kind='conn_whole'") == 0:
        one(mysql("INSERT INTO dup_events VALUES (16,'conn_whole',1.10)"), 'dup_insert')
        one(mysql("INSERT INTO dup_events VALUES (16,'conn_whole',1.10)"), 'dup_insert_2')
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=16 AND kind='conn_whole'") != 2:
        raise RuntimeError('dup_marker')
    one(mysql("UPDATE uniq_acct SET label='conn_whole' WHERE id=1 AND (label IS NULL OR label<>'conn_whole')"), 'uniq_update')
    if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-22' AND region='west'") == 0:
        one(mysql("INSERT INTO part_day VALUES ('2026-04-22','west',8.80)"), 'part_insert')
    # 9001, 9002, and 9003 end absent. Absence is the sealed state, not a new insert.
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=17 AND kind='maintain'") == 0:
        one(mysql("INSERT INTO dup_events VALUES (17,'maintain',2.20)"), 'maintain_insert')
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=17 AND kind='maintain'") != 1:
        raise RuntimeError('maintain_marker')
    one(mysql("UPDATE uniq_acct SET label='maintain' WHERE id=2 AND (label IS NULL OR label<>'maintain')"), 'maintain_update')
    if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-23' AND region='maintain'") == 0:
        one(mysql("INSERT INTO part_day VALUES ('2026-04-23','maintain',6.60)"), 'part_maintain')
    # A failed seal already applied the maintain rows. This wave is the next real delta.
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=18 AND kind='cycle5'") == 0:
        one(mysql("INSERT INTO dup_events VALUES (18,'cycle5',3.30)"), 'cycle5_insert')
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=18 AND kind='cycle5'") != 1:
        raise RuntimeError('cycle5_marker')
    one(mysql("UPDATE uniq_acct SET label='cycle5' WHERE id=3 AND (label IS NULL OR label<>'cycle5')"), 'cycle5_update')
    if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-24' AND region='cycle5'") == 0:
        one(mysql("INSERT INTO part_day VALUES ('2026-04-24','cycle5',7.70)"), 'cycle5_part')


def mutate_delete():
    for nz_id, label in ((9002, 'nz_maintain_delete'), (9003, 'cycle5_nz_delete')):
        if count(f'SELECT COUNT(*) FROM nz_obs WHERE id={nz_id}') != 0:
            one(mysql(f'DELETE FROM nz_obs WHERE id={nz_id}'), label)
        if count(f'SELECT COUNT(*) FROM nz_obs WHERE id={nz_id}') != 0:
            raise RuntimeError('nz_remains')
    if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-18' AND region='south'") != 0:
        raise RuntimeError('part_remains')


C5_CHANGED = ('dup_events', 'uniq_acct', 'nz_obs')


def tick_arrow(tick_path, table):
    tick = json.loads(Path(tick_path).read_text())
    spec = next(item for item in tick['tables'] if item['name'] == table)
    arrow_name = spec.get('arrow') or f'{tick.get("label", "current")}-{table}.arrow'
    arrow = Path(tick_path).parent / arrow_name
    if not arrow.exists():
        arrow = Path(tick_path).with_name(arrow_name)
    return sha256(arrow)


def import_diverged(run, phase, left, right, tables):
    catalogs = {}
    for table in tables:
        if tick_arrow(left, table) == tick_arrow(right, table):
            continue
        catalogs[table] = load_table(run, phase, right, table, False)
    return catalogs


def catalog_for(run, table):
    for phase in ('c4', 'after', 'mid'):
        path = run / 'import' / phase / table / 'catalog.json'
        if path.is_file():
            body = json.loads(path.read_text())
            if body.get('state') == 'awaiting_publish':
                return body
    return prior_binding(table)


def gold(run, capture_tick, catalogs):
    out = run / 'gold.json'
    if out.exists():
        body = json.loads(out.read_text())
        if body.get('all_equal') is not True:
            raise RuntimeError('typed_gold')
        return body
    entries = list(catalogs.values())
    spec = importlib.util.spec_from_file_location('collection', ROOT / 'tools/pg-builder/scripts/export_native_collection.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    body = module.collection(entries, [('oo_dl01w1_43', name) for name in TABLES])
    write(run / 'collection.json', body)
    proc = subprocess.run(
        [sys.executable, str(COMPARE), '--collection', str(run / 'collection.json'),
         '--capture', str(capture_tick), '--database', 'dorisconnection2',
         '--container', PG, '--user', 'postgres', '--out', str(out)],
        capture_output=True, timeout=120)
    kept(proc, run / 'gold-call')
    if proc.returncode or not out.exists():
        raise RuntimeError('typed_gold')
    return json.loads(out.read_text())


def cancel_accepted(text):
    # doris-cancel-once stays terminal. A later probe uses its own dedupe.
    # Retry of that probe returns terminal|cancelled and writes nothing.
    return text.startswith('cancelled|') or text == 'terminal|cancelled'


def recovery(run):
    path = run / 'recovery.json'
    if path.exists():
        return json.loads(path.read_text())
    before = watermark()
    queued = one(psql(
        "SELECT status||'|'||task_id::text||'|'||generation::text FROM jobs.enqueue("
        f"'{OWNER}'::uuid, 'native_update', 1, '{{}}', 'doris-conn', '{READER_SHA}', "
        "'doris-cancel-maintain', clock_timestamp(), 3)"), 'cancel_enqueue')
    status, task, gen = queued.split('|')
    if status not in {'enqueued', 'replayed'}:
        raise RuntimeError('cancel_enqueue')
    cancelled = one(psql(
        "SELECT status||'|'||state FROM jobs.cancel("
        f"'{OWNER}'::uuid, '{task}'::uuid, '{WORKER}', {gen})"), 'cancel')
    after_cancel = watermark()
    bad = {'protocol': 'oo.pipeline.native-update/v1', 'source_engine': 'doris',
           'source_id': 'dorispreview', 'source_generation': 1, 'registry_path': '/tmp/not-the-registry.json'}
    proc = subprocess.run(
        ['docker', 'run', '--rm', '-i', '--network', 'oo-dl01w1-43-ns2', '--platform', 'linux/arm64',
         '--pull', 'never', '-v', '/tmp/oo-pg46-runtime-finish:/tmp/oo-pg46-runtime-finish:ro',
         '-e', 'OO_NATIVE_ADMISSION_BIN=' + str(ADM),
         '-e', 'OO_NATIVE_BINDING_REGISTRY=/tmp/not-the-registry.json',
         '-e', 'OO_NATIVE_BINDING_REGISTRY_SHA256=' + ('0' * 64),
         '--entrypoint', str(PIPE), RUST, 'import-native-update'],
        input=json.dumps(bad).encode(), capture_output=True, timeout=60)
    kept(proc, run / 'recovery-load')
    refused = {}
    if proc.stdout:
        try:
            refused = json.loads(proc.stdout)
        except json.JSONDecodeError:
            refused = {}
    after = watermark()
    body = {'protocol': 'oo.doris.recovery/v1', 'cancel_status': cancelled,
            'load_reason': refused.get('reason'), 'load_state': refused.get('load_state'),
            'loader_invoked': refused.get('loader_invoked'),
            'watermark_before': before, 'watermark_after_cancel': after_cancel,
            'watermark_after_refusal': after,
            'recovery_verified': cancel_accepted(cancelled) and after == before
            and refused.get('loader_invoked') is False and refused.get('load_state') == 'not_sealed'}
    write(path, body)
    if body['recovery_verified'] is not True:
        raise RuntimeError('recovery_unverified')
    return body


def product_chain(run):
    config = BASE / 'product.json'
    decisions = BASE / 'decisions.json'
    dialogue_config = BASE / 'dialogue.json'
    if not (config.exists() and decisions.exists() and dialogue_config.exists()):
        write(run / 'product-gap.json', {'product_configured': False})
        raise RuntimeError('product_not_configured')
    product = run / 'product'
    product.mkdir(mode=0o700, exist_ok=True)
    if not (product / 'product-receipt.json').exists():
        if not (product / 'prepared.json').exists():
            proc = subprocess.run([sys.executable, str(PRODUCT_FLOW), '--config', str(config),
                                   '--out', str(product), '--prepare-only'], capture_output=True, timeout=1800)
            kept(proc, run / 'product-prepare')
            if proc.returncode:
                raise RuntimeError('product_prepare_failed')
        proc = subprocess.run([sys.executable, str(PRODUCT_FLOW), '--config', str(config),
                               '--out', str(product), '--decisions', str(decisions)],
                              capture_output=True, timeout=1800)
        kept(proc, run / 'product-decide')
        if proc.returncode:
            raise RuntimeError('product_flow_failed')
    dialogue = run / 'dialogue'
    if not (dialogue / 'verdict.json').exists():
        if dialogue.exists():
            raise RuntimeError('dialogue_failure_retained')
        proc = subprocess.run([sys.executable, str(DIALOGUE), '--config', str(dialogue_config),
                               '--out', str(dialogue)], capture_output=True, timeout=900)
        kept(proc, run / 'dialogue-call')
        if proc.returncode:
            raise RuntimeError('dialogue_failed')
    return (json.loads((product / 'product-receipt.json').read_text()),
            json.loads((dialogue / 'verdict.json').read_text()))


def capture_scheduled(cfg, evidence, label):
    tick_path = evidence / f'{label}.json'
    if tick_path.exists():
        return json.loads(tick_path.read_text())
    evidence.mkdir(mode=0o700, exist_ok=True)
    command = ['docker', 'run', '--rm', '--network', cfg['network'], '--pull', 'never',
               '--platform', 'linux/arm64', '--memory', '512m',
               '-v', f'{cfg["reader"]}:/opt/doris-reader:ro', '-v', f'{evidence}:/evidence',
               '-v', f'{cfg["password_file"]}:/secrets/reader.password:ro',
               '--entrypoint', '/opt/doris-reader', RUST,
               'capture-connection', '/evidence', '/secrets', cfg['host'], str(cfg['port']),
               cfg['database'], cfg['user'], str(cfg['period_ms']), label,
               str(cfg['max_rows_per_table'])]
    proc = subprocess.run(command, capture_output=True, timeout=240)
    kept(proc, evidence / f'call-{label}')
    if proc.returncode or not tick_path.exists():
        raise RuntimeError('period_capture_refused')
    body = json.loads(tick_path.read_text())
    if body.get('every_ms') != cfg['period_ms'] or body.get('product_reader_write_attempts') != 0:
        raise RuntimeError('period_capture_refused')
    return body


def wait_period(cfg, evidence):
    state = evidence / f'schedule-{cfg["period_ms"]}.json'
    if not state.exists():
        raise RuntimeError('period_schedule_missing')
    previous = int(json.loads(state.read_text())['finished_unix_ms'])
    delay_ms = previous + int(cfg['period_ms']) - int(time.time() * 1000)
    if delay_ms > 0:
        time.sleep(delay_ms / 1000 + 0.05)


def cycle1_targets(later_present, label):
    # Cycle 7 owns 9101 and the uniq label. Reapplying cycle 1 must not undo it.
    owned = later_present or label == 'cycle7'
    return {'nz_9101': not owned, 'label_cycle6': not owned}


def apply_cycle(index):
    # New fixture rows only. Historical markers 16/17/18 and regions east/north stay.
    if index == 1:
        later = count("SELECT COUNT(*) FROM dup_events WHERE event_id=20 AND kind='cycle7'") != 0
        label = one(mysql("SELECT label FROM uniq_acct WHERE id=5"), 'uniq5')
        target = cycle1_targets(later, label)
        if count("SELECT COUNT(*) FROM dup_events WHERE event_id=19 AND kind='cycle6'") == 0:
            one(mysql("INSERT INTO dup_events VALUES (19,'cycle6',4.40)"), 'cycle6_insert')
        if target['nz_9101'] and count("SELECT COUNT(*) FROM nz_obs WHERE id=9101") == 0:
            one(mysql("INSERT INTO nz_obs VALUES (9101,2.00)"), 'cycle6_nz')
        if target['label_cycle6']:
            one(mysql("UPDATE uniq_acct SET label='cycle6' WHERE id=5 AND label<>'cycle6'"), 'cycle6_update')
        if count("SELECT COUNT(*) FROM nz_obs WHERE id=9001") != 0:
            one(mysql("DELETE FROM nz_obs WHERE id=9001"), 'cycle6_delete')
        if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-25' AND region='cycle6'") == 0:
            one(mysql("INSERT INTO part_day VALUES ('2026-04-25','cycle6',8.80)"), 'cycle6_part')
        if count("SELECT COUNT(*) FROM nz_obs WHERE id=9001") != 0:
            raise RuntimeError('cycle6_nz_remains')
        return
    if count("SELECT COUNT(*) FROM dup_events WHERE event_id=20 AND kind='cycle7'") == 0:
        one(mysql("INSERT INTO dup_events VALUES (20,'cycle7',5.50)"), 'cycle7_insert')
    one(mysql("UPDATE uniq_acct SET label='cycle7' WHERE id=5 AND label<>'cycle7'"), 'cycle7_update')
    if count("SELECT COUNT(*) FROM nz_obs WHERE id=9101") != 0:
        one(mysql("DELETE FROM nz_obs WHERE id=9101"), 'cycle7_delete')
    if count("SELECT COUNT(*) FROM part_day WHERE day='2026-04-26' AND region='cycle7'") == 0:
        one(mysql("INSERT INTO part_day VALUES ('2026-04-26','cycle7',9.90)"), 'cycle7_part')
    if count("SELECT COUNT(*) FROM nz_obs WHERE id=9101") != 0:
        raise RuntimeError('cycle7_nz_remains')


def cycle_changes(before, after):
    counts = {'insert': 0, 'update': 0, 'delete': 0}
    left = {item['name']: item['rows'] for item in before['tables']}
    right = {item['name']: item['rows'] for item in after['tables']}
    for name in C5_CHANGED:
        if TABLES[name]['identity'] == 'unique':
            previous = {_receipt.key_text(name, row): row for row in left[name]}
            current = {_receipt.key_text(name, row): row for row in right[name]}
            counts['delete'] += len(previous.keys() - current.keys())
            counts['insert'] += len(current.keys() - previous.keys())
            counts['update'] += sum(1 for key in previous.keys() & current.keys() if previous[key] != current[key])
            continue
        removed = list(left[name])
        for row in right[name]:
            if row in removed:
                removed.remove(row)
            else:
                counts['insert'] += 1
        counts['delete'] += len(removed)
    return counts


def run_period(run, cfg):
    evidence = run / 'period-evidence'
    ticks = [capture_scheduled(cfg, evidence, 'period0')]
    cycles = []
    for index, label in ((1, 'period1'), (2, 'period2')):
        if not (evidence / f'{label}.json').exists():
            apply_cycle(index)
            wait_period(cfg, evidence)
        tick = capture_scheduled(cfg, evidence, label)
        previous = tick.get('previous_finished_unix_ms')
        if previous is None:
            raise RuntimeError('period_gap')
        gap = int(tick['finished_unix_ms']) - int(previous)
        if gap < int(cfg['period_ms']):
            raise RuntimeError('period_gap')
        change = cycle_changes(ticks[-1], tick)
        if min(change.values()) < 1:
            raise RuntimeError('period_cycle')
        cycles.append({'label': label, 'gap_ms': gap, **change,
                       'finished_unix_ms': tick['finished_unix_ms']})
        ticks.append(tick)
    return {'protocol': 'oo.doris.period/v1', 'period_ms': cfg['period_ms'],
            'cycles': cycles, 'cycles_verified': True}


def live_collection():
    binding = json.loads((BASE / 'gateway/reviewed/dorispreview/binding.json').read_text())
    return binding['catalog_version']


def archived_pack_catalogs():
    found = []
    root = BASE / 'api-run/work/builds'
    for pack in root.glob('b_*/native-maintenance/*/role-pack.json'):
        body = json.loads(pack.read_text())
        manifest = next(item['text'] for item in body['files'] if item['path'] == 'manifest.json')
        found.append(json.loads(manifest)['catalog_version'])
    return found


def pack_follows(live, archived):
    return live in archived


def official_current_pack():
    # The serving binding catalog must be a compiler-archived role pack.
    return pack_follows(live_collection(), set(archived_pack_catalogs()))


API_CONTAINER = 'oo-doris-connection-whole-api'
API_SHA = '5bc832f4a1bb9ada23a6f2a2691bf7d784d09977aa770f7a944c3c9d123be88d'
COLLECTION = '12cfb998ab05aef8cd92e55106b4c251240ded5a3c0d52eaece4c31fe1fad54c'
# ponytail: reuse is pinned to the finished 19/31 cycles. A later legal advance needs a new retained proof; this path does not replay apply_cycle.
SERVED = {
    'part_day': (16, 28), 'dup_events': (17, 29), 'uniq_acct': (18, 30), 'nz_obs': (19, 31),
    'agg_qty': (1, 2), 'dec_exact': (1, 4), 'str_key': (1, 6),
}
NOT_RUN = (
    'mutate_present', 'mutate_delete', 'apply_cycle', 'import_diverged', 'finish_c4',
    'grant_ack_execute', 'product_chain', 'capture_all', 'capture_partition',
)
RETAINED = (
    'cycles-aa427/period0.json', 'cycles-aa427/period1.json', 'cycles-aa427/period2.json',
    'random-dialogue/gold.json', 'random-dialogue/bank.json', 'random-dialogue/history.json',
    'random-dialogue/run-1/selection.json', 'random-dialogue/run-1/transcript.json', 'random-dialogue/run-1/verdict.json',
    'random-dialogue/run-2/selection.json', 'random-dialogue/run-2/transcript.json', 'random-dialogue/run-2/verdict.json',
    'random-dialogue/run-a/selection.json', 'random-dialogue/run-a/transcript.json',
    'cancel-aa427/recovery.json',
    'current-unloaded-aa427/import/c4/part_day/load.json',
    'cycles-aa427/import/period2/dup_events/load.json',
    'cycles-aa427/import/period2/uniq_acct/load.json',
    'cycles-aa427/import/period2/nz_obs/load.json',
    'filter-aa427/uniq_id5.stdout', 'filter-aa427/uniq_id5.exit',
    'filter-aa427/dup_event22.stdout', 'filter-aa427/dup_event22.exit',
    'filter-aa427/part_20260426.stdout', 'filter-aa427/part_20260426.exit',
    'filter-aa427/part_region_cycle7.stdout', 'filter-aa427/part_region_cycle7.exit',
    'filter-aa427/dec_id3.stdout', 'filter-aa427/dec_id3.exit',
    'filter-aa427/nz_9301.stdout', 'filter-aa427/nz_9301.exit',
    'gateway/packs/dorispreview.json', 'api-run/bundle/catalog.json',
    'api-run/bundle/profile.json', 'api-run/bundle/source_semantics.json', 'api-run/bundle/manifest.json',
)
TOKEN_FILES = (
    'gateway/client.token', 'gateway/client.token.expired', 'gateway/client.token.g12',
    'gateway/client.token.prev-e875', 'gateway/client.token.aa427-read',
)
CANCEL_QID = 'q_Jefy6KjLM1pPyl6DeDfxGMBdNW'
ABSENT_WAL = ('q_869YLE0RUHGZYVHJxdzsq3lKgC', 'q_lohn0ZGv75Eo0dkkDm5QQSjThs')
DIALOGUE_AT_RUNS = 'bc1e7489c60d96a8e1e8b331bb8341ae130de8f25a6daa3656afcae7633ad5ad'


def entry_refusal(claim_retained, pins_match):
    if claim_retained:
        return 'whole_claim_retained'
    if not pins_match:
        return 'installed_recipe_pins_stale'
    return 'prove_whole_unsatisfied'


def file_meta(path):
    path = Path(path)
    if not path.is_file() or path.is_symlink():
        raise RuntimeError('retained_stage_missing')
    mode = path.stat().st_mode
    return {'path': str(path), 'sha256': sha256(path), 'mode': oct(mode & 0o777), 'size': path.stat().st_size, 'uid': path.stat().st_uid}


def load_facts(path):
    body = json.loads(Path(path).read_text())
    receipt = body.get('partition_target_receipt')
    scope = receipt.get('partition_scope') if isinstance(receipt, dict) else None
    return {
        'logical_table': body.get('logical_table'),
        'source_generation': body.get('source_generation'),
        'target_generation': body.get('target_generation'),
        'loader_invoked': body.get('loader_invoked') is True,
        'load_state': body.get('load_state'),
        'row_count': body.get('row_count'),
        'partition_id': scope.get('partition_id') if isinstance(scope, dict) else None,
        'partition_receipt': isinstance(receipt, dict),
    }


def wal_for(qid):
    found = {}
    for kind in ('admit', 'cancel', 'complete'):
        path = BASE / 'gateway/wal' / f'{qid}-{kind}.wal'
        found[kind] = file_meta(path) if path.is_file() and not path.is_symlink() else None
    return found


def installed_card():
    registry = json.loads((Path.home() / '.cache/oo-source-onboarding/engines.json').read_text())
    row = registry['engines']['doris']
    recipe_path = Path(row['recipe'])
    recipe = json.loads(recipe_path.read_text())
    pins = []
    for pin in recipe['pins']:
        target = Path(pin['path'])
        live = sha256(target) if target.is_file() and not target.is_symlink() else None
        pins.append({'path': pin['path'], 'expected_sha256': pin['sha256'], 'live_sha256': live, 'match': live == pin['sha256']})
    return {
        'recipe': str(recipe_path),
        'recipe_sha256': sha256(recipe_path),
        'registry_recipe_sha256': row.get('recipe_sha256'),
        'registry_whole': row.get('whole') is True,
        'registry_whole_proof_sha256': row.get('whole_proof_sha256'),
        'pins': pins,
    }


def execute(config_path, run):
    """Attach this entry to the serving pack and the finished stages.

    whole.json is not written. Old DML, import, GRANT, and a new product stay off.
    """
    if SOCK.exists() and not os.environ.get('DOCKER_HOST'):
        os.environ['DOCKER_HOST'] = 'unix://' + str(SOCK)
    cfg = json.loads(Path(config_path).read_text())
    require_connection(cfg)
    run = Path(run)
    claim = (run / 'whole.json').is_file()
    mismatches = []
    source_s, target_s, state = one(psql(
        "SELECT source_generation::text || ' ' || target_generation::text || ' ' || state "
        "FROM _oo.source_watermark WHERE source='dorispreview'"), 'watermark').split()
    source_generation, target_generation = int(source_s), int(target_s)
    label = one(mysql("SELECT label FROM uniq_acct WHERE id=5"), 'uniq5')
    if (source_generation, target_generation, state, label) != (19, 31, 'lagging', 'aa427c2'):
        mismatches.append('served_identity')
    if 'aa427c2' not in (BASE / 'cycles-aa427/period2.json').read_text():
        mismatches.append('retained_period')
    catalog = json.loads((BASE / 'api-run/bundle/catalog.json').read_text())
    tables, physical = [], []
    for entry in catalog['entries']:
        table = entry['tables'][0]
        name = table['logical_table']
        stored = table['stored_physical']
        if not isinstance(stored, str) or not stored.startswith('pt_') or any(c not in '0123456789abcdef' for c in stored[3:]):
            raise RuntimeError('physical_name')
        physical.append(stored)
        tables.append({
            'logical_table': name, 'source_generation': entry['source_generation'],
            'target_generation': entry['target_generation'], 'run_id': entry['run_id'],
            'input_digest': entry['input_digest'], 'stored_physical': stored, 'state': entry['state'],
        })
        if SERVED.get(name) != (entry['source_generation'], entry['target_generation']) or entry['state'] != 'awaiting_publish':
            mismatches.append('table:' + str(name))
    if [item['logical_table'] for item in tables] != list(SERVED):
        mismatches.append('table_order')
    if catalog.get('collection_sha256') != COLLECTION:
        mismatches.append('collection')
    profile = json.loads((BASE / 'api-run/bundle/profile.json').read_text())
    if profile.get('generation_semantics') != 'high_watermark' or (profile.get('source_generation'), profile.get('target_generation')) != (19, 31):
        mismatches.append('profile_watermark')
    for name, pair in SERVED.items():
        row = profile['table_generations']['oo_dl01w1_43.' + name]
        if (row.get('source_generation'), row.get('target_generation')) != pair:
            mismatches.append('profile_table:' + name)
    semantics = json.loads((BASE / 'api-run/bundle/source_semantics.json').read_text())
    if (semantics.get('source_generation'), semantics.get('target_generation')) != (19, 31):
        mismatches.append('semantics_watermark')
    if semantics.get('reader_write_attempts') != 0 or semantics.get('source_full_typed_cell_diff') != 0:
        mismatches.append('semantics_reads')
    binding = json.loads((BASE / 'gateway/reviewed/dorispreview/binding.json').read_text())
    if binding.get('generation') != 19 or binding.get('catalog_version') != COLLECTION or binding.get('role') != 'dorispreview':
        mismatches.append('binding')
    if not official_current_pack():
        mismatches.append('official_current_pack')
    api_sha = one(subprocess.run(
        ['docker', 'exec', API_CONTAINER, 'sha256sum', '/api'], capture_output=True, timeout=30), 'api').split()[0]
    if api_sha != API_SHA:
        mismatches.append('api_elf')
    missing = one(psql(
        "SELECT COALESCE(string_agg(c.relname || '|' || a.attname, ',' ORDER BY c.relname, a.attname), '') "
        "FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
        "JOIN pg_attribute a ON a.attrelid = c.oid "
        "WHERE n.nspname = '_oo' AND c.relname IN (" + ','.join("'" + name + "'" for name in physical) + ") "
        "AND a.attnum > 0 AND NOT a.attisdropped "
        "AND NOT has_column_privilege('oo_r_dorispreview', c.oid, a.attnum, 'SELECT')"), 'column_acl')
    observed_missing = sorted(part for part in missing.split(',') if part)
    if observed_missing != sorted(name + '|_oo_pos' for name in physical):
        mismatches.append('column_acl')
    loads = {
        'c4_part_day': load_facts(BASE / 'current-unloaded-aa427/import/c4/part_day/load.json'),
        'c5_dup_events': load_facts(BASE / 'cycles-aa427/import/period2/dup_events/load.json'),
        'c5_uniq_acct': load_facts(BASE / 'cycles-aa427/import/period2/uniq_acct/load.json'),
        'c5_nz_obs': load_facts(BASE / 'cycles-aa427/import/period2/nz_obs/load.json'),
    }
    part = loads['c4_part_day']
    if not (part['partition_receipt'] and part['partition_id'] == 'p202604' and part['loader_invoked']
            and (part['source_generation'], part['target_generation']) == (16, 28)):
        mismatches.append('c4_part_day')
    for key, expected in (('c5_dup_events', (17, 29, 16)), ('c5_uniq_acct', (18, 30, 4)), ('c5_nz_obs', (19, 31, 0))):
        row = loads[key]
        if row['partition_receipt'] or not row['loader_invoked'] or (row['source_generation'], row['target_generation'], row['row_count']) != expected:
            mismatches.append(key)
    verdicts = {}
    gold_sha = sha256(BASE / 'random-dialogue/gold.json')
    for name in ('run-1', 'run-2'):
        body = json.loads((BASE / 'random-dialogue' / name / 'verdict.json').read_text())
        verdicts[name] = {key: body.get(key) for key in (
            'outcome', 'turns', 'counts_as_random', 'selection', 'model', 'semantic_ids', 'query_ids', 'gold_sha256', 'seed')}
        tools = set(body.get('tools') or [])
        if not (body.get('outcome') == 'verified' and type(body.get('turns')) is int and body['turns'] >= 2
                and body.get('counts_as_random') is True and body.get('selection') == 'random'
                and body.get('model') == 'grok-4.7' and body.get('reasoning_effort') == 'low'
                and {'query', 'freshness', 'describe', 'knowledge_cat'} <= tools and body.get('gold_sha256') == gold_sha):
            mismatches.append('verdict:' + name)
    ids = [verdicts[name].get('semantic_ids') for name in ('run-1', 'run-2')]
    if not all(isinstance(item, list) for item in ids) or set(ids[0]) & set(ids[1]):
        mismatches.append('dialogue_overlap')
    evidence = [file_meta(BASE / rel) for rel in RETAINED]
    tokens = [file_meta(BASE / rel) for rel in TOKEN_FILES]
    qids = [CANCEL_QID, *ABSENT_WAL]
    for name in ('run-1', 'run-2'):
        qids.extend(verdicts[name].get('query_ids') or [])
    filters = []
    for rel in RETAINED:
        if not (rel.startswith('filter-aa427/') and rel.endswith('.stdout')):
            continue
        payload = json.loads((BASE / rel).read_text())
        qid = payload.get('query_id')
        if isinstance(qid, str):
            qids.append(qid)
        filters.append({
            'file': str(BASE / rel), 'exit': int((BASE / rel.replace('.stdout', '.exit')).read_text().strip()),
            'query_id': qid, 'code': payload.get('code'), 'rows': payload.get('rows'), 'columns': payload.get('columns'),
        })
    card = installed_card()
    pins_match = bool(card['pins']) and all(pin['match'] for pin in card['pins']) and card['recipe_sha256'] == card['registry_recipe_sha256']
    if card['recipe_sha256'] != card['registry_recipe_sha256']:
        default_refusal = 'installed_recipe_drift'
    elif not pins_match:
        default_refusal = 'pin_drift'
    else:
        default_refusal = 'pins_current_prove_whole_unsatisfied'
    pointer = json.loads((BASE / 'six-field.json.run.json').read_text())
    pointer_out = Path(pointer['out'])
    receipt_path = pointer_out / 'product/product-receipt.json'
    receipt = json.loads(receipt_path.read_text()) if receipt_path.is_file() else {}
    e875_whole = pointer_out / 'whole.json'
    grant = json.loads((BASE / 'gateway/grant.json').read_text())
    role = {key: grant.get(key) for key in ('role', 'kind', 'project_id', 'principal_id', 'policy_version', 'knowledge_version')}
    if role.get('role') != 'oo_r_dorispreview':
        mismatches.append('client_role')
    pid = subprocess.run(['ps', '-p', '31458', '-o', 'pid='], capture_output=True, text=True)
    refusal = 'whole_claim_retained' if claim else ('served_source_moved' if mismatches else entry_refusal(False, pins_match))
    callback = {
        'protocol': 'oo.doris.entry-callback/v1',
        'whole': False, 'published': False, 'formal_signoff': False, 'full_product_chain_complete': False,
        'customer_writes': 0, 'refusal': refusal, 'default_entry_refusal': default_refusal,
        'session': 'aa427924-9c4b-4a5e-ae15-9a2aef414d44', 'manuscript_owner': OWNER,
        'shared_registry_owner': '86947', 'dialogue_verifier_writer': '26ae6875-cfaf-457a-b566-ab548bdc6989',
        'source_generation': source_generation, 'target_generation': target_generation, 'state': state,
        'uniq_id5_label': label, 'generation_semantics': profile.get('generation_semantics'),
        'tables': tables, 'collection_sha256': catalog.get('collection_sha256'),
        'binding': {key: binding.get(key) for key in ('generation', 'catalog_version', 'schema_hash', 'role', 'source_id', 'login', 'ontology_version')},
        'client_role': role, 'column_acl_without_select': observed_missing,
        'semantics_capture_methods': semantics.get('capture_methods'),
        'c4_is_partition_load': part['partition_id'] == 'p202604',
        'c5_not_substituted_for_c4': part['partition_receipt'] and all(not loads[key]['partition_receipt'] for key in ('c5_dup_events', 'c5_uniq_acct', 'c5_nz_obs')),
        'c3': 'not_used', 'loads': loads, 'api_sha256': api_sha,
        'whole_py_sha256': sha256(Path(__file__)), 'product_flow_sha256': sha256(PRODUCT_FLOW),
        'dialogue_py_sha256': sha256(DIALOGUE), 'dialogue_py_sha256_at_random_runs': DIALOGUE_AT_RUNS,
        'pack_sha256': sha256(BASE / 'gateway/packs/dorispreview.json'),
        'verdicts': verdicts, 'filters': filters, 'evidence': evidence, 'tokens': tokens,
        'wal': {qid: wal_for(qid) for qid in qids}, 'recovery': file_meta(BASE / 'cancel-aa427/recovery.json'),
        'e875': {
            'attached': False, 'out': str(pointer_out), 'pointer_sha256': sha256(BASE / 'six-field.json.run.json'),
            'input_sha256': pointer.get('input_sha256'),
            'whole_sha256': sha256(e875_whole) if e875_whole.is_file() else None,
            'producer': receipt.get('producer'), 'producer_sha256': receipt.get('producer_sha256'),
            'source_fresh_on_receipt': receipt.get('source_fresh'), 'receipt_catalog_sha256': receipt.get('catalog_sha256'),
            'trust': 'original producer retained; generation 19 catalog is not that receipt',
        },
        'installed_card': card, 'mismatches': mismatches,
        'pid_31458_absent': pid.returncode != 0 and not pid.stdout.strip(),
        'not_run': list(NOT_RUN),
        'blocked_on': {
            'owner': '86947', 'path': 'tools/source-onboarding/onboard.py',
            'symbols': ['prove_whole', 'direct_step', 'resume_identity_ok'], 'commit': '53eb5c115',
            'tokens': ['pin_drift', 'source_not_fresh', 'historical_completion_attached', 'product_resume_drift'],
            'reason': 'The installed card pins whole.py, product_flow.py, and dialogue.py to older bytes, so the default six-field entry raises pin_drift before it creates a run directory. e875 keeps producer b20148b6. That receipt is source_fresh only for catalog ed7c4168. Attaching it to collection 12cfb998 would be source_not_fresh. Copying the receipt without launching product_flow and dialogue in this step is historical_completion_attached. A catalog from another build is product_resume_drift. The foreign product_flow pin is not restored.',
        },
        'next_call': 'register_engine by owner 86947 only after the pins match the committed bytes and the recipe can satisfy prove_whole. This entry does not write the registry and does not launch a new product.',
    }
    if cancel_wal_missing(callback):
        mismatches.append('cancel_wal')
        callback['mismatches'] = mismatches
        if not claim and not mismatches[:-1]:
            callback['refusal'] = 'served_source_moved'
            refusal = 'served_source_moved'
    if not claim:
        write(run / 'entry-callback.json', callback)
    write(BASE / 'entry-callback.json', callback)
    raise RuntimeError(refusal)


def cancel_wal_missing(callback):
    cancel = callback['wal'][CANCEL_QID]
    return cancel['admit'] is None or cancel['cancel'] is None or cancel['complete'] is None


def demo():
    assert next_generation(5, 5) == 6 and next_generation(3, 6) == 7
    assert cancel_accepted('cancelled|cancelled') and cancel_accepted('terminal|cancelled')
    assert not cancel_accepted('terminal|failed') and not cancel_accepted('lost_lease|queued')
    assert partition_name('p202604')
    assert not partition_name('2026-04')
    assert parent_for(1) is None and parent_for(2) == 1
    assert TABLES['part_day']['identity'] == 'duplicate'
    assert not f"dup_events-g1-{'9'*16}-{READER_SHA[:8]}p".endswith('b874654f')
    doc = ack_document({
        'source_id': 'dorispreview', 'logical_schema': 'oo_dl01w1_43', 'logical_table': 'part_day',
        'run_id': '91d19bcd-cad3-4803-ae1f-c4a3c84f21e9', 'payload_digest': 'd' * 64,
        'partition_target_receipt': {
            'batch_id': 'c4-part_day-g4', 'job_task_id': 'e8a25060-bc60-42f8-9dee-f33438bf450c',
            'job_generation': 1, 'partition_scope': {'partition_id': 'p202603'}}})
    assert set(doc) == {
        'protocol', 'source_id', 'logical_schema', 'logical_table', 'partition_id',
        'run_id', 'batch_id', 'payload_digest', 'job_task_id', 'job_generation'}
    assert doc['partition_id'] == 'p202603' and doc['job_generation'] == 1
    before = {'tables': [
        {'name': 'dup_events', 'rows': ['1\u001fa\u001f1']},
        {'name': 'uniq_acct', 'rows': ['5\u001f1\u001fold']},
        {'name': 'nz_obs', 'rows': ['9001\u001f1']},
    ]}
    after = {'tables': [
        {'name': 'dup_events', 'rows': ['1\u001fa\u001f1', '19\u001fcycle6\u001f4.40']},
        {'name': 'uniq_acct', 'rows': ['5\u001f1\u001fcycle6']},
        {'name': 'nz_obs', 'rows': []},
    ]}
    assert cycle_changes(before, after) == {'insert': 1, 'update': 1, 'delete': 1}
    assert pack_follows('aa', {'aa'}) and not pack_follows('aa', {'bb'})
    assert cycle1_targets(True, 'cycle7') == {'nz_9101': False, 'label_cycle6': False}
    assert cycle1_targets(False, 'cycle7') == {'nz_9101': False, 'label_cycle6': False}
    assert cycle1_targets(False, 'native_full') == {'nz_9101': True, 'label_cycle6': True}
    import inspect
    body = inspect.getsource(execute)
    for name in NOT_RUN:
        assert name + '(' not in body
    assert 'whole.json' in body and "write(run / 'whole.json'" not in body
    assert entry_refusal(True, False) == 'whole_claim_retained'
    assert entry_refusal(False, False) == 'installed_recipe_pins_stale'
    assert entry_refusal(False, True) == 'prove_whole_unsatisfied'


def main():
    if len(sys.argv) == 2 and sys.argv[1] == '--self-check':
        demo()
        print('ok')
        return 0
    if len(sys.argv) != 3:
        print('doris-whole: usage', file=sys.stderr)
        return 2
    try:
        print(json.dumps(execute(sys.argv[1], sys.argv[2])))
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired, json.JSONDecodeError, KeyError, StopIteration) as exc:
        print('doris-whole: ' + type(exc).__name__ + ': ' + str(exc), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
