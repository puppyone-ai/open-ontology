#!/usr/bin/env python3
"""Stream every approved PG cell from the source and current native target."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def query(container, database, sql):
    command = ['docker', 'exec', container, 'psql', '-U', 'postgres', '-d', database,
               '-X', '-A', '-t', '-v', 'ON_ERROR_STOP=1', '-c', sql]
    return subprocess.check_output(command, stderr=subprocess.PIPE, text=True, timeout=30).strip()


def quote(name):
    if not name.isascii() or not name.isidentifier():
        raise ValueError('unsafe identifier')
    return '"' + name + '"'


def stream(container, database, sql):
    return subprocess.Popen(
        ['docker', 'exec', container, 'psql', '-U', 'postgres', '-d', database,
         '-X', '-A', '-q', '-t', '-v', 'ON_ERROR_STOP=1', '-c', sql],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )


def select_expr(name, oid):
    ref = quote(name)
    if oid == 17:
        return f'encode({ref},\'hex\')'
    if oid == 1184:
        return f'extract(epoch from {ref})::text'
    return f'{ref}::text'


def inspect(config):
    approved = {(obj['schema'], obj['table']) for obj in config['approved_objects']}
    fields = {}
    for field in config['approved_fields']:
        key = (field['schema'], field['table'])
        if key not in approved:
            raise ValueError('field outside approved scope')
        fields.setdefault(key, []).append(field['column'])
    reports = []
    for schema, table in sorted(approved):
        columns = fields[(schema, table)]
        source_sql = ("SELECT a.attname,a.atttypid,a.atttypmod FROM pg_attribute a "
                      "JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace "
                      f"WHERE n.nspname='{schema}' AND c.relname='{table}' AND a.attnum>0 "
                      "AND NOT a.attisdropped ORDER BY a.attnum")
        source_meta = [line.split('|') for line in query('oo-dl01w1-46-src', 'src', source_sql).splitlines()]
        source_meta = [(name, int(oid), int(mod)) for name, oid, mod in source_meta if name in columns]
        if [name for name, _, _ in source_meta] != columns:
            raise ValueError('approved source column order changed')
        mapping_sql = ("SELECT t.scope_table_physical,m.logical_column,m.physical_name,a.atttypid,a.atttypmod "
                       "FROM _oo.physical_mappings t JOIN _oo.physical_mappings m "
                       "ON m.source=t.source AND m.config_digest=t.config_digest AND m.run_generation=t.run_generation "
                       "AND m.logical_namespace=t.logical_namespace AND m.logical_table=t.logical_table "
                       "AND m.level='c' JOIN pg_class c ON c.relnamespace='_oo'::regnamespace "
                       "AND c.relname=t.scope_table_physical JOIN pg_attribute a ON a.attrelid=c.oid "
                       "AND a.attname=m.physical_name "
                       f"WHERE t.level='t' AND t.logical_namespace='{schema}' AND t.logical_table='{table}' "
                       "AND t.run_generation=(SELECT max(z.run_generation) FROM _oo.physical_mappings z "
                       f"WHERE z.level='t' AND z.logical_namespace='{schema}' AND z.logical_table='{table}') "
                       "ORDER BY a.attnum")
        target_meta = [line.split('|') for line in query('oo-dl01w1-46-native', 'dst_native_r2', mapping_sql).splitlines()]
        if len(target_meta) != len(columns):
            raise ValueError('target mapping cardinality')
        by_name = {name: (physical, int(oid), int(mod), physical_table)
                   for physical_table, name, physical, oid, mod in target_meta}
        if set(by_name) != set(columns):
            raise ValueError('target mapping columns')
        types = [(name, oid, mod) for name, oid, mod in source_meta]
        if any((by_name[name][1], by_name[name][2]) != (oid, mod) for name, oid, mod in types):
            raise ValueError('target original OID/typmod mismatch')
        physical_table = next(iter({value[3] for value in by_name.values()}))
        pk = columns[0]
        source_fields = ','.join(select_expr(name, oid) for name, oid, _ in types)
        target_fields = ','.join(select_expr(by_name[name][0], oid) for name, oid, _ in types)
        source_copy = (f"COPY (SELECT {source_fields} FROM {quote(schema)}.{quote(table)} "
                       f"ORDER BY {quote(pk)}) TO STDOUT WITH (FORMAT csv, NULL '\\N')")
        target_copy = (f"COPY (SELECT {target_fields} FROM _oo.{quote(physical_table)} "
                       f"ORDER BY {quote(by_name[pk][0])}) TO STDOUT WITH (FORMAT csv, NULL '\\N')")
        left = stream('oo-dl01w1-46-src', 'src', source_copy)
        right = stream('oo-dl01w1-46-native', 'dst_native_r2', target_copy)
        source_hash, target_hash = hashlib.sha256(), hashlib.sha256()
        differences, rows = [], 0
        while True:
            a, b = left.stdout.readline(), right.stdout.readline()
            if not a and not b:
                break
            rows += 1
            source_hash.update(a)
            target_hash.update(b)
            if a != b and len(differences) < 10:
                differences.append(rows)
        le, re = left.wait(timeout=180), right.wait(timeout=180)
        if le or re:
            raise RuntimeError(f'COPY failed: source={le} target={re}')
        reports.append({'dataset': f'{schema}.{table}', 'rows': rows, 'columns': len(columns),
                        'cells': rows * len(columns), 'source_sha256': source_hash.hexdigest(),
                        'target_sha256': target_hash.hexdigest(), 'first_mismatched_rows': differences,
                        'difference_count_zero': source_hash.digest() == target_hash.digest()})
    return {'protocol': 'oo.postgres-preview.independent-native-diff/v1',
            'tables': reports, 'rows': sum(row['rows'] for row in reports),
            'cells': sum(row['cells'] for row in reports),
            'difference_count_zero': all(row['difference_count_zero'] for row in reports)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = inspect(json.loads(args.config.read_text()))
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'rows': result['rows'], 'cells': result['cells'],
                      'difference_count_zero': result['difference_count_zero']}))
    return 0 if result['difference_count_zero'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
