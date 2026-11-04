#!/usr/bin/env python3
"""Print exact-version levels and missing P0 suites from the generated artifacts."""
import json

from collect import ROOT

CAPS = ('full', 'cdc', 'digest', 'schema_change', 'selfbuild', 'dialogue')
P0 = ('mysql57-selfmanaged', 'mysql8-selfmanaged', 'mysql9-selfmanaged',
      'postgres-selfmanaged', 'oracle-11g-xe', 'oracle-19c-selfmanaged',
      'oracle-21c-xe', 'oracle-23ai-selfmanaged')


def report(rows, matrix):
    lines = ['# Generated evidence report', '',
             'Levels are exactly those of the coordinator generator. `-` means no capability emitted.',
             'Versions remain exact: evidence never transfers between patches. `E` marks emulation.', '',
             '| Target | Version | full | cdc | digest | schema_change | selfbuild | dialogue | Execution |',
             '|---|---|---|---|---|---|---|---|---|']
    for item in matrix['targets']:
        caps = item['caps']
        levels = [caps.get(c, {}).get('level', '-') for c in CAPS]
        emulated = any(r['emulated'] for r in rows if (r['target'], r['version']) == (item['target'], item['version']))
        lines.append('| ' + ' | '.join([item['target'], item['version'], *levels, 'E' if emulated else '-']) + ' |')
    lines += ['', '## S5 P0 gaps', '',
              'Every observed P0 version is listed, plus untested Oracle 19c. PG 12 is excluded from P0.',
              'The family-only PG 17 failed probe is separate from 17.11. Missing means no row at all;',
              'skip/fail rows are NOT successful acceptance. The final column lists suites with no pass row.', '',
              '| Target | Version | No evidence row (T0-T8) | No pass row (T0-T8) |',
              '|---|---|---|---|']
    versions = {(r['target'], r['version']) for r in rows if r['target'] in P0
                and not (r['target'] == 'postgres-selfmanaged' and r['version'].startswith('12.'))}
    versions.add(('oracle-19c-selfmanaged', '19c'))
    for target, version in sorted(versions):
        selected = [r for r in rows if (r['target'], r['version']) == (target, version)]
        present = {r['suite'] for r in selected}
        passed = {r['suite'] for r in selected if r['result'] == 'pass'}
        missing = ', '.join(f'T{i}' for i in range(9) if f'T{i}' not in present) or '-'
        no_pass = ', '.join(f'T{i}' for i in range(9) if f'T{i}' not in passed) or '-'
        lines.append(f'| {target} | {version} | {missing} | {no_pass} |')
    return '\n'.join(lines) + '\n'


if __name__ == '__main__':
    print(report(json.loads((ROOT / 'execution/v0.2/evidence-index.json').read_text()),
                 json.loads((ROOT / 'docs/database-support/support-matrix.generated.json').read_text())), end='')
