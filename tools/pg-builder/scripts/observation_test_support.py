"""Synthetic evidence for unit tests, never a migration for retained artifacts."""
import library_observation as obs

def observed_fixture(tables):
    for table in tables:
        table.setdefault('physical', '"fixture"."t_' + table['dataset'].replace('.', '_') + '"')
        table.setdefault('ddl_sha256', 'fixture-ddl')
        for c in table['columns']:
            c.setdefault('physical_name', c['name'])
            rows, distinct = c['profile']['rows'], c['profile']['distinct']
            c['profile'] = obs.profiles({
                'rows': rows,
                'columns': [dict(name=c['name'], non_null=rows, distinct=distinct,
                                 min_length=1 if rows else None, max_length=3 if rows else None,
                                 blank_rows=0, digits_rows=0, leading_zero_rows=0)]
            }, [c['name']])[c['name']]
        table['observation'] = dict(
            protocol=obs.PROTOCOL, state='complete',
            expected_columns=[c['name'] for c in table['columns']],
            snapshot='fixture-snapshot', observed_at='fixture-time',
            binding={k: table[k] for k in
                     ['dataset', 'source_id', 'source_generation', 'target_generation', 'physical', 'ddl_sha256']})
    return tables
