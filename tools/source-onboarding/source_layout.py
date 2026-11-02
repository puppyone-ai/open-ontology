#!/usr/bin/env python3
"""Offline source layout and importability analysis; never connects to a source."""
import argparse
import json
import re
import uuid
from pathlib import Path

KEY_TYPES = {'tinyint', 'smallint', 'mediumint', 'int', 'integer', 'bigint',
             'char', 'varchar', 'binary', 'varbinary', 'date'}


def count(value, field):
    if type(value) is not int or value < 0:
        raise ValueError(field)
    return value


def key_type(key):
    return re.split(r'[ (]', key['type'].lower())[0]


def plan(tables, domain_limit=16, source_limit=32, *, library_id=None):
    ceiling = 4096 if library_id is not None else 32
    modeled_ceiling = ceiling if library_id is not None else 16
    if (type(domain_limit) is not int or type(source_limit) is not int
            or not 1 <= domain_limit <= modeled_ceiling
            or not domain_limit <= source_limit <= ceiling):
        raise ValueError('layout_limits')
    groups, exclusions, seen = {}, [], set()
    for table in tables:
        identity = (table['schema'], table['table'])
        if any(not isinstance(n, str) or not n or '\0' in n for n in identity):
            raise ValueError('table_identity')
        if identity in seen:
            raise ValueError('duplicate_table')
        seen.add(identity)
        rows = table.get('exact_rows')
        if rows is None:
            raise ValueError('exact_count_required')
        count(rows, 'exact_rows')
        estimate = table.get('estimated_rows')
        if estimate is not None:
            count(estimate, 'estimated_rows')
        keys = table.get('primary_key', []) or table.get('unique_key', [])
        if rows == 0 and not keys:
            exclusions.append(dict(schema=identity[0], table=identity[1], reason='empty_without_key',
                                   upgrade_condition='Recount before every run; import when rows or a usable unique key exist.'))
            continue
        modeled = table.get('modeled', True)
        group = table.get('domain') or table.get('prefix') or identity[1].split('_')[0]
        if not isinstance(group, str) or not group:
            raise ValueError('domain')
        groups.setdefault((identity[0], group, bool(modeled)), []).append((table, keys))
    sources, manifests = [], {}
    for (schema, domain, modeled), members in sorted(groups.items()):
        members.sort(key=lambda pair: pair[0]['table'])
        limit = domain_limit if modeled else source_limit
        for start in range(0, len(members), limit):
            batch = members[start:start + limit]
            key = f's{len(sources) + 1:03d}'
            names = [t['table'] for t, _ in batch]
            identity = [schema, domain, modeled, names]
            if library_id is not None:
                identity.insert(0, library_id)
            source_id = str(uuid.uuid5(uuid.NAMESPACE_URL, json.dumps(identity)))
            source = dict(key=key, source_id=source_id, shard='shard_' + key,
                          tables=names, rows=sum(t['exact_rows'] for t, _ in batch),
                          schema=schema, domain=domain, publish=modeled)
            sources.append(source)
            entries = []
            for t, keys in batch:
                rows, estimate = t['exact_rows'], t.get('estimated_rows')
                unreliable = estimate is None or abs(estimate - rows) > max(1, rows * .2)
                chunking = bool(keys) and (len(keys) > 1 or rows >= 5000 or unreliable)
                entries.append(dict(schema=schema, table=t['table'],
                                    columns=[c['name'] for c in t.get('columns', [])],
                                    chunking=chunking,
                                    budgets=dict(connect_timeout_ms=15000, statement_timeout_ms=30000,
                                                 deadline_ms=14400000 if chunking else 300000,
                                                 max_rows=100000 if chunking else min(1000000, max(1, rows)),
                                                 max_bytes=33554432)))
            manifests[key] = dict(generation=1, source_id=source_id, shard=source['shard'], tables=entries)
    return dict(sources=sources, manifests=manifests,
                exclusions=sorted(exclusions, key=lambda e: (e['schema'], e['table'])))


def preflight(document):
    findings = []

    def add(code, severity='warning', table=None):
        findings.append(dict(code=code, severity=severity, table=table))

    session = document.get('session', {})
    required = {'sql_mode', 'time_zone', 'have_ssl', 'log_bin', 'privileges'}
    if not required <= session.keys():
        add('session_facts_missing', 'blocker')
    if not {'STRICT_TRANS_TABLES', 'NO_ZERO_DATE'} <= set(session.get('sql_mode', '').split(',')):
        add('session_sql_mode')
    if session.get('time_zone') not in ('+00:00', 'UTC'):
        add('session_timezone')
    if session.get('have_ssl') == 'DISABLED':
        add('plaintext_approval_required', 'blocker')
    if session.get('log_bin') == 'OFF':
        add('cdc_unavailable')
    privileges = {p.upper() for p in session.get('privileges', [])}
    if privileges - {'SELECT', 'LOCK TABLES', 'SHOW VIEW', 'PROCESS', 'REPLICATION SLAVE',
                     'REPLICATION CLIENT', 'USAGE'}:
        add('write_capable_grant', 'blocker')
    if 'SELECT' not in privileges:
        add('select_grant_missing', 'blocker')
    for table in document['tables']:
        name = dict(schema=table['schema'], table=table['table'])
        rows, estimate = table.get('exact_rows'), table.get('estimated_rows')
        if rows is None:
            add('exact_count_required', 'blocker', name)
        else:
            count(rows, 'exact_rows')
            if estimate is not None:
                count(estimate, 'estimated_rows')
            if estimate is None or abs(estimate - rows) > max(1, rows * .2):
                add('estimate_unreliable', table=name)
        keys = table.get('primary_key', []) or table.get('unique_key', [])
        if any(key_type(k) not in KEY_TYPES for k in keys):
            add('unsupported_key', 'blocker', name)
        if any(key_type(k) == 'bigint' and 'unsigned' in k['type'].lower() for k in keys):
            add('decimal_key_loader_required', table=name)
        if rows == 0 and len(keys) > 1:
            add('empty_composite_key', table=name)
        if not keys and rows:
            add('keyless_bounded_snapshot', table=name)
        columns = table.get('columns', [])
        if len(columns) > 1024:
            add('wide_table', 'blocker', name)
        elif len(columns) > 256:
            add('role_pack_column_budget', table=name)
        if any(re.search(r'text|blob', c['type'], re.I) for c in columns):
            add('lob', table=name)
        if any(re.match(r'char\((\d+)\)', c['type'], re.I) and
               int(re.match(r'char\((\d+)\)', c['type'], re.I)[1]) > 63 for c in columns):
            add('char_to_varchar', table=name)
    layout = None
    if not any(f['code'] == 'exact_count_required' for f in findings):
        layout = plan(document['tables'])
    return dict(protocol='oo.source-preflight/v1', importable=not any(
        f['severity'] == 'blocker' for f in findings), findings=findings, layout=layout,
                recommendations=['Set session UTC and strict sql_mode; never change source globals.',
                                 'Explicitly approve zero_date_to_null / strip_text_nul only after review.',
                                 'Live-source tolerance must remain explicit and report unresolved chunks.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['plan', 'preflight'])
    parser.add_argument('--input', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    document = json.loads(args.input.read_text())
    result = plan(document['tables']) if args.mode == 'plan' else preflight(document)
    args.out.mkdir(mode=0o700, parents=True, exist_ok=False)
    (args.out / 'report.json').write_text(json.dumps(result, indent=2) + '\n')
    layout = result if args.mode == 'plan' else result['layout']
    if layout is not None:
        (args.out / 'sources.json').write_text(json.dumps(layout['sources'], indent=2) + '\n')
        (args.out / 'exclusions.json').write_text(json.dumps(layout['exclusions'], indent=2) + '\n')
        for key, manifest in layout['manifests'].items():
            (args.out / (key + '-manifest.json')).write_text(json.dumps(manifest, indent=2) + '\n')
    return 2 if args.mode == 'preflight' and not result['importable'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
