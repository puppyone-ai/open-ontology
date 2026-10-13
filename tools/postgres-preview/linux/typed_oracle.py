#!/usr/bin/env python3
"""Compare native PG simple-query text with Pack-mediated typed cells, exactly."""
import json
import sys
from pathlib import Path
from decimal import Decimal
from fractions import Fraction
from datetime import datetime

KINDS = {16:'bool',17:'bytea',20:'int8',21:'int2',23:'int4',25:'text',114:'json',700:'float4',701:'float8',1042:'bpchar',1043:'varchar',1082:'date',1114:'timestamp',1184:'timestamptz',1700:'numeric',2950:'uuid'}

def value_matches(source, cell, oid):
    if source is None:
        return cell == {'kind':'null'}
    if cell.get('kind') != KINDS.get(oid):
        return False
    kind = cell['kind']
    if kind == 'numeric':
        coefficient = int(cell['coefficient'])
        if cell['negative']:
            coefficient = -coefficient
        return Fraction(Decimal(source)) == Fraction(coefficient, 10 ** int(cell['scale']))
    if kind == 'bytea':
        return source.startswith('\\x') and bytes.fromhex(source[2:]) == bytes.fromhex(cell['value'])
    if kind in ('timestamptz','timestamp'):
        return datetime.fromisoformat(source) == datetime.fromisoformat(cell['value'].replace('Z','+00:00'))
    if kind == 'bool':
        return source in ('t','f') and cell['value'] is (source == 't')
    if kind.startswith('int'):
        return int(source) == int(cell['value'])
    if kind.startswith('float'):
        return float(source) == float(cell['value'])
    if kind == 'json':
        return json.loads(source,parse_float=Decimal) == json.loads(cell['value'],parse_float=Decimal)
    return source == cell['value']

def check(out):
    observations = json.loads((out/'source-table-oracle.json').read_text())['queries']
    catalog = json.loads((out/'stages/catalog-import.stdout').read_text())
    types = {f"{t['logical_schema']}.{t['logical_table']}":[c['oid'] for c in t['columns']] for t in catalog['tables']}
    summary = json.loads((out/'consumer-all/result.json').read_text())
    failures = []
    cells = 0
    for i, observation in enumerate(observations):
        if i >= len(summary['queries']):
            failures.append({'query':i,'reason':'query_not_completed'})
            continue
        if summary['queries'][i]['sql'] != observation['sql'] or summary['queries'][i]['dataset'] != observation['dataset']:
            failures.append({'query':i,'reason':'identity'})
            continue
        result = json.loads((out/f'consumer-all/table-{i}.stdout').read_text())
        rows = observation['rows']
        if result.get('truncated') or len(result.get('rows',[])) != len(rows):
            failures.append({'query':i,'reason':'row_count_or_truncation'})
            continue
        for r, (expected, actual) in enumerate(zip(rows,result['rows'])):
            oids = types[observation['dataset']]
            if len(expected) != len(actual) or len(actual) != len(oids):
                failures.append({'query':i,'row':r,'reason':'column_count'})
                continue
            for c,(s,v,oid) in enumerate(zip(expected,actual,oids)):
                cells += 1
                if not value_matches(s,v,oid):
                    failures.append({'query':i,'row':r,'column':c,'oid':oid,'reason':'typed_value'})
    report={'queries':len(observations),'cells':cells,'mismatches':failures,'source':'native PG read-only session before disconnect','consumer':'fixed Pack-mediated operator-preview after disconnect'}
    (out/'typed-table-oracle-result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(failures)

if __name__ == '__main__':
    sys.exit(check(Path(sys.argv[1])))
