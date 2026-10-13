"""Validate caller authorization before any Oracle connection or secret read.

Secret references resolve through a separately supplied local registry, never
through an arbitrary caller-provided password path. Schema profiles describe
supported exact encodings; object names remain caller-selected grant targets.
"""
import json
import os
import re
import stat
import sys
from pathlib import Path

IDENT = re.compile(r'[A-Za-z][A-Za-z0-9_]{0,127}\Z')
PROFILES = {'batch': ('ID', 'N16', 'N1', 'D', 'C6', 'V2', 'R'),
            'wide': ('ID', 'N_FREE'), 'ts': ('ID', 'TS', 'TSTZ')}

class CustomerRefusal(ValueError):
    def __init__(self, message, trace):
        super().__init__(message)
        self.trace = trace

def refuse(trace, message):
    """Authorization failed. The registry file is not opened."""
    trace.append({
        'step': 'registry_before_open',
        'registry_io': False,
        'password_read': False,
        'no_password_read': True,
        'source_io': False,
        'oracle_read': False,
    })
    raise CustomerRefusal(message, trace)

def object_name(value):
    if not isinstance(value, str): raise ValueError('object identifier')
    parts = value.split('.')
    if len(parts) != 2 or any(not IDENT.fullmatch(x) for x in parts):
        raise ValueError('object identifier')
    return value.upper()

def load_customer(path, registry_path):
    doc = json.loads(Path(path).read_text())
    trace = []
    if doc.get('protocol') != 'oo.customer-input/v1': raise ValueError('customer protocol')
    trace.append({'step': 'protocol', 'ok': True})
    source = doc.get('source', {})
    for key in ('reader', 'service'):
        if not isinstance(source.get(key), str) or not IDENT.fullmatch(source[key]):
            raise ValueError('source '+key)
    host = source.get('container')
    if not isinstance(host, str) or not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.-]{0,252}',host):
        raise ValueError('source host')
    trace.append({'step': 'source_identifiers', 'ok': True, 'reader': source['reader'], 'service': source['service']})
    approved = doc.get('approved_objects', [])
    if not approved: raise ValueError('empty authorization')
    approved = {object_name(x) for x in approved}
    selections = doc.get('selections', [])
    if not selections: raise ValueError('empty selection')
    checked = []
    for item in selections:
        obj = object_name(item.get('object'))
        profile = item.get('profile')
        fields = item.get('fields')
        if obj not in approved:
            refuse(trace, 'unapproved object')
        if profile not in PROFILES: raise ValueError('unsupported schema profile')
        if not fields or len(set(fields)) != len(fields) or any(x not in PROFILES[profile] for x in fields):
            refuse(trace, 'unapproved or unsupported field')
        if tuple(fields) != PROFILES[profile]:
            raise ValueError('partial projection unsupported by this exact encoding')
        item['object'] = obj
        checked.append({'object': obj, 'profile': profile, 'fields': list(fields)})
    budget = doc.get('budget', {})
    if budget.get('unit') != 'Rows' or type(budget.get('hard_limit')) is not int or not 1 <= budget['hard_limit'] <= 8:
        refuse(trace, 'source row budget')
    trace.append({'step': 'authorization', 'ok': True, 'approved_objects': sorted(approved), 'selections': checked, 'budget': budget})
    # Authorization and identifiers have all been validated before registry I/O.
    registry_file = Path(registry_path)
    trace.append({'step': 'registry_before_open', 'path_exists': registry_file.exists(), 'registry_io': False})
    registry = json.loads(registry_file.read_text())
    trace.append({'step': 'registry_open', 'registry_io': True, 'keys': sorted(k for k in registry if isinstance(k, str))})
    ref = doc.get('secret_ref')
    if not isinstance(ref,str) or not ref.startswith('secret://') or ref not in registry:
        raise ValueError('unresolved secret reference')
    password = Path(registry[ref])
    if not password.is_absolute(): raise ValueError('secret path must be absolute')
    meta = password.lstat()
    mode = stat.S_IMODE(meta.st_mode)
    trace.append({
        'step': 'secret_file',
        'ref': ref,
        'path': str(password),
        'absolute': password.is_absolute(),
        'is_symlink': password.is_symlink(),
        'is_regular': stat.S_ISREG(meta.st_mode),
        'mode': format(mode, '04o'),
        'other_bits_clear': (meta.st_mode & 0o077) == 0,
        'owner_uid': meta.st_uid,
        'owner_uid_matches_caller': meta.st_uid == os.getuid(),
        'byte_length': meta.st_size,
        'secret_bytes_recorded': False,
    })
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise ValueError('secret file ownership or mode')
    doc['_open_trace'] = trace
    doc['_password_file'] = str(password)
    return doc


def main(argv):
    if len(argv) != 4 or argv[1] != 'refuse':
        print('usage: customer_input.py refuse <customer.json> <registry.json>', file=sys.stderr)
        return 2
    try:
        load_customer(argv[2], argv[3])
    except CustomerRefusal as exc:
        sys.stdout.write(json.dumps({
            'ok': False,
            'error': str(exc),
            'trace': exc.trace,
            'registry_opened': False,
            'password_read': False,
            'source_io': False,
        }, sort_keys=True) + '\n')
        return 2
    sys.stdout.write('unexpected success\n')
    return 1


if __name__ == '__main__':
    raise SystemExit(main(sys.argv))
