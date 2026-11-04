#!/usr/bin/env python3
"""Collect explicit suite verdicts from committed summaries, never local raw runs."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
INDEX = 'execution/v0.2/evidence-index.json'
SHA = re.compile(r'^[0-9a-f]{64}$')
SUITE = re.compile(r'^T[0-8]$')


def target(engine, version):
    if engine == 'mysql':
        return ('mysql57' if version.startswith('5.7') else
                'mysql9' if version.startswith('9.') else 'mysql8') + '-selfmanaged'
    if engine == 'oracle':
        return 'oracle-' + {'11': '11g-xe', '19': '19c-selfmanaged',
                            '21': '21c-xe', '23': '23ai-selfmanaged'}[version[:2]]
    return {'postgres': 'postgres-selfmanaged', 'sqlserver': 'sqlserver-selfmanaged',
            'mariadb': 'mariadb-lts', 'opengauss': 'opengauss-gaussdb',
            'oceanbase': 'oceanbase-mysql'}.get(engine, engine)


def verdict(value):
    if not isinstance(value, str):
        return None
    value = value.lower()
    if value in ('pass', 'passed', 'fail', 'failed', 'skip', 'skipped'):
        return {'passed': 'pass', 'failed': 'fail', 'skipped': 'skip'}.get(value, value)
    if value in ('partial', 'not_run', 'blocked_startup') or value.startswith('component_pass'):
        return 'skip'
    return None


def row(engine, version, suite, result, run, digest, variant=None, emulated=False):
    result = verdict(result)
    if not all((engine, version, SUITE.fullmatch(suite), result, run)):
        return None
    if not SHA.fullmatch(digest):
        raise ValueError('invalid recorded result hash')
    return dict(target=target(engine, version), version=version, suite=suite,
                variant=variant, result=result, run_id=run, sha256=digest, emulated=emulated)


def result_hash(record, suite, fallback):
    hashes = record.get('artifacts_sha256', record.get('artifact_sha256',
                        record.get('evidence_sha256', record.get('sha256', {}))))
    if not isinstance(hashes, dict):
        return fallback
    names = {'T1': ('t1-t8.json', 'result.json'), 'T8': ('resource-receipt.json', 't1-t8.json', 'result.json'),
             'T7': ('audit-result.json', 'audit-report.json'),
             'T0': ('T0.json', 'result.json'), 'T2': ('T1-T2.json', 'result.json')}
    for name in names.get(suite, ('result.json',)):
        if name in hashes:
            return hashes[name]
    return fallback


def json_suites(path, data, digest):
    """Direct Tn verdict records (Oracle runs, SSH acceptance, openGauss)."""
    if not isinstance(data, dict):
        return []
    engine = path.split('/')[2] if path.startswith('testing/engines/') else None
    records = []
    for key in ('runs', 'versions', 'engines', 'acceptance'):
        if isinstance(data.get(key), list):
            records.extend(data[key])
    if engine == 'opengauss':
        for version, record in data.get('engine_results', {}).items():
            hashes = {k.rsplit('/', 1)[-1]: v for k, v in data.get('sha256', {}).items()
                      if f'/{version}/' in k}
            records.append(dict(record, version=version, run_id=data.get('run_id'), sha256=hashes))
    out = []
    for rec in records:
        if not isinstance(rec, dict):
            continue
        family = rec.get('engine', engine)
        version = rec.get('version_actual', rec.get('version'))
        if isinstance(version, dict):
            family = version.get('family', family)
            version = version.get('raw')
        if not isinstance(version, str) or not family:
            continue
        run = rec.get('run_id', data.get('run_id'))
        platform = rec.get('platform', '')
        if isinstance(rec.get('image'), dict):
            platform = rec['image'].get('platform', platform)
        # These lane summaries document Apple Silicon execution (Plan D1).
        emulated = rec.get('execution') == 'emulated' or (engine == 'oracle' and platform == 'linux/amd64')
        for suite in (f'T{i}' for i in range(9)):
            value = rec.get(suite)
            variant = rec.get('mode') if suite in ('T2', 'T3', 'T7') else None
            if suite == 'T7' and isinstance(rec.get('t7_pass'), bool):
                value = 'pass' if rec['t7_pass'] else 'fail'
            if engine == 'opengauss' and isinstance(value, str) and value.startswith('passed'):
                # Engine-only oracle, not Agent DB product acceptance.
                value = 'skip'
            r = row(family, version, suite, value, run, result_hash(rec, suite, digest), variant, emulated)
            if r:
                out.append(r)
    return out


def explicit_rows(data):
    """Already normalized evidence rows; no surrounding success inheritance."""
    if not isinstance(data, list):
        return []
    out = []
    for r in data:
        if not isinstance(r, dict) or not all(r.get(k) for k in ('target', 'version', 'suite', 'result', 'run_id', 'sha256')):
            continue
        if not SUITE.fullmatch(r['suite']) or r['result'] not in ('pass', 'fail', 'skip') or not SHA.fullmatch(r['sha256']):
            raise ValueError('invalid explicit evidence row')
        if not isinstance(r.get('emulated', False), bool):
            raise ValueError('emulated must be boolean')
        out.append({k: r.get(k, False if k == 'emulated' else None) for k in
                    ('target', 'version', 'suite', 'variant', 'result', 'run_id', 'sha256', 'emulated')})
    return out


def tables(text):
    return [[c.strip().strip('`') for c in line.strip().strip('|').split('|')]
            for line in text.splitlines() if line.startswith('|') and not re.match(r'^\|[ :|-]+$', line)]


def pins(text, digest):
    """PINS fixture T0 is explicitly skip for product admission."""
    runs = dict((e.lower(), r) for e, r in re.findall(r'(MySQL|PostgreSQL) run: `([^`]+)`', text))
    out = []
    for cells in tables(text):
        if len(cells) != 4 or not re.match(r'^(mysql|postgres) \d', cells[0]):
            continue
        engine, version = cells[0].split()
        if verdict(cells[2]) is None:
            continue
        run = runs.get('postgresql' if engine == 'postgres' else engine)
        out.append(row(engine, version, 'T0', 'skip', run, cells[3], emulated='emulated' in cells[1]))
    return [r for r in out if r]


def mysql_t1(text, digest):
    hashes = {c[0]: c[1] for c in tables(text) if len(c) == 2 and SHA.fullmatch(c[1])}
    out = []
    for c in tables(text):
        if len(c) != 7 or not re.fullmatch(r'\d+\.\d+\.\d+', c[0]):
            continue
        values = c[2].split(' / ')
        if len(values) != 2:
            continue
        for suite, value in zip(('T1', 'T8'), values):
            out.append(row('mysql', c[0], suite, value, c[6],
                           hashes.get(f'{c[6]}/mysql-{c[0]}', digest), emulated='emulated' in c[1]))
    return [r for r in out if r]


def fixture_t0(text, digest):
    hashes = {c[0]: c[1] for c in tables(text) if len(c) == 2 and SHA.fullmatch(c[1])}
    out = []
    for c in tables(text):
        if len(c) == 5 and c[0] in ('mysql', 'postgres', 'oracle') and c[3] == 'pass / identical':
            out.append(row(c[0], c[1], 'T0', 'skip', c[4], hashes.get(c[4], digest),
                           emulated=c[2] == 'emulated'))
    return out


def olap(text, digest):
    out = []
    for c in tables(text):
        if len(c) != 9 or c[0] not in ('ClickHouse', 'Doris', 'StarRocks'):
            continue
        for suite, value in zip(('T0', 'T1', 'T2', 'T8'), c[3:7]):
            out.append(row(c[0].lower(), c[1].split()[0], suite, value, c[2], c[8],
                           'c5' if suite == 'T2' else None))
    return [r for r in out if r]


def family_acceptance(text, digest):
    refs = {c[1]: (c[0], c[2]) for c in tables(text) if len(c) == 3 and SHA.fullmatch(c[2])}
    out = []
    for c in tables(text):
        if len(c) != 8 or c[0] not in refs:
            continue
        name = c[0]
        engine, version = next((e, name[len(prefix):]) for e, prefix in
                               [('mariadb', 'mariadb-'), ('tidb', 'pingcap-tidb-v'),
                                ('oceanbase', 'oceanbase-oceanbase-ce-')] if name.startswith(prefix))
        run, sha = refs[name]
        for suite, value in zip(('T0', 'T1', 'T8'), (c[1], c[2], c[5])):
            out.append(row(engine, version, suite, value, run, sha))
        # A cycle count is not an explicit verdict: do not promote T2.
    return [r for r in out if r]


def adapter_tables(path, text, digest):
    """Historical component checks stay skip at incomplete product boundaries."""
    out = []
    if path == 'testing/engines/mysql/SUMMARY.md':
        for c in tables(text):
            if len(c) == 6 and re.match(r'^\d+\.\d+\.\d+,', c[0]):
                version = c[0].split(',')[0]
                if verdict(c[2]):
                    out.append(row('mysql', version, 'T0', 'skip', c[1], digest, emulated='amd64' in c[0]))
    if path == 'testing/engines/postgres/RESULTS.md':
        run = re.search(r'--run-id ([\w-]+)', text)
        for c in tables(text):
            if len(c) == 6 and c[0].startswith('postgres:') and run:
                version = c[0].split(':')[1].removesuffix('-bookworm')
                hash_match = re.search(r'`pg' + version.split('.')[0] + r'/SHA256SUMS.json`: `([0-9a-f]{64})`', text)
                sha = hash_match[1] if hash_match else digest
                if verdict(c[1]):
                    out.append(row('postgres', version, 'T0', 'skip', run[1], sha))
                if c[2].startswith('pass ('):
                    out.append(row('postgres', version, 'T1', 'skip', run[1], sha))
    return [r for r in out if r]


def sqlserver(text, digest, data):
    """Join explicit validation verdicts to version-specific raw hashes."""
    aliases = dict(re.findall(r'- ([A-D]): `([^`]+)`', text))
    records = {r['product_version']: r for r in data['matrix']}
    out = []
    for c in tables(text):
        if len(c) != 5 or c[1] not in records:
            continue
        rec = records[c[1]]
        for column, suites in ((2, ('T0', 'T1', 'T2', 'T8')), (3, ('T7',))):
            match = re.match(r'(pass|fail), run ([A-D])\b', c[column])
            if not match:
                continue
            run = aliases[match[2]]
            for suite in suites:
                name = 'audit-report.json' if suite == 'T7' else 'product-suites.json'
                sha = rec['artifacts'].get(f'evidence/{run}/sqlserver-{rec["version"]}/{name}', digest)
                variants = ('c1', 'c5') if suite == 'T2' else ('c1',) if suite == 'T7' else (None,)
                for variant in variants:
                    out.append(row('sqlserver', c[1], suite, match[1], run, sha, variant, 'emulation' in c[4]))
    return out


def s5_validation(text, digest):
    # Smoke success is not T7 product success. Only the explicitly failed
    # no-op hook run has a suite verdict and version in this summary.
    match = re.search(r'PostgreSQL (\d+) run.*?T7 fails.*?`(h-s5-[^`]+)`', text, re.S)
    return [row('postgres', match[1], 'T7', 'fail', match[2], digest)] if match else []


MARKDOWN = {
    'tools/oo-test/PINS-VALIDATION.md': pins,
    'tools/oo-test/VALIDATION-M-S1E.md': mysql_t1,
    'tools/oo-test/S5-VALIDATION.md': s5_validation,
    'tools/oo-test/VALIDATION.md': fixture_t0,
    'testing/engines/olap/SUMMARY.md': olap,
    'testing/engines/mariadb/C-S3c-SUMMARY.md': family_acceptance,
}


def committed(root):
    names = subprocess.check_output(['git', 'ls-files', '-z', '*evidence*.json', '*SUMMARY*.json',
                                     '*RESULTS*.json', '*VALIDATION*.json', 'tools/oo-test/*.md',
                                     'testing/engines/**'], cwd=root).decode().split('\0')
    for path in sorted(set(names) - {INDEX, ''}):
        if not path.endswith(('.json', '.md')) or path.startswith('tools/evidence-index/'):
            continue
        if any(part in path.split('/') for part in ('fixtures', 'examples', 'contracts')):
            continue
        # Read the committed blob, not dirty summaries or git-ignored raw results.
        content = subprocess.check_output(['git', 'show', f'HEAD:{path}'], cwd=root)
        yield path, content


def collect(root):
    blobs = dict(committed(root))
    rows, inventory = [], []
    for path, content in blobs.items():
        digest = hashlib.sha256(content).hexdigest()
        text = content.decode()
        if path.endswith('.json'):
            data = json.loads(text)
            found = explicit_rows(data) + json_suites(path, data, digest)
        elif path in MARKDOWN:
            found = MARKDOWN[path](text, digest)
        elif path == 'testing/engines/sqlserver/S3B-VALIDATION.md':
            found = sqlserver(text, digest, json.loads(blobs['testing/engines/sqlserver/S3B-RESULTS.json']))
        else:
            found = adapter_tables(path, text, digest)
        rows.extend(found)
        inventory.append({'source': path, 'summary_sha256': digest, 'rows': len(found),
                          'reason': 'explicit verdicts' if found else 'no supported explicit suite/version/result record'})
    unique = {json.dumps(r, sort_keys=True): r for r in rows}
    rows = sorted(unique.values(), key=lambda r: (r['target'], r['version'], r['suite'], r['variant'] or '', r['run_id'], r['result'], r['sha256']))
    # An ambiguous equal run_id would otherwise depend on input ordering.
    seen = {}
    for r in rows:
        key = tuple(r[k] for k in ('target', 'version', 'suite', 'variant', 'run_id'))
        if key in seen and seen[key] != r['result']:
            raise ValueError(f'conflicting verdicts: {key}')
        seen[key] = r['result']
    return rows, inventory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--report', type=Path, help='write collection provenance inventory')
    args = parser.parse_args()
    rows, inventory = collect(ROOT)
    text = json.dumps(rows, indent=2) + '\n'
    if args.check:
        if not (ROOT / INDEX).exists() or (ROOT / INDEX).read_text() != text:
            parser.exit(1, 'FAIL: evidence index stale; run collect.py\n')
    else:
        (ROOT / INDEX).write_text(text)
    if args.report:
        args.report.write_text(json.dumps(inventory, indent=2) + '\n')
    print(f'PASS: {len(rows)} rows from {sum(bool(x["rows"]) for x in inventory)} committed summaries; {len(inventory)} files inspected')


if __name__ == '__main__':
    main()
