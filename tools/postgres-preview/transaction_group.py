"""Assemble one admitted CDC group from the tables that transaction changed."""
import hashlib
import json
from pathlib import Path


def _key(row):
    return (row['logical_schema'], row['logical_table'])


def already_applied(scope, acknowledged_scope):
    """Recognize a replay of the acknowledged commit, and reject an LSN collision."""
    end = (scope or {}).get('end_lsn')
    if not end or not acknowledged_scope:
        return False
    if end != acknowledged_scope.get('end_lsn'):
        return False
    identity = lambda value: (
        value.get('source_id'), value.get('xid'),
        tuple(sorted((table['logical_schema'], table['logical_table'])
                     for table in value.get('tables', []))),
    )
    if not identity(scope)[0] or identity(scope)[1] is None:
        raise RuntimeError('same-LSN replay lacks transaction identity')
    if identity(scope) != identity(acknowledged_scope):
        raise RuntimeError('commit LSN conflicts with acknowledged transaction')
    return True


def assemble(entries, scopes, vector_entries, generation):
    """Keep the admitted pages for `generation` that the scope actually names.

    A PostgreSQL transaction may change one table. Pages from another
    generation, and publication tables absent from this scope, stay out.
    """
    generation = int(generation)
    chosen = [entry for entry in entries if int(entry['source_generation']) == generation]
    chosen.sort(key=_key)
    keys = [_key(entry) for entry in chosen]
    if not keys or len(keys) != len(set(keys)):
        raise RuntimeError('admitted transaction group mismatch')
    matches = []
    for scope in scopes:
        tables = scope.get('tables') or []
        got = [_key(table) for table in tables]
        gens = [int(table['source_generation']) for table in tables]
        if len(got) == len(set(got)) and set(got) == set(keys) and gens == [generation] * len(keys):
            matches.append(scope)
    if len(matches) != 1:
        raise RuntimeError('admitted transaction group mismatch')
    scope = matches[0]
    order = [_key(table) for table in scope['tables']]
    by_key = {_key(entry): entry for entry in chosen}
    chosen = [by_key[key] for key in order]
    vec = []
    keyset = set(keys)
    for item in vector_entries:
        if _key(item) not in keyset:
            continue
        if not str(item['registry_path']).endswith('/%s/registry.json' % generation):
            continue
        vec.append(item)
    vec_by_key = {_key(item): item for item in vec}
    if set(vec_by_key) != keyset or len(vec_by_key) != len(vec):
        raise RuntimeError('admitted transaction group mismatch')
    vec = [vec_by_key[key] for key in order]
    return chosen, scope, vec


def load_admitted(capture_root):
    """Read sealed pages already admitted under `capture_root`. Does not capture."""
    root = Path(capture_root)
    scopes = []
    seen = set()
    entries = []
    vector = []
    for cap_path in sorted((root / 'native-admit').glob('*/*.capture.json')):
        cap = json.loads(cap_path.read_text())
        scope = cap.get('transaction_scope')
        if scope is None:
            continue
        marker = (scope.get('end_lsn'), scope.get('xid'), tuple(
            (table['logical_schema'], table['logical_table'], table['source_generation'])
            for table in scope['tables']))
        if marker not in seen:
            seen.add(marker)
            scopes.append(scope)
        generation = int(cap['source_generation'])
        table = cap['logical_schema'] + '.' + cap['logical_table']
        seal = root / 'sealed' / table / str(generation)
        registry_path = seal / 'registry.json'
        decision_path = seal / 'decision.json'
        if not registry_path.is_file() or not decision_path.is_file():
            continue
        if json.loads(decision_path.read_text()).get('admitted') is not True:
            continue
        req = json.loads(cap_path.with_name(cap_path.name.replace('.capture.', '.request.')).read_text())
        binding = json.loads(registry_path.read_text())['bindings'][0]
        fields = {field['name']: field for field in binding['fields']}
        for column in req['columns']:
            for name in ('source_oid', 'source_typmod', 'source_format'):
                column[name] = fields[column['name']][name]
        for name in ('schema_digest', 'type_digest', 'config_digest', 'run_id'):
            req[name] = binding[name]
        req['audit_digest'] = binding['capture_receipt_sha256']
        req['admission_path'] = binding['admission_receipt_path']
        req['admission_expect'] = {'declared_receipt_sha256': binding['admission_receipt_sha256']}
        req['max_arrow_bytes'] = 16 * 1024 * 1024
        entries.append(req)
        vector.append({
            'logical_schema': cap['logical_schema'],
            'logical_table': cap['logical_table'],
            'registry_path': str(registry_path),
            'registry_sha256': hashlib.sha256(registry_path.read_bytes()).hexdigest(),
        })
    entries.sort(key=_key)
    vector.sort(key=_key)
    return entries, scopes, vector
