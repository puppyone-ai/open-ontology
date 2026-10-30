"""Exact, aggregate-only independent arithmetic checks on a local imported snapshot."""
import argparse
from datetime import date
from decimal import Decimal
import json
import os
from pathlib import Path
import re

import library_metrics
import library_selfbuild as lib
from library_observation import quote, literal


def statements(facts, spec):
    if not isinstance(spec, dict) or set(spec) - {
            'metric', 'dataset', 'key', 'scope', 'time_column', 'period', 'kind', 'components'}:
        raise ValueError('measure_config')
    if not library_metrics.identifier(spec.get('metric')):
        raise ValueError('measure_identifier')
    tables = [t for t in facts['tables'] if t['dataset'] == spec.get('dataset')]
    if len(tables) != 1:
        raise ValueError('measure_dataset')
    table = tables[0]
    columns = {c['name']: c for c in table['columns']}
    key = spec.get('key')
    if key not in columns or table['primary_key'] != [key]:
        raise ValueError('measure_declared_single_key_required')
    scope = spec.get('scope')
    if (not isinstance(scope, dict) or not scope
            or any(k not in columns or not isinstance(v, str) or not v for k, v in scope.items())):
        raise ValueError('measure_scope_required')
    kind = spec.get('kind')
    if kind not in {'sum', 'count_distinct'}:
        raise ValueError('measure_kind')
    components = spec.get('components', [])
    if (kind == 'sum' and (not isinstance(components, list) or not 1 <= len(components) <= 8)
            or kind == 'count_distinct' and components):
        raise ValueError('measure_components')
    for component in components:
        if (not isinstance(component, dict) or set(component) != {'column', 'sign'}
                or type(component['sign']) is not int or component['sign'] not in {-1, 1}
                or component['column'] not in columns or not re.fullmatch(
                    r'(?:numeric|decimal)(?:\(\d+(?:,\s*\d+)?\))?|smallint|integer|bigint',
                    columns[component['column']]['data_type'])):
            raise ValueError('measure_exact_components_required')
    if len({c['column'] for c in components}) != len(components):
        raise ValueError('measure_duplicate_components')
    period, clock = spec.get('period'), spec.get('time_column')
    if clock is not None and (clock not in columns or columns[clock]['data_type'] not in {
            'date', 'timestamp', 'timestamp without time zone'}):
        raise ValueError('measure_clock_unconfirmed')
    if period is not None:
        if not isinstance(period, dict) or set(period) != {'start', 'end'} or clock is None:
            raise ValueError('measure_period')
        start, end = (date.fromisoformat(period[k]) for k in ('start', 'end'))
        if start >= end or start.isoformat() != period['start'] or end.isoformat() != period['end']:
            raise ValueError('measure_period')
    def where(physical):
        ref = lambda name: quote(columns[name]['physical_name'] if physical else name)
        terms = [ref(k) + '=' + literal(v) for k, v in sorted(scope.items())]
        if period:
            typ = 'DATE' if columns[clock]['data_type'] == 'date' else 'TIMESTAMP'
            terms.extend([ref(clock) + '>=' + typ + ' ' + literal(period['start']),
                          ref(clock) + '<' + typ + ' ' + literal(period['end'])])
        return ' AND '.join(terms)
    expression = lambda names: ' + '.join(
        ('-' if item['sign'] == -1 else '') + name for item, name in zip(components, names))
    logical = ('SELECT COUNT(DISTINCT ' + quote(key) + ')' if kind == 'count_distinct'
               else 'SELECT SUM(' + expression([quote(c['column']) for c in components]) + ')')
    logical += ' AS value FROM ' + table['dataset'] + ' WHERE ' + where(False)
    fields = [quote(columns[key]['physical_name']) + ' AS event_key']
    fields += [quote(columns[c['column']]['physical_name']) + ' AS c' + str(i)
               for i, c in enumerate(components)]
    if clock:
        fields.append(quote(columns[clock]['physical_name']) + ' AS event_time')
    raw = 'SELECT ' + ','.join(fields) + ' FROM ' + table['physical'] + ' WHERE ' + where(True)
    if kind == 'sum':
        names = ['c' + str(i) for i in range(len(components))]
        direct = 'SUM(' + expression(names) + ')'
        # Reconstruct the same linear measure from independently grouped components.
        sums = expression(['SUM(c' + str(i) + ' * n)' for i in range(len(components))])
        independent = 'SELECT ' + sums + ' AS value FROM (SELECT ' + ','.join(names)
        independent += ',COUNT(*) AS n FROM raw WHERE ' + ' AND '.join(n + ' IS NOT NULL' for n in names)
        independent += ' GROUP BY ' + ','.join(names) + ') grouped'
        nulls = 'COUNT(*) FILTER(WHERE ' + ' OR '.join(n + ' IS NULL' for n in names) + ')'
    else:
        direct = 'COUNT(DISTINCT event_key)'
        independent = 'SELECT COUNT(*) AS value FROM (SELECT event_key FROM raw WHERE event_key IS NOT NULL GROUP BY event_key) grouped'
        nulls = '0'
    summary = ['COUNT(*) AS records', 'COUNT(DISTINCT event_key) AS distinct_keys',
               'COUNT(*) FILTER(WHERE event_key IS NULL) AS null_keys', nulls + ' AS null_measure_rows',
               direct + '::text AS value']
    if clock:
        summary += ['MIN(event_time)::text AS first_event', 'MAX(event_time)::text AS last_event']
    sql = ('SELECT row_to_json(result) FROM (WITH raw AS MATERIALIZED (' + raw
           + '), direct AS (SELECT ' + ','.join(summary) + ' FROM raw), independent AS ('
           + independent + ') SELECT direct.*, independent.value::text AS independent_value,'
           + ' pg_current_snapshot()::text AS snapshot, statement_timestamp()::text AS observed_at'
           + ' FROM direct CROSS JOIN independent) result')
    return logical, sql


def reconcile(db, facts, spec):
    logical, sql = statements(facts, spec)
    result = db.query(sql)
    if (not isinstance(result, dict) or not result.get('snapshot') or not result.get('observed_at')
            or any(type(result.get(k)) is not int or result[k] < 0 for k in
                   ('records', 'distinct_keys', 'null_keys', 'null_measure_rows'))):
        raise ValueError('measure_result_incomplete')
    if (result['distinct_keys'] > result['records'] or result['null_keys'] > result['records']
            or result['null_measure_rows'] > result['records']):
        raise ValueError('measure_result_inconsistent')
    values = [result.get(k) for k in ('value', 'independent_value')]
    for value in values:
        if value is not None and (not isinstance(value, str) or not re.fullmatch(r'-?\d+(?:\.\d+)?', value)):
            raise ValueError('measure_exact_result_required')
    matched = (values[0] is None and values[1] is None
               or all(v is not None for v in values) and Decimal(values[0]) == Decimal(values[1]))
    report = {
        'protocol': 'oo.builder.measure-reconciliation/v1', 'metric': spec['metric'], 'config': spec,
        'facts_sha256': facts['facts_sha256'], 'definition_sql': logical,
        'definition_query_sha256': lib.digest(logical), 'independent_query_sha256': lib.digest(sql),
        'result': result, 'arithmetic_matches': matched,
        'key_unique_in_scope': result['distinct_keys'] == result['records'] and result['null_keys'] == 0,
        'coverage': 'local_imported_snapshot_only', 'same_oracle_statement_snapshot': True,
        'gateway_snapshot_comparable': False, 'source_completeness_claimed': False,
        'business_accepted': False,
    }
    report['evidence_sha256'] = lib.digest(report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    root = Path(__file__).resolve().parents[4]
    output = args.output.resolve()
    if output.is_relative_to(root) or output.exists():
        parser.error('output must be a new private directory outside the repository')
    path = Path(config['facts_input'])
    if path.is_symlink() or lib.hashlib.sha256(path.read_bytes()).hexdigest() != config['facts_sha256']:
        raise ValueError('measure_facts_digest')
    facts = json.loads(path.read_text())
    lib.require_complete_facts(facts)
    db = lib.ReadOnlyPostgres(config['psql_command'], config.get('statement_timeout_ms', 120000))
    lib.validate_retained_tables(db, facts['tables'], config['ddl_directory'])
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True)
    names = [s['metric'] for s in config['measures']]
    if len(names) != len(set(names)) or not 1 <= len(names) <= 100:
        raise ValueError('measure_inventory')
    results = []
    for spec in config['measures']:
        result = reconcile(db, facts, spec)
        lib.write(output / (spec['metric'] + '.json'), result)
        results.append({'metric': spec['metric'], 'arithmetic_matches': result['arithmetic_matches'],
                        'key_unique_in_scope': result['key_unique_in_scope'],
                        'evidence_sha256': result['evidence_sha256']})
    lib.validate_retained_tables(db, facts['tables'], config['ddl_directory'])
    lib.write(output / 'report.json', {'checks': results, 'business_accepted': False})
    print(lib.canonical({'checks': results, 'business_accepted': False}))


if __name__ == '__main__':
    main()
