"""Check exact Gateway column access without returning any business rows."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def expected_columns(facts):
    result = {}
    for table in facts['tables']:
        name = table['dataset']
        if name in result or len(name.split('.')) != 2 or any(not p for p in name.split('.')):
            raise ValueError('audit_dataset_identity')
        columns = [c['name'] for c in table['columns']]
        if not columns or len(set(columns)) != len(columns):
            raise ValueError('audit_column_identity')
        result[name] = set(columns)
    if not result:
        raise ValueError('audit_empty_scope')
    return result


def inspect_table(dataset, expected, invoke):
    quote = lambda name: '"' + name.replace('"', '""') + '"'
    sql = 'SELECT * FROM ' + '.'.join(quote(p) for p in dataset.split('.')) + ' LIMIT 0'
    code, result = invoke(['query', '--json', sql])
    if code or result.get('error') or result.get('code'):
        return {'dataset': dataset, 'passed': False, 'reason': 'gateway_query_failed',
                'error_code': result.get('code', 'CLI_FAILURE')}
    actual = [c.get('name') for c in result.get('columns', []) if isinstance(c, dict)]
    empty = result.get('rows') == [] and result.get('returned_rows') == 0
    complete = result.get('truncated') is False
    query_id = result.get('query_id')
    passed = (empty and complete and set(actual) == expected and len(actual) == len(expected)
              and isinstance(query_id, str) and query_id.startswith('q_'))
    return {'dataset': dataset, 'passed': passed, 'expected_columns': len(expected),
            'returned_columns': len(actual), 'missing': sorted(expected - set(actual)),
            'unexpected': sorted(set(actual) - expected, key=str),
            'zero_business_rows': empty, 'untruncated': complete, 'query_id': query_id,
            'policy_version': result.get('policy_version'),
            'knowledge_version': result.get('knowledge_version')}


def run(config, output):
    output = Path(output).resolve()
    if any((p / '.git').exists() for p in (output, *output.parents)):
        raise ValueError('audit_output_must_be_private')
    raw = Path(config['facts']['path']).read_bytes()
    if hashlib.sha256(raw).hexdigest() != config['facts']['sha256']:
        raise ValueError('audit_facts_digest')
    expected = expected_columns(json.loads(raw))
    command = config['cli_command']
    if not isinstance(command, list) or not command or any(not isinstance(a, str) for a in command):
        raise ValueError('audit_cli_command')
    output.mkdir(mode=0o700, parents=True, exist_ok=False)

    def invoke(args):
        try:
            process = subprocess.run(command + args, capture_output=True, text=True, timeout=45)
            result = json.loads(process.stdout or process.stderr)
            return process.returncode, result if isinstance(result, dict) else {'error': True}
        except (OSError, ValueError, subprocess.TimeoutExpired):
            return 1, {'code': 'CLI_FAILURE'}

    records = []
    for index, (name, columns) in enumerate(sorted(expected.items()), 1):
        record = inspect_table(name, columns, invoke)
        records.append(record)
        (output / f'{index:04d}.json').write_text(json.dumps(record, ensure_ascii=False))
    versions = {(r.get('policy_version'), r.get('knowledge_version')) for r in records}
    all_passed = all(r['passed'] for r in records) and len(versions) == 1 and all(next(iter(versions)))
    summary = {'protocol': 'oo.gateway.access-audit/v1', 'passed': all_passed,
               'facts_sha256': config['facts']['sha256'], 'tables': len(records),
               'columns': sum(map(len, expected.values())),
               'passed_tables': sum(r['passed'] for r in records),
               'versions': [list(v) for v in sorted(versions, key=str)],
               'same_version': len(versions) == 1,
               'business_accepted': False, 'checks': 'SELECT * LIMIT 0 via CLI'}
    (output / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    summary = run(json.loads(Path(args.config).read_text()), args.output)
    print(json.dumps(summary))
    raise SystemExit(0 if summary['passed'] else 2)
