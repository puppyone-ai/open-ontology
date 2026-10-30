#!/usr/bin/python3
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import stat
import uuid
from contextlib import contextmanager

POLICY_FLAGS = ('zero_date_to_null', 'strip_text_nul')
PARTS_PROTOCOL = 'oo.mysql.c5-range-parts/v1'
REQUEST_PROTOCOL = 'oo.pipeline.native-c5-range-parts/v1'
PART_PROTOCOL = 'oo.pipeline.native-c5-range-part/v1'
MAX_METADATA = 16 * 1024 * 1024
MAX_INDEX_LINE = 65536
RANGE_KEYS = (
    'source_engine', 'source_id', 'schema', 'table', 'schema_fingerprint',
    'chunk_topology_revision', 'chunk_id', 'lo', 'hi',
    'prior_membership_revision', 'parent_generation', 'source_generation',
)
NOTE_KEYS = {
    'protocol', 'source_id', 'schema', 'table', 'column', 'attnum', 'path',
    'row_index', 'key_digest', 'reason', 'value_len', 'run_generation',
    'input_digest', 'type_policy_version', 'raw_value_recorded',
}

def type_policy(value):
    if value is None:
        value = {}
    if not isinstance(value, dict) or set(value) - set(POLICY_FLAGS):
        raise RuntimeError('normalization policy shape')
    if any(type(flag) is not bool for flag in value.values()):
        raise RuntimeError('normalization policy shape')
    return {key: value.get(key, False) for key in POLICY_FLAGS}

def source_key_columns(raw, facts, fields):
    keys = raw.get('key_columns')
    flagged = [field['name'] for field in fields if field.get('key') is True]
    if (not isinstance(keys, list) or not keys
            or any(not isinstance(key, str) or not key for key in keys)
            or len(set(keys)) != len(keys) or keys != facts.get('key_columns')
            or len(flagged) != len(keys) or set(keys) != set(flagged)):
        raise RuntimeError('source key order/binding mismatch')
    return list(keys)

def normalization_metadata(raw, facts, capture, expected_policy):
    policy = type_policy(expected_policy)
    if type_policy(raw.get('type_policy')) != policy or type_policy(facts.get('type_policy')) != policy:
        raise RuntimeError('normalization policy mismatch')
    reference = raw.get('normalization_rejects')
    if reference != facts.get('normalization_rejects'):
        raise RuntimeError('normalization evidence mismatch')
    result = {'type_policy': policy}
    if reference is None:
        return result
    if not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}:
        raise RuntimeError('normalization reference')
    if (not isinstance(reference['path'], str)
            or not isinstance(reference['sha256'], str) or len(reference['sha256']) != 64
            or any(c not in '0123456789abcdef' for c in reference['sha256'])):
        raise RuntimeError('normalization reference')
    path = Path(reference['path'])
    if not path.is_absolute() or path.parent.resolve() != capture.resolve():
        raise RuntimeError('normalization path')
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077:
        raise RuntimeError('normalization notes budget/path')
    pin = file_ref(path)
    if pin['sha256'] != reference['sha256']:
        raise RuntimeError('normalization notes digest')
    count = 0
    for record in json_array(path):
        count += 1
        if not isinstance(record, dict) or set(record) - NOTE_KEYS:
            raise RuntimeError('normalization note shape')
        flag = {'zero_date': 'zero_date_to_null', 'nul_byte': 'strip_text_nul'}.get(record.get('reason'))
        if not flag or not policy[flag] or record.get('raw_value_recorded') is not False:
            raise RuntimeError('normalization note policy')
        if any(record.get(key) != raw[key] for key in ('source_id', 'schema', 'table')):
            raise RuntimeError('normalization note scope')
        if record.get('input_digest') != raw['arrow_sha256'] or record.get('run_generation') != raw['source_generation']:
            raise RuntimeError('normalization note binding')
        if (type(record.get('value_len')) is not int or not 0 <= record['value_len'] < 2**63
                or type(record.get('row_index')) is not int or not 0 <= record['row_index'] < raw['row_count']
                or not isinstance(record.get('key_digest'), str) or len(record['key_digest']) != 64
                or any(c not in '0123456789abcdef' for c in record['key_digest'])):
            raise RuntimeError('normalization note shape')
        if len(json.dumps(record, separators=(',', ':'), ensure_ascii=False).encode()) > 1024:
            raise RuntimeError('normalization note budget')
    if not count:
        raise RuntimeError('normalization notes budget')
    verify_ref(pin)
    result['normalization_rejects'] = dict(reference)
    return result


def json_array(path):
    # Normalization notes grow with rows; retain only one bounded record.
    decoder = json.JSONDecoder()
    with Path(path).open(encoding='utf-8') as stream:
        buffer = ''
        eof = False

        def more():
            nonlocal buffer, eof
            block = stream.read(4096)
            eof = not block
            buffer += block
            if len(buffer) > MAX_INDEX_LINE:
                raise RuntimeError('normalization note budget')

        def whitespace():
            nonlocal buffer
            buffer = buffer.lstrip()
            while not buffer and not eof:
                more()
                buffer = buffer.lstrip()

        whitespace()
        if not buffer.startswith('['):
            raise RuntimeError('normalization notes shape')
        buffer = buffer[1:]
        whitespace()
        if buffer.startswith(']'):
            buffer = buffer[1:]
        else:
            while True:
                whitespace()
                try:
                    record, end = decoder.raw_decode(buffer)
                except ValueError:
                    if eof:
                        raise RuntimeError('normalization notes truncated')
                    more()
                    continue
                buffer = buffer[end:]
                yield record
                whitespace()
                if buffer.startswith(']'):
                    buffer = buffer[1:]
                    break
                if not buffer.startswith(','):
                    raise RuntimeError('normalization notes separator')
                buffer = buffer[1:]
        whitespace()
        if buffer:
            raise RuntimeError('normalization notes trailing data')


def digest(path):
    with Path(path).open('rb') as stream:
        hash_ = hashlib.sha256()
        for block in iter(lambda: stream.read(65536), b''):
            hash_.update(block)
    return hash_.hexdigest()


def bounded_read(path):
    with Path(path).open('rb') as stream:
        data = stream.read(MAX_METADATA + 1)
    if len(data) > MAX_METADATA:
        raise RuntimeError('metadata size')
    return data


def load(path):
    return json.loads(bounded_read(path))


def file_identity(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mode,
            metadata.st_mtime_ns, metadata.st_ctime_ns)


@contextmanager
def private_reader(path):
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts:
        raise RuntimeError('private file path')
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or before.st_mode & 0o077:
        raise RuntimeError('private file mode/type')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        if file_identity(before) != file_identity(os.fstat(fd)):
            raise RuntimeError('private file changed')
        yield stream
        if (file_identity(before) != file_identity(os.fstat(fd))
                or file_identity(before) != file_identity(path.lstat())):
            raise RuntimeError('private file changed')


def file_ref(path):
    hash_ = hashlib.sha256()
    size = 0
    with private_reader(path) as stream:
        for block in iter(lambda: stream.read(65536), b''):
            hash_.update(block)
            size += len(block)
    return dict(path=str(path), sha256=hash_.hexdigest(), bytes=size)


def verify_ref(reference):
    if (not isinstance(reference, dict) or set(reference) != {'path', 'sha256', 'bytes'}
            or not isinstance(reference['path'], str)
            or type(reference['bytes']) is not int or reference['bytes'] <= 0
            or not isinstance(reference['sha256'], str)
            or len(reference['sha256']) != 64
            or any(c not in '0123456789abcdef' for c in reference['sha256'])
            or file_ref(reference['path']) != reference):
        raise RuntimeError('file reference changed/digest')


def save_bytes(path, value):
    pending = path.with_name(path.name + '.' + uuid.uuid4().hex + '.pending')
    with pending.open('xb') as stream:
        os.chmod(pending, 0o600)
        stream.write(value)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(pending, path)
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def save(path, value):
    save_bytes(path, (json.dumps(value, indent=2) + '\n').encode())


def call(argv, out, payload=None):
    with (out / 'stdout').open('wb') as stdout, (out / 'stderr').open('wb') as stderr:
        result = subprocess.run(argv, input=payload, stdout=stdout, stderr=stderr)
        stdout.flush()
        os.fsync(stdout.fileno())
    save(out / 'exit.json', result.returncode)
    if result.returncode:
        raise RuntimeError('layered source stage refused: ' + str(out))
    return bounded_read(out / 'stdout')


def archive_failed_capture(capture_call):
    exit_path = capture_call / 'exit.json'
    if not exit_path.exists():
        return
    status = load(exit_path)
    if type(status) is not int:
        raise RuntimeError('source capture exit metadata')
    if status == 0:
        return
    history = capture_call.parent / 'capture-failures'
    history.mkdir(mode=0o700, exist_ok=True)
    if not stat.S_ISDIR(capture_call.lstat().st_mode) or not stat.S_ISDIR(history.lstat().st_mode):
        raise RuntimeError('source capture history directory')
    # Retain the same-pin driver checkpoint; only failed invocation logs move.
    capture_call.rename(history / uuid.uuid4().hex)
    for directory in (history, capture_call.parent):
        fd = os.open(directory, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    capture_call.mkdir(mode=0o700)

def publish_registry(registered, registry):
    document = bounded_read(registry)
    if registered.exists():
        if bounded_read(registered) != document:
            raise RuntimeError('registered evidence changed')
    else:
        save_bytes(registered, document)


def freeze(path, value):
    encoded = (json.dumps(value, indent=2, sort_keys=True) + '\n').encode()
    if path.exists():
        if bounded_read(path) != encoded:
            raise RuntimeError('resume evidence changed: ' + str(path))
    else:
        save_bytes(path, encoded)


def iter_parts(raw):
    reference = raw['parts_index']
    verify_ref(reference)
    expected = raw['part_count']
    total = raw['row_count']
    if (type(expected) is not int or expected <= 0
            or type(total) is not int or total < 0 or (total == 0 and expected != 1)):
        raise RuntimeError('parts count')
    directory = Path(reference['path']).parent
    rows = count = size = 0
    hash_ = hashlib.sha256()
    with private_reader(reference['path']) as stream:
        while True:
            line = stream.readline(MAX_INDEX_LINE + 1)
            if not line:
                break
            if len(line) > MAX_INDEX_LINE or not line.endswith(b'\n'):
                raise RuntimeError('parts index line')
            hash_.update(line)
            size += len(line)
            part = json.loads(line)
            if (not isinstance(part, dict) or set(part) != {'ordinal', 'arrow', 'row_count'}
                    or type(part['ordinal']) is not int or part['ordinal'] != count
                    or count >= expected or type(part['row_count']) is not int
                    or part['row_count'] < 0 or (total and not part['row_count'])
                    or not isinstance(part['arrow'], dict)
                    or part['arrow'].get('path') != str(directory / f'part-{count}.arrow')):
                raise RuntimeError('parts order/shape')
            rows += part['row_count']
            count += 1
            if rows > total:
                raise RuntimeError('parts row count')
            verify_ref(part['arrow'])
            yield part
    if (count != expected or rows != total or size != reference['bytes']
            or hash_.hexdigest() != reference['sha256']):
        raise RuntimeError('parts incomplete/digest')


def native_request(raw, facts, record, config, batch_id):
    columns = [{key: value for key, value in field.items() if key != 'key'}
               for field in record['fields']]
    for column in columns:
        column['logical_name'] = column['name']
    native = dict(protocol='oo.pipeline.native-update/v1', source_engine='mysql',
                  source_id=raw['source_id'], source_generation=raw['source_generation'],
                  parent_source_generation=raw['parent_generation'], logical_schema=raw['schema'],
                  logical_table=raw['table'], batch_id=batch_id, run_id=record['run_id'],
                  capture_method='c5_diff', delete_detection='reconcile', consistency_grade='B',
                  cross_table_consistent=False, position_codec='snapshot-zero/v1',
                  position_lineage=raw['source_id'] + '.' + raw['table'], applied_pos='0',
                  full_snapshot=False, columns=columns,
                  key_columns=source_key_columns(raw, facts, record['fields']), tombstones=[],
                  arrow_path=raw['arrow_path'], max_arrow_bytes=config['max_arrow_bytes'],
                  admission_path=record['admission_receipt_path'], snapshot=record['snapshot'],
                  target=config['target'])
    for name in ['config_digest', 'schema_digest', 'type_digest', 'input_digest']:
        native[name] = record[name]
    native['audit_digest'] = record['capture_receipt_sha256']
    return native


def prepare_part(config, capture, directory, table):
    checkpoint = directory / 'prepared.json'
    if checkpoint.exists():
        prepared = load(checkpoint)
        for reference in prepared.values():
            verify_ref(reference)
        return Path(prepared['report']['path']).parent
    attempt = directory / ('prepare-' + uuid.uuid4().hex)
    attempt.mkdir(mode=0o700)
    inputs = attempt / 'inputs'
    argv = [sys.executable, config['prepare'], '--capture', str(capture),
            '--out', str(inputs), '--secret-ref', config['secret_ref']]
    if config.get('source_audit_policy') is not None:
        if config['source_audit_policy'] != 'grant_enforced':
            raise RuntimeError('installed source audit policy')
        argv += ['--source-audit-policy', 'grant_enforced']
    call(argv, attempt)
    table_inputs = inputs / table
    prepared = {name: file_ref(table_inputs / (name + '.json'))
                for name in ('report', 'config', 'witness')}
    save(checkpoint, prepared)
    return table_inputs


def admit_part(config, raw, scope, facts, capture, directory, pair, metadata):
    inputs = prepare_part(config, capture, directory, raw['table'])
    completed = directory / 'admitted.json'
    if completed.exists():
        saved = load(completed)
        for reference in saved['artifacts']:
            verify_ref(reference)
        registry = Path(saved['line']['registry']['path'])
    else:
        attempt = directory / ('seal-' + uuid.uuid4().hex)
        attempt.mkdir(mode=0o700)
        sealed = attempt / 'sealed'
        call([config['admission']['path'], 'seal-native', '--report', str(inputs / 'report.json'),
              '--config', str(inputs / 'config.json'), '--out', str(sealed)], attempt)
        registry = sealed / 'registry.json'
    registry_ref = file_ref(registry)
    document = load(registry)
    if (document.get('protocol') != 'oo.source-admission.binding-registry/v1'
            or len(document.get('bindings', [])) != 1):
        raise RuntimeError('part registry shape')
    record = document['bindings'][0]
    if (record.get('protocol') != 'oo.source-admission.capture-binding/v1'
            or record.get('range_scope') != scope or record.get('snapshot') != scope['snapshot']
            or record.get('run_id') != facts['run_id']
            or any(record.get(key) != raw[key] for key in ('source_id', 'source_engine', 'source_generation'))
            or record.get('input_digest') != scope['part']['arrow']['sha256']
            or record.get('objects') != [dict(logical_schema=raw['schema'], logical_table=raw['table'],
                                            row_count=scope['part']['row_count'],
                                            arrow_sha256=scope['part']['arrow']['sha256'])]
            or record.get('capture_receipt_path') != str(capture / 'capturer.json')
            or record.get('report_path') != str(inputs / 'report.json')
            or record.get('config_path') != str(inputs / 'config.json')
            or record.get('driver_path') != config['driver']['path']
            or record.get('schema_digest') != digest(facts['schema_preimage_path'])
            or record.get('type_digest') != digest(facts['type_preimage_path'])
            or record.get('driver_digest') != config['driver']['sha256']):
        raise RuntimeError('sealed part context mismatch')
    artifacts = [registry_ref]
    for name in ('capture_receipt', 'admission_receipt', 'catalog', 'report', 'config'):
        reference = file_ref(record[name + '_path'])
        if reference['sha256'] != record[name + '_sha256']:
            raise RuntimeError('sealed part evidence mismatch')
        artifacts.append(reference)
    native = native_request(raw, facts, record, config, pair)
    line = dict(scope=scope, registry=registry_ref, native_request=native,
                type_policy=metadata['type_policy'],
                normalization_rejects=metadata.get('normalization_rejects'))
    freeze(completed, dict(line=line, artifacts=artifacts))
    return line


def multipart_request(config, occurrence, raw, out, pair, layered):
    if (raw.get('target') != config['target']
            or raw.get('binding') != occurrence['binding'] or raw.get('pin') != occurrence['pin']
            or raw.get('source_engine') != 'mysql' or raw.get('capture_method') != 'c5_diff'
            or raw.get('delete_detection') != 'reconcile'
            or raw.get('position_codec') != 'snapshot-zero/v1' or raw.get('applied_pos') != '0'
            or raw.get('input_digest') != raw['parts_index']['sha256']
            or any(key in raw for key in ('native_request', 'lease', 'lease_owner', 'lease_token',
                                          'arrow_path', 'arrow_sha256', 'members', 'members_file'))
            or raw['source_generation'] != raw['parent_generation'] + 1):
        raise RuntimeError('parts source context mismatch')
    root = Path(raw['parts_index']['path']).parent
    complete = load(root / 'capture-complete.json')
    if (complete.get('protocol') != PARTS_PROTOCOL or complete.get('snapshot_complete') is not True
            or complete.get('admission_seal') is not False
            or any(complete.get(key) != raw[key] for key in (
                'source_id', 'schema', 'table', 'source_generation', 'parent_generation',
                'parts_index', 'part_count', 'row_count'))):
        raise RuntimeError('parts capture incomplete/context')
    shared = {name: file_ref(root / name) for name in (
        'capture-started.json', 'capture-complete.json', 'driver.json',
        'schema-preimage.json', 'type-preimage.json')}
    audit = complete['source_audit']
    if audit.get('records_file') is not None:
        verify_ref(audit['records_file'])
        shared['source_audit'] = audit['records_file']
    descriptor = load(root / 'driver.json')
    if (descriptor.get('executable_path') != config['driver']['path']
            or descriptor.get('executable_sha256') != config['driver']['sha256']):
        raise RuntimeError('parts driver mismatch')
    freeze(out / 'capture-context.json', shared)
    policy = type_policy(layered.get('type_policy'))
    if type_policy(raw.get('type_policy')) != policy:
        raise RuntimeError('normalization policy mismatch')
    # Pin even the not-yet-prepared parts before the first fallible subprocess.
    # A retry must not silently adopt changed later witnesses or masked notes.
    for part in iter_parts(raw):
        ordinal = part['ordinal']
        facts_path = root / f'part-{ordinal}.capture' / 'capturer.json'
        facts = load(facts_path)
        source_ref = file_ref(root / f'part-{ordinal}.source.json')
        if facts.get('part_source') != source_ref:
            raise RuntimeError('part source reference mismatch')
        pins = dict(facts=file_ref(facts_path), source=source_ref, arrow=part['arrow'])
        reference = facts.get('normalization_rejects')
        if reference is not None:
            if (not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}
                    or Path(reference['path']).parent != root):
                raise RuntimeError('normalization reference')
            pins['normalization_rejects'] = file_ref(reference['path'])
            if pins['normalization_rejects']['sha256'] != reference['sha256']:
                raise RuntimeError('normalization notes digest')
        directory = out / f'part-{ordinal}'
        directory.mkdir(mode=0o700, exist_ok=True)
        freeze(directory / 'source-pins.json', pins)
    index = out / 'admitted.jsonl'
    pending = out / ('admitted-' + uuid.uuid4().hex + '.pending')
    common_projection = None
    with pending.open('xb') as stream:
        os.chmod(pending, 0o600)
        for part in iter_parts(raw):
            ordinal = part['ordinal']
            capture = root / f'part-{ordinal}.capture'
            facts_path = capture / 'capturer.json'
            facts = load(facts_path)
            scope = dict(protocol=PART_PROTOCOL, range={key: raw[key] for key in RANGE_KEYS},
                         parts_index=raw['parts_index'], part_count=raw['part_count'],
                         total_rows=raw['row_count'], part=part, snapshot=complete['snapshot'])
            table = dict(name=raw['table'], schema=raw['schema'], arrow=f'part-{ordinal}.arrow',
                         arrow_path=part['arrow']['path'], arrow_sha256=part['arrow']['sha256'],
                         row_count=part['row_count'], route='C5', consistency='B',
                         delete_mode='reconcile', cursor_safety='no_cursor', quality='RANGE_SNAPSHOT')
            if (facts.get('range_scope') != scope or facts.get('tables') != [table]
                    or facts.get('snapshot') != complete['snapshot']
                    or facts.get('run_id') != complete['snapshot']['value']
                    or facts.get('part_ordinal') != ordinal or facts.get('snapshot_complete') is not True
                    or facts.get('source_audit') != audit
                    or facts.get('source_identity') != complete['source_identity']
                    or any(facts.get(key) != raw[key] for key in ('source_id', 'source_engine', 'source_generation'))
                    or facts.get('key_columns') != raw.get('key_columns')
                    or any(facts.get(key) != str(root / name) for key, name in (
                        ('driver_path', 'driver.json'), ('schema_preimage_path', 'schema-preimage.json'),
                        ('type_preimage_path', 'type-preimage.json')))):
                raise RuntimeError('part witness context mismatch')
            source_ref = facts['part_source']
            if source_ref['path'] != str(root / f'part-{ordinal}.source.json'):
                raise RuntimeError('part source path mismatch')
            verify_ref(source_ref)
            source = load(source_ref['path'])
            if (source.get('ordinal') != ordinal or source.get('run_id') != facts['run_id']
                    or source.get('connection_id') != complete['connection_id']):
                raise RuntimeError('part source context mismatch')
            part_raw = dict(raw, arrow_path=part['arrow']['path'], arrow_sha256=part['arrow']['sha256'],
                            row_count=part['row_count'], normalization_rejects=source.get('normalization_rejects'))
            metadata = normalization_metadata(part_raw, facts, root, policy)
            directory = out / f'part-{ordinal}'
            directory.mkdir(mode=0o700, exist_ok=True)
            pins = dict(facts=file_ref(facts_path), source=source_ref, arrow=part['arrow'])
            if metadata.get('normalization_rejects') is not None:
                pins['normalization_rejects'] = file_ref(metadata['normalization_rejects']['path'])
            freeze(directory / 'source-pins.json', pins)
            line = admit_part(config, part_raw, scope, facts, capture, directory, pair, metadata)
            projection = {key: line['native_request'][key] for key in (
                'schema_digest', 'type_digest', 'columns', 'key_columns')}
            projection['capture'] = {key: facts.get(key) for key in (
                'config_sha256', 'max_rows', 'max_bytes', 'observed_privileges')}
            if common_projection is not None and common_projection != projection:
                raise RuntimeError('part projection drift')
            common_projection = projection
            encoded = (json.dumps(line, separators=(',', ':')) + '\n').encode()
            if len(encoded) > MAX_METADATA:
                raise RuntimeError('admitted metadata size')
            stream.write(encoded)
        stream.flush()
        os.fsync(stream.fileno())
    for reference in shared.values():
        verify_ref(reference)
    for part in iter_parts(raw):
        directory = out / f"part-{part['ordinal']}"
        for reference in load(directory / 'source-pins.json').values():
            verify_ref(reference)
        for reference in load(directory / 'admitted.json')['artifacts']:
            verify_ref(reference)
    if index.exists():
        if file_ref(index)['sha256'] != file_ref(pending)['sha256']:
            raise RuntimeError('admitted index changed')
        pending.unlink()
    else:
        os.replace(pending, index)
    admitted = file_ref(index)
    result = dict(raw, protocol=REQUEST_PROTOCOL, admitted_index=admitted)
    descriptor = dict(protocol='oo.source-admission.c5-parts-index/v1', admitted_index=admitted,
                      parts_index=raw['parts_index'], part_count=raw['part_count'],
                      row_count=raw['row_count'], source_id=raw['source_id'],
                      source_generation=raw['source_generation'])
    registered = Path(config['registry_template'].format(binding=occurrence['binding'], pin=occurrence['pin']))
    registered.parent.mkdir(parents=True, exist_ok=True)
    freeze(registered, descriptor)
    freeze(out / 'request.json', result)
    return result


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    args = parser.parse_args()
    raw_config = bounded_read(args.config)
    config = json.loads(raw_config)
    occurrence_text = sys.stdin.read(MAX_METADATA + 1)
    if len(occurrence_text) > MAX_METADATA:
        raise RuntimeError('occurrence metadata size')
    occurrence = json.loads(occurrence_text)
    if occurrence['method'] != 'C5' or occurrence['c5_mode'] != 'layered':
        raise RuntimeError('layered occurrence required')
    for name in ['driver', 'admission']:
        if digest(config[name]['path']) != config[name]['sha256']:
            raise RuntimeError(name + ' changed')
    binding = occurrence['binding']
    pair = str(uuid.UUID(binding)) + '_' + str(int(occurrence['pin']))
    root = Path(config['state_dir']).absolute()
    root.mkdir(parents=True, exist_ok=True)
    with (root / 'producer.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        out = root / pair
        out.mkdir(exist_ok=True)
        frozen = out / 'operator.json'
        config_hash = hashlib.sha256(raw_config).hexdigest()
        if frozen.exists():
            if load(frozen) != config_hash:
                raise RuntimeError('operator configuration changed')
        else:
            save(frozen, config_hash)
        request_path = out / 'request.json'
        if (request_path.exists() and not (out / 'capture-context.json').exists()
                and load(request_path).get('protocol') in (
                    'oo.pipeline.native-c5-range/v1', 'oo.jobs.native-c5-nochange/v1')):
            sys.stdout.write(bounded_read(request_path).decode())
            return
        layered_config = load(config['layered_config'])
        freeze(out / 'invocation.json', dict(occurrence=occurrence,
               layered_config_sha256=digest(config['layered_config']),
               prepare_sha256=digest(config['prepare'])))
        capture_call = out / 'capture-call'
        capture_call.mkdir(exist_ok=True)
        source_path = out / 'source.json'
        if not source_path.exists():
            archive_failed_capture(capture_call)
            # A completed capture survives prepare/seal failure, including old
            # producer runs that only retained capture-call/stdout.
            stdout = capture_call / 'stdout'
            if stdout.exists():
                captured = bounded_read(stdout)
                json.loads(captured)
            else:
                captured = call([config['driver']['path'], 'c5-layered', '--config',
                                 config['layered_config']], capture_call, json.dumps(occurrence).encode())
                json.loads(captured)
            save_bytes(source_path, captured)
        freeze(out / 'source-pin.json', file_ref(source_path))
        raw = load(source_path)
        if raw['protocol'] == PARTS_PROTOCOL:
            result = multipart_request(config, occurrence, raw, out, pair, layered_config)
            sys.stdout.write(json.dumps(result) + '\n')
            return
        if raw['protocol'] == 'oo.jobs.native-c5-nochange/v1':
            save(request_path, raw)
            sys.stdout.write(bounded_read(request_path).decode())
            return
        capture = Path(raw['arrow_path']).parent
        facts = load(capture / 'capturer.json')
        metadata = normalization_metadata(raw, facts, capture, layered_config.get('type_policy'))
        scope = {key: raw[key] for key in facts['range_scope']}
        if scope != facts['range_scope'] or raw['target'] != config['target']:
            raise RuntimeError('range evidence mismatch')
        inputs = out / 'inputs'
        if not inputs.exists():
            prepare = out / 'prepare-call'
            prepare.mkdir(exist_ok=True)
            argv = ['/usr/bin/python3', config['prepare'], '--capture', str(capture), '--out', str(inputs), '--secret-ref', config['secret_ref']]
            if config.get('source_audit_policy') is not None:
                if config['source_audit_policy'] != 'grant_enforced':
                    raise RuntimeError('installed source audit policy')
                argv += ['--source-audit-policy', 'grant_enforced']
            call(argv, prepare)
        table_inputs = inputs / raw['table']
        sealed = out / 'sealed'
        if not sealed.exists():
            seal = out / 'seal-call'
            seal.mkdir(exist_ok=True)
            call([config['admission']['path'], 'seal-native', '--report', str(table_inputs / 'report.json'), '--config', str(table_inputs / 'config.json'), '--out', str(sealed)], seal)
        registry = sealed / 'registry.json'
        binding_record = load(registry)['bindings'][0]
        if binding_record.get('range_scope') != scope:
            raise RuntimeError('sealed range scope mismatch')
        native = native_request(raw, facts, binding_record, config, pair)
        raw.update(metadata)
        raw['native_request'] = native
        registered = Path(config['registry_template'].format(binding=occurrence['binding'], pin=occurrence['pin']))
        registered.parent.mkdir(parents=True, exist_ok=True)
        publish_registry(registered, registry)
        save(request_path, raw)
        sys.stdout.write(bounded_read(request_path).decode())


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('layered producer: ' + str(error), file=sys.stderr)
        sys.exit(1)
