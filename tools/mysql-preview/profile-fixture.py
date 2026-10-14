#!/usr/bin/env python3
"""Actual bounded aggregate profiles of the owned synthetic fixture; no samples.

The private client file selects the product reader with verified TLS.
This never selects a customer schema or changes the source.
"""
import argparse
import json
import re
import subprocess
from pathlib import Path

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--container', required=True)
    parser.add_argument('--client-file', required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--generation', type=int, default=1)
    args = parser.parse_args()
    if not args.container.startswith('oo-dl01w1-06-'):
        raise SystemExit('owned synthetic source required')
    def sql(text):
        return subprocess.check_output(['docker','exec',args.container,'mysql',
            '--defaults-file='+args.client_file,'--default-character-set=utf8mb4',
            '--batch','--raw','--skip-column-names','-e',text],text=True)
    schema = 'native_mysql_20261012'
    tables = []
    for table in ['hubs','shipments','tracking_events','range_gap','value_probe']:
        columns = sql(f"SELECT COLUMN_NAME,DATA_TYPE,COLUMN_TYPE,IS_NULLABLE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION").splitlines()
        fields = []
        for row in columns:
            name,kind,source_type,nullable = row.split('\t')
            if not re.fullmatch('[a-zA-Z_][a-zA-Z0-9_]*', name):
                raise SystemExit('unexpected synthetic column identifier')
            binary = kind in ('binary','varbinary','blob','tinyblob','mediumblob','longblob')
            minimum = f'HEX(MIN(`{name}`))' if binary else f'CAST(MIN(`{name}`) AS CHAR)'
            maximum = f'HEX(MAX(`{name}`))' if binary else f'CAST(MAX(`{name}`) AS CHAR)'
            value = json.loads(sql(f"SELECT JSON_OBJECT('rows',COUNT(*),'nulls',COUNT(*)-COUNT(`{name}`),'distinct',COUNT(DISTINCT BINARY `{name}`),'min',{minimum},'max',{maximum}) FROM `{schema}`.`{table}`"))
            assert 0 <= value['nulls'] <= value['rows'] and 0 <= value['distinct'] <= value['rows']
            value['null_count'] = value['nulls']
            value['distinct_count'] = value['distinct']
            fields.append(dict(name=name,source_type=source_type,nullable=nullable=='YES',
                               aggregation='full_unsampled',min_max_encoding='hex' if binary else 'source_text',**value))
        tables.append(dict(dataset=f'{schema}.{table}',schema=schema,table=table,rows=fields[0]['rows'],full_scan=True,columns=fields))
    args.out.write_text(json.dumps(dict(protocol='oo.source.mysql.profile/v1',source_id='mysql_native',
        source_generation=args.generation,samples='none',sampled=False,tables=tables),indent=2)+'\n')

if __name__ == '__main__':
    main()
