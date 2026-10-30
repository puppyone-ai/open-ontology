"""Measure a published Oracle target using existing full-scan observations.

Reads only the supplied PostgreSQL target and pinned local source evidence.
Does not connect to Oracle, mint a seal, publish, infer source time, or call S5.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path

import library_observation as observation
import oracle_observation as oracle

PROTOCOL = 'oo.builder.oracle-target-observation/v1'
STATE_COLUMNS = {
    'source_watermark': (
        'source', 'consistency', 'consistency_grade', 'applied_pos', 'source_committed_at',
        'applied_at', 'state', 'cross_table_consistent', 'source_generation',
        'target_generation', 'position_codec', 'position_lineage', 'source_epoch'),
    'native_publication_receipt': (
        'run_id', 'receipt_sha256', 'source_id', 'source_generation', 'target_generation',
        'position_codec', 'applied_pos', 'consistency', 'consistency_grade',
        'cross_table_consistent', 'table_vector', 'published_at', 'publication_xid'),
    'full_import_run': (
        'run_id', 'source_id', 'source_generation', 'target_generation', 'config_digest',
        'type_digest', 'input_digest', 'schema_digest', 'state', 'sealed_at'),
    'source_instance': ('source', 'source_epoch', 'engine', 'measured_id', 'config_digest', 'schema_digest'),
    'table_quality': (
        'schema_name', 'table_name', 'rejects_open', 'reconcile_status',
        'partial_txn_in_progress', 'capture_method', 'delete_detection', 'position_state', 'budget_state'),
}


C5_STATE_COLUMNS = {
    'c5_oracle_binding': ('source', 'schema_name', 'table_name', 'source_schema', 'source_table',
                          'identity', 'baseline_run', 'snapshot_scn', 'manifest_sha256', 'key_ident'),
    'c5_block_receipt': ('source', 'schema_name', 'table_name', 'chunk_id', 'source_generation',
                         'scope', 'run_id', 'batch_id', 'payload_digest', 'target_generation'),
}

def state_sql(source_id, c5_run_ids=()):
    source = observation.literal(source_id)
    parts = []
    selected = dict(STATE_COLUMNS)
    if c5_run_ids:
        selected.pop('source_instance')
        selected.update(C5_STATE_COLUMNS)
    for name, columns in selected.items():
        if name == 'table_quality':
            where = ("exists(select 1 from _oo.table_freshness f where f.source=" + source
                     + " and f.schema_name=t.schema_name and f.table_name=t.table_name)")
        else:
            where = ('source_id' if name in {'full_import_run', 'native_publication_receipt'}
                     else 'source') + '=' + source
        if c5_run_ids and name in {'full_import_run', 'native_publication_receipt', 'c5_block_receipt'}:
            where += ' and run_id in (' + ','.join(observation.literal(run) + '::uuid'
                                                  for run in c5_run_ids) + ')'
        select = ','.join('t.' + observation.quote(c) for c in columns)
        parts.append(observation.literal(name) + ",(select coalesce(json_agg(row_to_json(r)),'[]'::json)"
                     + " from (select " + select + " from _oo." + name + " t where " + where
                     + " order by " + select + ") r)")
    return ("select json_build_object('database',current_database(),'principal',current_user,"
            "'read_only',current_setting('transaction_read_only'),'metadata',json_build_object("
            + ','.join(parts) + '))')


def one(rows, code):
    oracle.require(isinstance(rows, list) and len(rows) == 1, code)
    return rows[0]


def dictionary_binding(schema, dictionary, source_id):
    oracle.require(schema.get('protocol') == 'oo.oracle.inventory/v1'
                   and dictionary.get('protocol') == 'oo.oracle.source-catalog-observation/v1'
                   and schema.get('source_id') == dictionary.get('source_id') == source_id
                   and dictionary.get('engine') == 'oracle', 'cross_source')
    oracle.require(schema.get('metadata_observation', {}).get('observed_scn')
                   == dictionary['database']['observed_scn'], 'dictionary_observation_binding')
    for table in schema['tables']:
        measured = one([t for t in dictionary['tables']
                        if (t['owner'], t['table']) == (table['owner'], table['table'])],
                       'dictionary_table_binding')
        oracle.require(table['keys'] == measured['primary_key'], 'dictionary_primary_key')
        expected = {c['name']: c for c in measured['columns']}
        oracle.require(len(expected) == len(table['columns']), 'dictionary_column_binding')
        for c in table['columns']:
            d = expected.get(c['name'], {})
            oracle.require(all(c.get(a) == d.get(b) for a, b in (
                ('type', 'data_type'), ('precision', 'precision'), ('scale', 'scale'),
                ('nullable', 'nullable'), ('comment', 'comment'), ('bytes', 'data_length'),
                ('default_observation', 'default_observation'))), 'dictionary_column_binding')


def verify_state(config, catalog, schema, dictionary, publication, observed):
    """Validate observations, not the immutable catalog's pre-publication state."""
    entries = oracle.catalogs(catalog)
    proofs = [oracle.c5_binding(entry) for entry in entries]
    if any(proofs):
        oracle.require(all(proofs), 'mixed_capture_methods')
        return verify_c5_state(config, entries, proofs, schema, dictionary, publication, observed)
    oracle.require(observed.get('database') == config['target_database']
                   and observed.get('read_only') == 'on', 'target_session')
    oracle.require(publication.get('protocol') == 'oo.pipeline.native-c1-publication-result/v1'
                   and publication.get('publication', {}).get('outcome') == 'published',
                   'publication_result')
    receipt = publication['receipt']
    oracle.require(receipt.get('protocol') == 'oo.pipeline.native-publication-receipt/v1'
                   and publication.get('receipt_sha256') == oracle.digest(receipt),
                   'publication_digest')
    source_id = config['source_id']
    oracle.require(receipt.get('source_id') == source_id
                   and receipt.get('oracle_buffer', {}).get('source_identity') == dictionary['source_identity'],
                   'publication_source_identity')
    state = observed['metadata']
    watermark = one(state['source_watermark'], 'watermark_coverage')
    source = one(state['source_instance'], 'source_instance_coverage')
    oracle.require(source['source'] == watermark['source'] == source_id
                   and source['engine'] == 'oracle'
                   and source['measured_id'] == receipt['position_lineage']
                   and source['source_epoch'] == receipt['source_epoch']
                   and watermark['state'] == 'live', 'current_source_identity')
    for field in ('source_generation', 'target_generation', 'consistency', 'consistency_grade',
                  'cross_table_consistent', 'position_codec', 'position_lineage', 'source_epoch'):
        oracle.require(watermark[field] == receipt[field], 'current_watermark_drift')
    oracle.require(str(watermark['applied_pos']) == str(receipt['applied_pos']), 'current_position_drift')
    table_count = 0
    for entry in oracle.catalogs(catalog):
        oracle.require(entry['source_id'] == source_id and entry['engine'] == 'oracle', 'cross_source')
        run = one([r for r in state['full_import_run'] if r['run_id'] == entry['run_id']], 'run_coverage')
        # Loader run schema_digest is source/Arrow; catalog schema_digest is its
        # exported logical target preimage. Do not relabel either digest.
        for key in oracle.IDENTITY:
            if key != 'schema_digest':
                oracle.require(run[key] == entry[key], 'current_run_binding')
        oracle.require(run['state'] == 'published', 'current_run_unpublished')
        pub = one([p for p in state['native_publication_receipt']
                   if p['run_id'] == entry['run_id']], 'publication_coverage')
        oracle.require(pub['receipt_sha256'] == publication['receipt_sha256']
                       and pub['table_vector'] == receipt['tables'], 'current_publication_drift')
        for key in ('source_id', 'source_generation', 'target_generation', 'consistency',
                    'consistency_grade', 'cross_table_consistent', 'position_codec'):
            oracle.require(pub[key] == receipt[key], 'current_publication_drift')
        oracle.require(str(pub['applied_pos']) == str(receipt['applied_pos']), 'current_position_drift')
        for stored in entry['tables']:
            table_count += 1
            vector = one([v for v in receipt['tables'] if v['run_id'] == entry['run_id']
                          and v['physical_name'] == stored['stored_physical']], 'publication_table_coverage')
            oracle.require((vector['storage_schema'], vector['storage_table']) ==
                           (stored['logical_schema'], stored['logical_table'])
                           and vector['accepted_rows'] == stored['accepted_rows']
                           and vector['canonical_digest'] == stored['canonical_digest']
                           and not vector['rejects'], 'publication_table_binding')
            native = one([t for t in schema['tables'] if (t['owner'], t['table']) ==
                          (vector['schema'], vector['table'])], 'publication_source_table')
            oracle.bind_catalog_columns(native['columns'], stored['columns'])
            fresh = one([f for f in observed['freshness']
                         if f['physical_name'] == stored['stored_physical']], 'freshness_coverage')
            oracle.require(fresh['state'] == 'live' and fresh['source'] == source_id
                           and fresh['source_generation'] == entry['source_generation']
                           and fresh['target_generation'] == entry['target_generation']
                           and str(fresh['last_pos']) == str(receipt['applied_pos']),
                           'current_generation_drift')
            quality = one([q for q in state['table_quality'] if
                           (q['schema_name'], q['table_name']) ==
                           (stored['logical_schema'], stored['logical_table'])], 'quality_coverage')
            oracle.require(quality['rejects_open'] == 0 and not quality['partial_txn_in_progress']
                           and quality['position_state'] == 'ok' and quality['budget_state'] == 'ok',
                           'target_quality')
    oracle.require(table_count == len(receipt['tables']), 'publication_table_coverage')

def verify_c5_state(config, entries, proofs, schema, dictionary, receipt, observed):
    oracle.require(observed.get('database') == config['target_database']
                   and observed.get('read_only') == 'on', 'target_session')
    access = config.get('profile_access', 'internal_aggregate_api')
    oracle.require(access in {'internal_aggregate_api', 'internal_owner_readonly'}, 'c5_profile_access')
    if access == 'internal_owner_readonly':
        oracle.require(observed.get('principal') == 'oo_target_owner', 'c5_profile_principal')
    else:
        oracle.require(observed.get('principal') not in (None, 'oo_target_owner', 'postgres'),
                       'c5_profile_principal')
    oracle.require(receipt.get('protocol') == 'oo.pipeline.native-publication-receipt/v1'
                   and receipt.get('source_id') == config['source_id']
                   and receipt.get('collection_mode') == 'per_table'
                   and receipt.get('consistency') == 'per_table'
                   and receipt.get('consistency_grade') == 'S0'
                   and receipt.get('cross_table_consistent') is False
                   and receipt.get('position_codec') == 'snapshot-zero/v1'
                   and receipt.get('position_lineage') == 'oracle.snapshot'
                   and receipt.get('applied_pos') == '0'
                   and not any(k in receipt for k in ('oracle_buffer', 'source_instance', 'source_epoch')),
                   'c5_publication_result')
    publication_sha = oracle.digest(receipt)
    oracle.require(all(p['publication_receipt_sha256'] == publication_sha for p in proofs),
                   'publication_digest')
    state = observed['metadata']
    oracle.require('source_instance' not in state, 'c5_source_instance_forbidden')
    watermark = one(state['source_watermark'], 'watermark_coverage')
    oracle.require(watermark['source'] == config['source_id'] and watermark['state'] == 'live',
                   'current_source_identity')
    for key in ('source_generation', 'target_generation', 'consistency', 'consistency_grade',
                'cross_table_consistent', 'position_codec', 'position_lineage'):
        oracle.require(watermark[key] == receipt[key], 'current_watermark_drift')
    oracle.require(str(watermark['applied_pos']) == '0', 'current_position_drift')
    oracle.require(len(receipt['tables']) == len(entries), 'publication_table_coverage')
    for entry, proof in zip(entries, proofs):
        stored, scope = entry['tables'][0], proof['scope']
        snapshot = scope['oracle_snapshot']
        oracle.require(entry['source_id'] == config['source_id'], 'cross_source')
        run = one([v for v in state['full_import_run'] if v['run_id'] == entry['run_id']], 'run_coverage')
        oracle.require(run['state'] == 'published' and all(run[k] == entry[k]
                       for k in oracle.IDENTITY if k != 'schema_digest'), 'current_run_binding')
        oracle.require(run['schema_digest'] == scope['schema_fingerprint'], 'c5_source_schema_binding')
        pub = one([v for v in state['native_publication_receipt'] if v['run_id'] == entry['run_id']],
                  'publication_coverage')
        oracle.require(pub['receipt_sha256'] == publication_sha and pub['table_vector'] == receipt['tables'],
                       'current_publication_drift')
        for key in ('source_id', 'source_generation', 'target_generation', 'consistency',
                    'consistency_grade', 'cross_table_consistent', 'position_codec'):
            oracle.require(pub[key] == receipt[key], 'current_publication_drift')
        oracle.require(str(pub['applied_pos']) == '0', 'current_position_drift')
        vector = one([v for v in receipt['tables'] if v['run_id'] == entry['run_id']
                      and v['physical_name'] == stored['stored_physical']], 'publication_table_coverage')
        oracle.require((vector['schema'], vector['table']) ==
                       (stored['logical_schema'], stored['logical_table'])
                       and vector['accepted_rows'] == proof['effective_rows']
                       and vector['canonical_digest'] == stored['canonical_digest']
                       and vector.get('capture_method') == 'c5_diff'
                       and vector.get('delete_detection') == 'reconcile'
                       and vector.get('consistency_grade') == 'S0' and not vector['rejects'],
                       'publication_table_binding')
        oracle.require(vector.get('source_generation') == entry['source_generation']
                       and vector.get('target_generation') == entry['target_generation']
                       and str(vector.get('applied_pos')) == '0', 'publication_table_binding')
        native = one([v for v in schema['tables'] if (v['owner'], v['table']) ==
                      (scope['schema'], scope['table'])], 'publication_source_table')
        oracle.bind_catalog_columns(native['columns'], stored['columns'])
        target_source = one([v for v in state['c5_oracle_binding'] if
            (v['schema_name'], v['table_name']) == (stored['logical_schema'], stored['logical_table'])],
            'c5_source_binding')
        oracle.require(target_source['source'] == config['source_id']
                       and target_source['identity'] == snapshot['identity'] == schema.get('identity')
                       and str(target_source['snapshot_scn']) == snapshot['scn']
                       and target_source['manifest_sha256'] == snapshot['manifest_sha256']
                       and (target_source['source_schema'], target_source['source_table']) ==
                           (scope['schema'], scope['table']), 'c5_source_binding')
        oracle.require(len(native['keys']) == 1
                       and target_source['key_ident'] == oracle.logical(native['keys'][0]),
                       'c5_source_key_binding')
        identity = dictionary['source_identity']
        expected_identity = '|'.join(str(identity[k]) for k in
                                    ('dbid', 'resetlogs_change', 'pdb_guid', 'pdb_name', 'db_unique_name'))
        oracle.require(identity.get('engine') == 'oracle'
                       and snapshot['identity'].rsplit('|', 1)[0] == expected_identity,
                       'publication_source_identity')
        block = one([v for v in state['c5_block_receipt'] if v['run_id'] == entry['run_id']],
                    'c5_block_binding')
        mapped_scope = dict(scope, schema=stored['logical_schema'], table=stored['logical_table'])
        oracle.require(block['source'] == config['source_id'] and block['scope'] == mapped_scope
                       and block['source_generation'] == entry['source_generation']
                       and block['target_generation'] == entry['target_generation']
                       and block['chunk_id'] == scope['chunk_id'], 'c5_block_binding')
        fresh = one([v for v in observed['freshness'] if v['physical_name'] == stored['stored_physical']],
                    'freshness_coverage')
        oracle.require(fresh['source'] == config['source_id'] and fresh['state'] == 'live'
                       and fresh['source_generation'] == entry['source_generation']
                       and fresh['target_generation'] == entry['target_generation']
                       and str(fresh['last_pos']) == '0', 'current_generation_drift')
        quality = one([q for q in state['table_quality'] if (q['schema_name'], q['table_name']) ==
                       (stored['logical_schema'], stored['logical_table'])], 'quality_coverage')
        oracle.require(quality['rejects_open'] == 0 and not quality['partial_txn_in_progress']
                       and quality['position_state'] == 'ok' and quality['budget_state'] == 'ok'
                       and quality['capture_method'] == 'c5_diff'
                       and quality['delete_detection'] == 'reconcile', 'target_quality')

def receipt_of(catalog, publication):
    return publication if all(oracle.c5_binding(e) for e in oracle.catalogs(catalog)) else publication['receipt']


def validate_evidence(config, docs):
    import oracle_temporal_evidence
    publication = oracle.read_pin(config.get('publication'))
    dictionary = oracle.read_pin(config.get('source_catalog'))
    schema, profile, semantics = (docs[k] for k in ('source_schema', 'profile', 'semantics'))
    dictionary_binding(schema, dictionary, config['source_id'])
    oracle.require(schema['metadata_observation']['dictionary']['sha256']
                   == config['source_catalog']['sha256'], 'dictionary_pin_binding')
    pins = {k: config[k]['sha256'] for k in ('catalog', 'source_schema', 'source_catalog', 'publication')}
    oracle.require(semantics.get('input_sha256') == pins
                   and semantics.get('profile_sha256') == config['profile']['sha256'],
                   'target_observation_binding')
    oracle.require(semantics.get('measurement_code_sha256') ==
                   hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'measurement_code_drift')
    oracle.require(semantics.get('measurement_sha256') == oracle.digest(
        {k: v for k, v in semantics.items() if k != 'measurement_sha256'}), 'target_observation_digest')
    verify_state(config, docs['catalog'], schema, dictionary, publication, semantics['target_observation'])
    oracle.require(semantics['consistency_grade'] ==
                   semantics['target_observation']['metadata']['source_watermark'][0]['consistency_grade']
                   and semantics['source_metadata_observation'] == dictionary['database']
                   and semantics['source_catalog_observation'] == dictionary
                   and semantics['publication_receipt'] == receipt_of(docs['catalog'], publication)
                   and semantics.get('source_read') is False
                   and semantics.get('business_semantics_approved') is False
                   and semantics.get('official_business_dictionary') == 'missing'
                   and semantics.get('model_calls') == 0, 'semantics_projection_binding')
    oracle.require(profile.get('profile_origin') == 'read_only_published_target', 'profile_origin')
    oracle.require(semantics['source_clock'] == {
        'source_committed_at': semantics['target_observation']['metadata']['source_watermark'][0]['source_committed_at'],
        'basis': '_oo.source_watermark', 'metadata_scn_is_row_clock': False},
        'source_clock_binding')
    proofs = [oracle.c5_binding(e) for e in oracle.catalogs(docs['catalog'])]
    if any(proofs):
        oracle.require(semantics.get('capture_method') == 'c5_diff'
                       and semantics.get('delete_detection') == 'reconcile'
                       and semantics.get('oracle_c5_bindings') == proofs
                       and semantics.get('profile_access') == profile.get('profile_access')
                       == config.get('profile_access', 'internal_aggregate_api')
                       and semantics.get('agent_read_accepted') is False,
                       'c5_semantics_projection')
    for table in profile['tables']:
        columns = table['columns']
        observation.profiles(dict(rows=table['rows'], columns=columns), [c['name'] for c in columns])
        oracle.require(bool(table.get('snapshot')) and bool(table.get('observed_at')), 'profile_snapshot')
    temporal = oracle_temporal_evidence.verify(config, schema, publication)
    if temporal is not None:
        oracle.require(semantics.get('temporal_capture_proof') == temporal
                       and semantics.get('temporal_policy') == temporal['temporal_policy'],
                       'temporal_capture_projection')
    else:
        oracle.require('temporal_capture_proof' not in semantics, 'temporal_capture_projection')
    return temporal


def pin(path):
    return dict(path=str(Path(path).resolve()), sha256=hashlib.sha256(Path(path).read_bytes()).hexdigest())


def write(path, value):
    with Path(path).open('x') as stream:
        stream.write(json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':')) + '\n')


def measure(db, config, output):
    import library_selfbuild as lib
    import oracle_temporal_evidence
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    docs = {k: oracle.read_pin(config.get(k)) for k in ('catalog', 'source_schema', 'source_catalog', 'publication')}
    catalog, schema, dictionary, publication = (docs[k] for k in docs)
    temporal = oracle_temporal_evidence.verify(config, schema, publication)
    dictionary_binding(schema, dictionary, config['source_id'])
    oracle.require(schema['metadata_observation']['dictionary']['sha256'] == config['source_catalog']['sha256'],
                   'dictionary_pin_binding')
    entries = oracle.catalogs(catalog)
    proofs = [oracle.c5_binding(entry) for entry in entries]
    oracle.require(not any(proofs) or all(proofs), 'mixed_capture_methods')
    sql = state_sql(config['source_id'], [e['run_id'] for e in entries] if any(proofs) else ())
    protected = any(proofs) and config.get('profile_access', 'internal_aggregate_api') == 'internal_aggregate_api'
    if protected:
        oracle.require(len(entries) == 1, 'c5_observation_table')
        sql = oracle.observation_state_query(entries[0])

    def current():
        metadata = db.query(lib.METADATA_SQL)
        state = db.query(sql)
        state['freshness'] = [f for f in metadata['freshness'] or [] if f['source'] == config['source_id']]
        verify_state(config, catalog, schema, dictionary, publication, state)
        return metadata, state

    before_meta, before = current()
    write(output / 'target-before.json', before)
    write(output / 'metadata-before.json', before_meta)
    write(output / 'queries.json', {'metadata': lib.METADATA_SQL, 'state': sql})
    tables = []
    for entry in entries:
        for stored in entry['tables']:
            dataset = stored['logical_schema'] + '.' + stored['logical_table']
            raw = one([t for t in schema['tables'] if oracle.logical(t['owner']) + '.' +
                       oracle.logical(t['table']) == dataset], 'source_table_binding')
            native = oracle.bind_catalog_columns(raw['columns'], stored['columns'])
            fresh = one([f for f in before['freshness'] if f['physical_name'] == stored['stored_physical']],
                        'freshness_coverage')
            mappings = lib.active_column_mappings(before_meta, fresh)
            oracle.require(len(mappings) == len(stored['columns']), 'current_mapping_drift')
            for c in stored['columns']:
                mapped = one([m for m in mappings if m['physical_name'] == c['stored_physical']],
                             'current_mapping_drift')
                oracle.require(mapped['logical_column'] == oracle.logical(native[c['logical_name']]['name'])
                               and mapped['scope_namespace_physical'] == stored['stored_namespace']
                               and mapped['config_digest'] == entry['config_digest']
                               and mapped['run_generation'] == entry['source_generation'], 'current_mapping_drift')
                attr = one([a for a in before_meta['attributes'] if a['nspname'] == '_oo'
                            and a['relname'] == stored['stored_physical']
                            and a['attname'] == c['stored_physical']], 'current_type_drift')
                oracle.require((attr['oid'], attr['typmod']) == (c['oid'], c['typmod']), 'current_type_drift')
                native_column = native[c['logical_name']]
                proof = ((temporal or {}).get('columns', {}).get(
                    raw['owner'] + '.' + raw['table'] + '.' + native_column['name']))
                oracle.check_target(native_column, c,
                                    {'temporal_policy': (temporal or {}).get('temporal_policy', {})}, proof)
            columns = [dict(name=c['logical_name'], physical_name=c['stored_physical']) for c in stored['columns']]
            query = observation.profile_sql('"_oo".' + observation.quote(stored['stored_physical']), columns)
            if entry.get('oracle_c5_binding') is not None:
                query = oracle.profile_query(dict(entry, columns=stored['columns'],
                    profile_access=config.get('profile_access', 'internal_aggregate_api')), query)
            measured = db.query(query)
            observation.profiles(measured, [c['name'] for c in columns])
            oracle.require(measured['rows'] == stored['accepted_rows'], 'target_row_drift')
            write(output / ('profile-query-' + str(len(tables)) + '.json'), {'sql': query, 'result': measured})
            table_profile = dict(dataset=dataset, full_scan=True,
                                 **{k: entry[k] for k in oracle.IDENTITY}, **measured)
            table_profile['columns'] = [dict(c, source_name=native[c['name']]['name'],
                                       source_type=native[c['name']]['type']) for c in measured['columns']]
            tables.append(table_profile)
    after_meta, after = current()
    write(output / 'target-after.json', after)
    write(output / 'metadata-after.json', after_meta)
    oracle.require(before == after and before_meta == after_meta, 'target_observation_drift')
    for key, doc in docs.items():
        oracle.require(oracle.read_pin(config[key]) == doc, 'input_drift')
    common = dict(source_id=config['source_id'], source_generation=max(e['source_generation'] for e in entries),
                  target_generation=max(e['target_generation'] for e in entries),
                  generation_semantics='per_dataset_binding_vector')
    if catalog['protocol'] == oracle.COLLECTION:
        common.update(collection_sha256=config['catalog']['sha256'],
                      binding_vector_sha256=oracle.digest(catalog['binding_vector']),
                      binding_vector=catalog['binding_vector'])
    else:
        common.update({k: catalog[k] for k in oracle.IDENTITY})
    profile = dict(protocol='oo.source.oracle.profile/v1', engine='oracle', sampled=False,
                   samples='none', profile_origin='read_only_published_target', **common, tables=tables)
    if any(proofs):
        profile['profile_access'] = config.get('profile_access', 'internal_aggregate_api')
    write(output / 'profile.json', profile)
    watermark = before['metadata']['source_watermark'][0]
    semantics = dict(protocol='oo.source.oracle.audit/v1', engine='oracle', **common,
                     measurement_protocol=PROTOCOL, target_observation=before,
                     measurement_code_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                     input_sha256={k: config[k]['sha256'] for k in docs},
                     profile_sha256=pin(output / 'profile.json')['sha256'],
                     source_metadata_observation=dictionary['database'],
                     source_catalog_observation=dictionary, publication_receipt=receipt_of(catalog, publication),
                     consistency_grade=watermark['consistency_grade'],
                     source_clock=dict(source_committed_at=watermark['source_committed_at'],
                                       basis='_oo.source_watermark', metadata_scn_is_row_clock=False),
                     source_read=False, business_semantics_approved=False,
                     official_business_dictionary='missing', model_calls=0)
    if temporal is not None:
        semantics.update(temporal_capture_proof=temporal, temporal_policy=temporal['temporal_policy'])
    if any(proofs):
        semantics.update(capture_method='c5_diff', delete_detection='reconcile', oracle_c5_bindings=proofs,
                         profile_access=config.get('profile_access', 'internal_aggregate_api'),
                         agent_read_accepted=False)
    semantics['measurement_sha256'] = oracle.digest(semantics)
    write(output / 'semantics.json', semantics)
    result = {k: config[k] for k in ('source_id', 'target_database', 'catalog', 'source_schema',
                                    'source_catalog', 'publication')}
    result.update(profile=pin(output / 'profile.json'), semantics=pin(output / 'semantics.json'))
    for key in ('business_dictionary', 'require_business_dictionary', 'profile_access', 'temporal_capture'):
        if key in config:
            result[key] = config[key]
    oracle.load(result)
    write(output / 'oracle-input.json', result)
    return result


def main():
    import library_selfbuild as lib
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    config = json.loads(args.config.read_text())
    db = lib.ReadOnlyPostgres(config['psql_command'], config.get('statement_timeout_ms', 15000))
    try:
        result = measure(db, config, args.output)
    except Exception as error:
        if args.output.is_dir():
            write(args.output / 'failure.json', {'state': 'failed', 'error': observation.error_code(error),
                                               'aggregate_queries': db.calls, 'model_calls': 0})
        raise
    print(json.dumps({'protocol': PROTOCOL, 'input': str(args.output / 'oracle-input.json'),
                      'source_id': result['source_id'], 'aggregate_queries': db.calls, 'model_calls': 0}))


if __name__ == '__main__':
    main()
