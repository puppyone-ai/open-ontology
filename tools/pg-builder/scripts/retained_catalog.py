"""Pinned declared metadata for a retained cut, never a source connection.

oo.builder.source-catalog/v1 carries source_id and tables in SourceTable shape:
schema/name, columns (name/not_null/comment), primary_key (name), foreign_keys
(name/columns in ForeignKeyColumn shape). Comments must be text or null, not
the irreversible UntrustedEvidence hashes in the legacy PostgreSQL catalog.
"""
import unicodedata

PROTOCOL = 'oo.builder.source-catalog/v1'

def unique(values, key, code):
    result = {key(value): value for value in values}
    if len(result) != len(values):
        raise ValueError(code)
    return result

def text_comment(value):
    if value is not None and not isinstance(value, str):
        raise ValueError('retained_source_comment_unavailable')
    return value or ''

def apply(tables, catalog, receipt, *, previous=None, stale_tables=()):
    # The loader receipt is authoritative for table names; decoding dataset
    # names alone would conflate encoded identifiers with literal q_ names.
    from retained_observation import original
    normalize = lambda name: unicodedata.normalize('NFC', name)
    if (catalog.get('protocol') != PROTOCOL
            or catalog.get('source_id') != receipt['source_id']):
        raise ValueError('retained_source_catalog_binding')
    sources = unique(catalog['tables'], lambda t: (t['schema'], t['name']),
                     'retained_source_table_mismatch')
    retained = unique(tables, lambda t: t['dataset'], 'retained_source_table_mismatch')
    published = unique(receipt['tables'], lambda t: (t['schema'], t['table']),
                       'retained_source_table_mismatch')
    old = {t['dataset']: t for t in (previous or {}).get('tables', [])}
    for identity, entry in published.items():
        if '.'.join(identity) not in stale_tables:
            continue
        # A stale cut carries the previous reviewed projection. Current source
        # DDL cannot invalidate its metadata or block unrelated fresh tables.
        prior = old.get(entry['storage_schema'] + '.' + entry['storage_table'])
        if prior is None:
            raise ValueError('retained_stale_origin_missing')
        foreign_keys = []
        for fk in prior.get('foreign_keys', []):
            schema, name = fk['referenced_dataset'].split('.', 1)
            foreign_keys.append(dict(name=fk['name'], columns=[
                dict(name=original(child), ref_schema=original(schema),
                     ref_table=original(name), ref_name=original(parent))
                for child, parent in zip(fk['columns'], fk['referenced_columns'])]))
        sources[identity] = dict(schema=identity[0], name=identity[1],
            comment=prior.get('table_comment'), foreign_keys=foreign_keys,
            primary_key=[dict(name=original(c)) for c in prior['primary_key']],
            columns=[dict(name=original(c['name']), not_null=not c['nullable'],
                          comment=c.get('comment')) for c in prior['columns']])
    if set(sources) != set(published):
        raise ValueError('retained_source_table_mismatch')
    mapped, column_names, declared_names = {}, {}, {}
    for identity, source in sources.items():
        entry = published[identity]
        dataset = entry['storage_schema'] + '.' + entry['storage_table']
        if dataset not in retained or dataset in mapped.values():
            raise ValueError('retained_source_table_mismatch')
        mapped[identity] = dataset
        table = retained[dataset]
        source_columns = unique(source['columns'], lambda c: normalize(c['name']),
                                'retained_source_column_mismatch')
        declared_names[identity] = set(source_columns)
        columns = unique(table['columns'], lambda c: normalize(original(c['name'])),
                         'retained_source_column_mismatch')
        if not set(columns) <= set(source_columns):
            raise ValueError('retained_source_column_mismatch')
        extra = sorted(set(source_columns) - set(columns))
        if extra:
            table['unpublished_columns'] = [source_columns[name] for name in extra]
        column_names[identity] = {name: column['name'] for name, column in columns.items()}
        table['table_comment'] = text_comment(source.get('comment'))
        for name, column in columns.items():
            declared = source_columns[name]
            if type(declared['not_null']) is not bool:
                raise ValueError('retained_source_nullability')
            column['nullable'] = not declared['not_null']
            column['comment'] = text_comment(declared.get('comment'))
        try:
            table['primary_key'] = [column_names[identity][normalize(k['name'])]
                                    for k in source['primary_key']]
        except KeyError:
            raise ValueError('retained_source_key_mismatch') from None
        if len(set(table['primary_key'])) != len(table['primary_key']):
            raise ValueError('retained_source_key_mismatch')
        if any(c['nullable'] for c in columns.values() if c['name'] in table['primary_key']):
            raise ValueError('retained_source_key_nullable')
        table['foreign_keys'] = []
        table['excluded_foreign_keys'] = []
    if set(mapped.values()) != set(retained):
        raise ValueError('retained_source_table_mismatch')
    for identity, source in sources.items():
        table = retained[mapped[identity]]
        unique(source['foreign_keys'], lambda fk: fk['name'], 'retained_source_fk_mismatch')
        for fk in source['foreign_keys']:
            pairs = fk['columns']
            targets = {(c['ref_schema'], c['ref_table']) for c in pairs}
            if len(targets) != 1:
                raise ValueError('retained_source_fk_mismatch')
            target = next(iter(targets))
            if (any(normalize(c['name']) not in declared_names[identity] for c in pairs)
                    or target in mapped and any(normalize(c['ref_name']) not in declared_names[target]
                                                for c in pairs)):
                raise ValueError('retained_source_fk_mismatch')
            if (any(normalize(c['name']) not in column_names[identity] for c in pairs)
                    or target in mapped and any(normalize(c['ref_name']) not in column_names[target]
                                                for c in pairs)):
                table['excluded_foreign_keys'].append(dict(
                    constraint=fk, reason='unreviewed_column'))
                continue
            try:
                child = [column_names[identity][normalize(c['name'])] for c in pairs]
                if target not in mapped:
                    table['excluded_foreign_keys'].append(dict(
                        constraint=fk, reason='referenced_table_outside_scope'))
                    continue
                parent = [column_names[target][normalize(c['ref_name'])] for c in pairs]
            except KeyError:
                raise ValueError('retained_source_fk_mismatch') from None
            if len(set(child)) != len(child) or len(set(parent)) != len(parent):
                raise ValueError('retained_source_fk_mismatch')
            table['foreign_keys'].append(dict(name=fk['name'], columns=child,
                referenced_dataset=mapped[target], referenced_columns=parent))
