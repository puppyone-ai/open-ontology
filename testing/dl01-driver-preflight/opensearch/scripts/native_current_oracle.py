#!/usr/bin/env python3
"""Compare every current sealed Arrow cell with independent physical PG reads."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('cells', HERE / 'native_cell_oracle.py')
cells = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cells)
base = cells.ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2'
legal = os.environ.get('OO_OPENSEARCH_LEGAL_BATCH') == '1'
out = base / ('current-20261013/legal-batch/oracle' if legal else 'current-20261013/oracle')
binding = 'f7bae27e-e2b5-489f-9326-64801ec5aec4'


def psql(sql):
    return subprocess.run(['docker', 'exec', 'oo-dl01w1-44-native-fresh-pg', 'psql', '-U', 'postgres',
        '-d', 'oo_dl01w1_44_native_fresh', '-v', 'ON_ERROR_STOP=1', '-At', '-c', sql],
        check=True, capture_output=True, text=True).stdout


def main():
    out.mkdir(parents=True, exist_ok=False)
    decoded_dir = out / 'decode'
    decoded_dir.mkdir()
    decoder = base.parent / 'native-20261012/bin/os-decode'
    import hashlib
    assert hashlib.sha256(decoder.read_bytes()).hexdigest() == '7cb5a706267893f76e2b9df965c650b81283d0f2d781b4624de9b9c0cb4d6fa2'
    mappings = json.loads(psql("SELECT COALESCE(json_agg(json_build_object('generation',run_generation,'level',level,'column',logical_column,'physical',physical_name)),'[]'::json)::text FROM _oo.physical_mappings WHERE source='oo_dl01w1_44' AND logical_namespace='ledger'"))
    tables = {row['generation']: row['physical'] for row in mappings if row['level'] == 't'}
    columns = {}
    for row in mappings:
        if row['level'] == 'c':
            columns.setdefault(row['generation'], {})[row['column']] = row['physical']
    diffs = []
    compared = 0
    reports = []
    pins = ((1, 1), (2, 2), (3, 3), (4, 4), (6, 5), (7, 6)) if legal else ((1, 1), (2, 2), (3, 3), (4, 4), (6, 5))
    for pin, generation in pins:
        package = base / 'scheduled-source' / binding / str(pin)
        request = json.loads((package / 'loader-input.json').read_text())
        arrow = Path(request['arrow_path'])
        decoded = decoded_dir / f'gen{generation}.json'
        decoded.touch()
        proc = subprocess.run(['docker', 'run', '--rm', '--pull', 'never', '--network', 'none',
            '--entrypoint', '/opt/os-decode', '-v', f'{decoder}:/opt/os-decode:ro',
            '-v', f'{arrow}:/input.arrow:ro', '-v', f'{decoded}:/output.json',
            'oo-a395-selfbuild:local', '--ipc', '/input.arrow', '--out', '/output.json'],
            capture_output=True)
        (out / f'decode-gen{generation}.stdout').write_bytes(proc.stdout)
        (out / f'decode-gen{generation}.stderr').write_bytes(proc.stderr)
        (out / f'decode-gen{generation}.exit').write_text(str(proc.returncode) + '\n')
        if proc.returncode:
            raise RuntimeError(f'decode gen{generation} failed')
        source = json.loads(decoded.read_text())
        table = tables[generation]
        mapped = columns[generation]
        if set(mapped) != {field['name'] for field in source['fields']}:
            diffs.append(f'schema:gen{generation}')
        definitions = json.loads(psql("SELECT COALESCE(json_object_agg(a.attname,t.typname),'{}'::json)::text FROM pg_attribute a JOIN pg_type t ON t.oid=a.atttypid WHERE a.attrelid='_oo." + table + "'::regclass AND a.attnum>0 AND NOT a.attisdropped"))
        pieces = []
        for logical, physical in sorted(mapped.items()):
            typ = definitions[physical]
            if typ == 'numeric':
                expression = f'{cells.quote_ident(physical)}::text'
                pieces.append(f"'{logical}', {expression}")
            else:
                pieces.append(cells.expr(logical, physical, typ))
        sql = "SELECT COALESCE(json_agg(row_json ORDER BY row_json->>'source_doc_id'),'[]'::json)::text FROM (SELECT json_build_object(" + ','.join(pieces) + ") row_json FROM _oo." + cells.quote_ident(table) + ') q'
        target = json.loads(psql(sql))
        (out / f'pg-gen{generation}.json').write_text(json.dumps(target, indent=2) + '\n')
        left = {r['source_doc_id']: r for r in source['rows']}
        right = {r['source_doc_id']: r for r in target}
        if len(left) != len(source['rows']) or len(right) != len(target) or set(left) != set(right):
            diffs.append(f'keys:gen{generation}')
        for key in sorted(set(left) & set(right)):
            l, r = left[key], right[key]
            for field in ('source_doc_id', 'source_index'):
                compared += 1
                if l[field] != r[field]: diffs.append(f'{generation}:{key}:{field}')
            compared += 1
            if r['raw_source'] is None or bytes.fromhex(r['raw_source']).decode('utf-8') != l['raw_source_utf8']:
                diffs.append(f'{generation}:{key}:raw_source')
            for field, value in l['columns'].items():
                compared += 2
                if r['present__' + field] != ('true' if value['present'] else 'false'):
                    diffs.append(f'{generation}:{key}:present__{field}')
                if r[field] != cells.arrow_cell(value):
                    diffs.append(f'{generation}:{key}:{field}')
        reports.append({'generation': generation, 'pin': pin, 'rows': len(target), 'columns': len(mapped), 'table': table})
    verdict = {'diff_count': len(diffs), 'diffs': diffs, 'compared_cells': compared,
        'sampled': False, 'generations': reports, 'source': 'sealed Arrow decoder',
        'target': 'independent physical PostgreSQL select'}
    (out / 'verdict.json').write_text(json.dumps(verdict, indent=2) + '\n')
    print(json.dumps({'diff_count': len(diffs), 'compared_cells': compared, 'generations': reports}))
    return 0 if not diffs and compared > 0 else 2


if __name__ == '__main__':
    raise SystemExit(main())
