"""Pinned native Oracle dictionaries -> read-only AgentDB observation bindings.

No connector, model, DDL synthesis, admission, or publication lives here. File
hashes bind operator-supplied evidence; they are not an admission signature.
"""
import hashlib
import json
from pathlib import Path
import re
import unicodedata

CATALOG = 'oo.pipeline.sealed-import-catalog/v1'
COLLECTION = 'oo.pipeline.sealed-import-collection/v1'
IDENTITY = ('source_id', 'source_generation', 'target_generation', 'run_id',
            'config_digest', 'schema_digest', 'type_digest', 'input_digest')
C5_BINDING = 'oo.pipeline.oracle-c5-catalog-binding/v1'
# Same frozen PG17 set as oo-connectors/src/ident.rs; parity is tested.
RESERVED = set("""all analyse analyze and any array as asc asymmetric both case cast
check collate column constraint create current_catalog current_date current_role
current_schema current_time current_timestamp current_user default deferrable desc
distinct do else end except false fetch for foreign from grant group having in
initially intersect into lateral leading limit localtime localtimestamp not null
offset on only or order placing primary references returning select session_user
some symmetric system_user table then to trailing true union unique user using
variadic when where window with""".split())


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                    separators=(',', ':')).encode()).hexdigest()


def require(ok, code):
    if not ok:
        raise ValueError('oracle_' + code)


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate_key')
        result[key] = value
    return result


def read_pin(pin, raw=False):
    require(isinstance(pin, dict) and isinstance(pin.get('path'), str)
            and re.fullmatch('[0-9a-f]{64}', pin.get('sha256', '')), 'input_pin')
    path = Path(pin['path'])
    require(path.is_file() and not path.is_symlink()
            and 0 < path.stat().st_size <= 32 * 1024 * 1024, 'input_file_budget')
    data = path.read_bytes()
    require(len(data) <= 32 * 1024 * 1024
            and hashlib.sha256(data).hexdigest() == pin['sha256'], 'input_digest')
    return data if raw else json.loads(data, object_pairs_hook=unique)


def logical(name):
    """The connector's pgident/v1 spelling, including literal q_ prefixes."""
    require(isinstance(name, str) and 0 < len(name.encode()) <= 128
            and not re.search(r'[\x00-\x1f\x7f]', name), 'source_identifier')
    name = unicodedata.normalize('NFC', name)
    if (re.fullmatch('[a-z_][a-z_0-9]*', name) and not name.startswith('q_')
            and len(name.encode()) <= 63 and name not in RESERVED):
        return name
    return 'q_' + name.encode().hex()


def native_type(column):
    kind = column['type']
    if kind == 'NUMBER' and column.get('precision') is not None:
        return f"NUMBER({column['precision']},{column['scale']})"
    return kind


def unsupported(column):
    kind = column['type']
    if column.get('virtual') == 'YES' or column.get('user_generated') == 'NO':
        return 'derived_or_system_column'
    if kind == 'NUMBER':
        p, s = column.get('precision'), column.get('scale')
        if type(p) is not int or type(s) is not int or not 1 <= p <= 38 or not 0 <= s <= p:
            return 'number_encoding_contract_required'
    elif kind.startswith('TIMESTAMP'):
        match = re.fullmatch(r'TIMESTAMP(?:\((\d+)\))?( WITH TIME ZONE)?', kind)
        if not match:
            return 'temporal_contract_required'
        if int(match[1] or 6) > 6:
            # Aligned nanos need their own producer proof, not a lossy PG scan.
            return 'submicrosecond_source_proof_required'
    elif kind not in {'DATE', 'RAW', 'CHAR', 'NCHAR', 'VARCHAR2', 'NVARCHAR2'}:
        return 'unsupported_source_type'
    return None


def check_target(column, stored, semantics, precision_proof=None):
    issue = unsupported(column)
    if issue == 'submicrosecond_source_proof_required':
        require(isinstance(precision_proof, dict)
                and precision_proof == {'source_type': column['type'], 'submicroseconds_zero': True},
                issue)
    else:
        require(issue is None, issue or 'source_type')
    kind, oid, typmod = column['type'], stored['oid'], stored['typmod']
    if kind == 'NUMBER':
        p, s = column['precision'], column['scale']
        integer_digits = {21: 4, 23: 9, 20: 18}
        require((oid in integer_digits and s == 0 and p <= integer_digits[oid])
                or (oid == 1700 and typmod == ((p << 16) | s) + 4),
                'number_target_precision')
    elif kind == 'DATE' or kind.startswith('TIMESTAMP'):
        timezone = kind.endswith(' WITH TIME ZONE')
        require(oid == (1184 if timezone else 1114), 'temporal_target_type')
        precision = 0 if kind == 'DATE' else int(
            re.fullmatch(r'TIMESTAMP(?:\((\d+)\))?( WITH TIME ZONE)?', kind)[1] or 6)
        require(typmod == -1 or precision <= typmod <= 6, 'temporal_target_precision')
        if timezone:
            require(semantics.get('temporal_policy', {}).get('timezone') == 'utc_instant',
                    'timezone_policy_required')
    elif kind == 'RAW':
        require(oid == 17 and typmod == -1, 'raw_target_type')
    else:
        require(oid in {25, 1042, 1043}, 'character_target_type')
        if typmod != -1:
            length = column.get('char_length') or column.get('bytes')
            require(type(length) is int and typmod - 4 >= length, 'character_target_length')

def target_contract(column):
    kind = column['type']
    if kind == 'DATE':
        return 'naive_timestamp_preserves_time_of_day'
    if kind.endswith(' WITH TIME ZONE'):
        return 'utc_instant_original_zone_not_preserved'
    if kind.startswith('TIMESTAMP'):
        return 'naive_timestamp_microseconds'
    if kind == 'NUMBER':
        return 'exact_declared_precision_scale'
    if kind == 'RAW':
        return 'binary_not_text'
    return 'character_storage'


def catalogs(document):
    if document.get('protocol') == CATALOG:
        return [document]
    require(document.get('protocol') == COLLECTION, 'catalog_protocol')
    require(document.get('collection_sha256') == digest(
        {k: v for k, v in document.items() if k != 'collection_sha256'}), 'collection_digest')
    entries, vector = document.get('entries', []), document.get('binding_vector', [])
    require(entries and len(entries) == len(vector), 'collection_vector')
    for entry, binding in zip(entries, vector):
        require(len(entry.get('tables', [])) == 1, 'collection_table')
        table = entry['tables'][0]
        expected = {k: entry.get(k) for k in IDENTITY}
        expected.update(dataset=table['logical_schema'] + '.' + table['logical_table'],
                        stored_namespace=table['stored_namespace'], stored_physical=table['stored_physical'])
        require(binding == expected, 'collection_vector')
    return entries

def c5_binding(entry):
    """Accept published catalogs only with the pipeline's exact C5 proof."""
    proof = entry.get('oracle_c5_binding')
    if proof is None and 'oracle_c5_binding' not in entry:
        require(entry.get('state') == 'awaiting_publish', 'catalog_unsealed')
        return None
    require(isinstance(proof, dict) and set(proof) == {
        'protocol', 'source_id', 'run_id', 'source_generation', 'target_generation',
        'captured_rows', 'effective_rows', 'scope', 'publication_receipt_sha256'},
        'c5_catalog_binding')
    require(entry.get('protocol') == CATALOG and entry.get('engine') == 'oracle'
            and entry.get('state') == 'published' and entry.get('capture') == 'sealed_import'
            and entry.get('shadows_queried') is False and proof['protocol'] == C5_BINDING,
            'c5_catalog_binding')
    require(isinstance(entry.get('source_id'), str) and bool(entry['source_id'])
            and isinstance(entry.get('run_id'), str)
            and re.fullmatch('[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', entry['run_id'])
            and all(isinstance(entry.get(k), str) and re.fullmatch('[0-9a-f]{64}', entry[k])
                    for k in ('config_digest', 'schema_digest', 'type_digest', 'input_digest')),
            'c5_catalog_identity')
    require(all(proof.get(k) == entry.get(k) and k in entry for k in
                ('source_id', 'run_id', 'source_generation', 'target_generation'))
            and all(type(proof[k]) is int and proof[k] > 0 for k in
                    ('source_generation', 'target_generation'))
            and all(type(proof[k]) is int and proof[k] >= 0 for k in
                    ('captured_rows', 'effective_rows'))
            and proof['captured_rows'] <= proof['effective_rows']
            and isinstance(proof.get('publication_receipt_sha256'), str)
            and re.fullmatch('[0-9a-f]{64}', proof['publication_receipt_sha256']),
            'c5_catalog_identity')
    tables, scope = entry.get('tables', []), proof.get('scope')
    require(len(tables) == 1 and isinstance(scope, dict), 'c5_catalog_scope')
    require(scope.get('protocol') == 'oo.pipeline.native-c5-range/v1'
            and scope.get('source_engine') == 'oracle' and scope.get('capture_method') == 'c5_diff'
            and scope.get('delete_detection') == 'reconcile'
            and scope.get('source_id') == entry['source_id']
            and type(scope.get('source_generation')) is int
            and scope.get('source_generation') == entry['source_generation']
            and type(scope.get('parent_generation')) is int
            and scope['parent_generation'] + 1 == entry['source_generation']
            and type(scope.get('row_count')) is int
            and scope.get('row_count') == proof['captured_rows']
            and scope.get('arrow_sha256') == entry.get('input_digest')
            and isinstance(scope.get('schema_fingerprint'), str)
            and re.fullmatch('[0-9a-f]{64}', scope['schema_fingerprint'])
            and scope.get('position_codec') == 'snapshot-zero/v1'
            and scope.get('applied_pos') == '0'
            and tables[0].get('accepted_rows') == proof['effective_rows'], 'c5_catalog_scope')
    snapshot = scope.get('oracle_snapshot', {})
    require(isinstance(snapshot, dict)
            and isinstance(snapshot.get('identity'), str) and bool(snapshot['identity'])
            and isinstance(snapshot.get('scn'), str) and re.fullmatch('[1-9][0-9]{0,37}', snapshot['scn'])
            and isinstance(snapshot.get('manifest_sha256'), str)
            and re.fullmatch('[0-9a-f]{64}', snapshot['manifest_sha256'])
            and snapshot.get('source_schema') == scope.get('schema')
            and snapshot.get('source_table') == scope.get('table')
            and (logical(scope['schema']), logical(scope['table'])) ==
                (tables[0]['logical_schema'], tables[0]['logical_table']), 'c5_catalog_snapshot')
    return proof

def observation_binding(binding):
    """Exact operator admission, shared by the protected state and profile APIs."""
    if 'tables' in binding:
        require(len(binding['tables']) == 1, 'c5_observation_table')
        stored = binding['tables'][0]
    else:
        stored = binding
    keys = ('stored_namespace', 'stored_physical', 'columns', 'accepted_rows')
    require(all(k in binding for k in (*IDENTITY, 'oracle_c5_binding'))
            and all(k in stored for k in keys), 'c5_observation_binding')
    return {**{k: binding[k] for k in (*IDENTITY, 'oracle_c5_binding')},
            **{k: stored[k] for k in keys}}

def observation_state_query(entry):
    from library_observation import literal
    return ('select _oo.c5_oracle_observation_state('
            + literal(json.dumps(observation_binding(entry), sort_keys=True,
                                 separators=(',', ':'))) + '::jsonb)')

def profile_query(binding, default_sql, columns=None):
    """Internal aggregate API, not an Agent-visible view/fence override."""
    proof = binding.get('oracle_c5_binding')
    if proof is None:
        return default_sql
    require(proof.get('protocol') == C5_BINDING and all(
        proof.get(k) == binding.get(k) for k in
        ('source_id', 'run_id', 'source_generation', 'target_generation'))
        and all(type(binding.get(k)) is int and binding[k] > 0
                for k in ('source_generation', 'target_generation'))
        and isinstance(proof.get('publication_receipt_sha256'), str)
        and re.fullmatch('[0-9a-f]{64}', proof['publication_receipt_sha256']), 'c5_profile_binding')
    from library_observation import literal
    expected = {c['stored_physical']: {c['logical_name']} for c in binding['columns']}
    for mapping in binding.get('column_mappings', []):
        require(mapping['physical_name'] in expected, 'c5_profile_columns')
        expected[mapping['physical_name']].add(mapping['logical_column'])
    selected = ([{'name': c['logical_name'], 'physical_name': c['stored_physical']}
                 for c in binding['columns']] if columns is None else
                [{'name': c['name'], 'physical_name': c['physical_name']} for c in columns])
    require(len(selected) == len(expected) and {c['physical_name'] for c in selected} == set(expected)
            and all(c['name'] in expected[c['physical_name']] for c in selected), 'c5_profile_columns')
    access = binding.get('profile_access', 'internal_aggregate_api')
    require(access in {'internal_aggregate_api', 'internal_owner_readonly'}, 'c5_profile_access')
    if access == 'internal_owner_readonly':
        require(default_sql.lstrip().lower().startswith('select '), 'c5_profile_query')
        return ("select case when current_user='oo_target_owner' "
                "and current_setting('transaction_read_only')='on' then ("
                + default_sql + ") else null end")
    return ('select _oo.c5_oracle_profile(' + ','.join([
        literal(json.dumps(observation_binding(binding), sort_keys=True, separators=(',', ':'))) + '::jsonb',
        literal(json.dumps(selected, sort_keys=True, separators=(',', ':'))) + '::jsonb']) + ')')


def bind_catalog_columns(raw, stored):
    """Join native names or pgident names without rewriting sealed bytes."""
    result, seen = {}, set()
    for column in stored:
        name = column['logical_name']
        matches = [c for c in raw if name in {
            unicodedata.normalize('NFC', c['name']), logical(c['name'])}]
        require(len(matches) == 1 and name not in result, 'catalog_column_coverage')
        native = matches[0]
        require(native['name'] not in seen, 'catalog_column_coverage')
        seen.add(native['name'])
        result[name] = native
    require(len(result) == len(raw), 'catalog_column_coverage')
    return result

def load(config, inventory=None):
    """Return source metadata keyed by normalized dataset; never query a source."""
    require(isinstance(config, dict) and bool(config.get('source_id')), 'input_source')
    require(type(config.get('max_columns', 65536)) is int
            and 1 <= config.get('max_columns', 65536) <= 65536, 'column_observation_budget')
    docs = {key: read_pin(config.get(key)) for key in
            ('catalog', 'source_schema', 'profile', 'semantics')}
    if config.get('source_bound') is not None:
        docs['source_bound'] = read_pin(config['source_bound'])
    catalog, schema, profile, semantics = (docs[k] for k in
                                          ('catalog', 'source_schema', 'profile', 'semantics'))
    require(schema.get('protocol') == 'oo.oracle.inventory/v1', 'source_schema_protocol')
    require(schema.get('source_id') == config['source_id'], 'cross_source')
    require(profile.get('protocol') in {'oo.native.profile/v1', 'oo.source.oracle.profile/v1',
                                       'oo.oracle.native-full-profile/v1'},
            'profile_protocol')
    require(semantics.get('protocol') in {'oo.native.semantics/v1', 'oo.source.oracle.audit/v1',
            'oo.source-semantics.source-bound/v1', 'oo.oracle.native-c5-source-semantics/v1'},
            'semantics_protocol')
    temporal_evidence = None
    if semantics.get('measurement_protocol') == 'oo.builder.oracle-target-observation/v1':
        from oracle_target_observation import validate_evidence
        temporal_evidence = validate_evidence(config, docs)
    entries = catalogs(catalog)
    pins = {key + '_sha256': config[key]['sha256'] for key in docs}
    # The shared S5 rules list predates this adapter; its facts must bind it too.
    pins['adapter_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    dictionary = config.get('business_dictionary')
    require(type(config.get('require_business_dictionary', False)) is bool, 'dictionary_requirement')
    if dictionary is None:
        require(not config.get('require_business_dictionary'), 'business_dictionary_missing')
        business = {'state': 'missing', 'business_semantics_approved': False}
    else:
        read_pin(dictionary, raw=True)
        require(dictionary.get('source_id') == config['source_id']
                and dictionary.get('source_schema_sha256') == pins['source_schema_sha256'],
                'dictionary_source_binding')
        business = {k: dictionary[k] for k in ('sha256', 'source_id', 'source_schema_sha256')}
        business.update(state='pinned_unreviewed', business_semantics_approved=False)
    bound = semantics if semantics['protocol'] == 'oo.source-semantics.source-bound/v1' else docs.get('source_bound')
    if bound is not None:
        require(bound.get('protocol') == 'oo.source-semantics.source-bound/v1'
                and bound.get('engine') == 'oracle'
                and all(k in bound and bound[k] == catalog.get(k) for k in IDENTITY),
                'source_bound_identity')
    for doc in (profile, semantics):
        require(doc.get('source_id') == config['source_id'], 'cross_source')
        require(all(doc.get(k, 'oracle') == 'oracle' for k in ('engine', 'source_engine')), 'cross_source')
        if catalog['protocol'] == COLLECTION:
            require(doc.get('collection_sha256') == pins['catalog_sha256']
                    and doc.get('binding_vector_sha256') == digest(catalog['binding_vector']),
                    'profile_semantics_binding')
        else:
            required = IDENTITY if bound is None else ('source_id', 'source_generation', 'target_generation')
            require(all(k in doc and doc[k] == catalog.get(k) for k in required)
                    and all(doc[k] == catalog.get(k) for k in IDENTITY if k in doc),
                    'profile_semantics_binding')
    if semantics['protocol'] == 'oo.oracle.native-c5-source-semantics/v1':
        require(semantics.get('executed') is True and bound is not None
                and semantics.get('evidence', {}).get('source-bound', {}).get('sha256')
                == pins.get('source_bound_sha256'), 'source_semantics_unmeasured')
    if bound is not None:
        require(bound.get('dictionary', {}).get('protocol') == 'oo.source-semantics.type-dictionary/v1'
                and bool(bound['dictionary'].get('columns')), 'type_dictionary_missing')
        require(isinstance(bound.get('scn'), str) and bound['scn'].isdigit()
                and bound.get('published_scn') == bound['scn'], 'source_bound_position')
    require(profile.get('sampled') is False, 'sampled_profile')
    raw_tables = schema.get('tables', [])
    require(0 < len(raw_tables) <= 4096, 'table_inventory_budget')
    sources = {}
    if inventory is None:
        inventory = []
    for table in raw_tables:
        dataset = logical(table['owner']) + '.' + logical(table['table'])
        require(dataset not in sources and 0 < len(table['columns']) <= 4096, 'source_table_inventory')
        cols = table['columns']
        require(len({c['name'] for c in cols}) == len(cols)
                and len({logical(c['name']) for c in cols}) == len(cols)
                and all(c.get('nullable') in {'Y', 'N'} for c in cols), 'source_column_inventory')
        keys = table.get('keys')
        require(isinstance(keys, list) and len(set(keys)) == len(keys)
                and set(keys) <= {c['name'] for c in cols}, 'source_primary_key')
        sources[dataset] = table
        inventory.append(dict(dataset=dataset, source_schema=table['owner'], source_table=table['table'],
                              disposition='not_in_sealed_catalog', issues=table.get('issues', []),
                              primary_key=keys, key_state='validated' if keys else 'no_validated_primary_key', columns=[
                                  dict(name=c['name'], source_type=native_type(c),
                                       disposition=unsupported(c) or 'not_observed') for c in cols]))
    result = {}
    profiles = profile.get('tables', [])
    require(isinstance(profiles, list)
            and len({p['dataset'] for p in profiles}) == len(profiles), 'profile_table_coverage')
    for entry in entries:
        require(entry.get('protocol') == CATALOG and entry.get('engine') == 'oracle', 'catalog_engine')
        require(entry.get('source_id') == config['source_id'], 'cross_source')
        require(all(re.fullmatch('[0-9a-f]{64}', entry.get(k, '')) for k in
                    ('schema_digest', 'type_digest', 'config_digest', 'input_digest'))
                and isinstance(entry.get('run_id'), str) and bool(entry['run_id']), 'catalog_identity')
        proof = c5_binding(entry)
        if proof is not None:
            require(semantics.get('measurement_protocol') == 'oo.builder.oracle-target-observation/v1'
                    and profile.get('profile_origin') == 'read_only_published_target',
                    'c5_target_observation_required')
        require(entry.get('capture') == 'sealed_import'
                and entry.get('shadows_queried') is False, 'catalog_unsealed')
        require(all(type(entry.get(k)) is int and entry[k] > 0
                    for k in ('source_generation', 'target_generation')), 'catalog_generation')
        quality = entry.get('quality', {})
        require(quality.get('loss_concealed') is False
                and quality.get('stored_rejected_rows') == 0
                and quality.get('manifest_reject_count') == 0, 'catalog_quality')
        tables = entry.get('tables', [])
        require(bool(tables), 'catalog_table_missing')
        preimage = ''
        for stored in tables:
            dataset = stored['logical_schema'] + '.' + stored['logical_table']
            require(dataset in sources and dataset not in result, 'catalog_table_binding')
            raw = sources[dataset]
            listed = stored['columns']
            columns = bind_catalog_columns(raw['columns'], listed)
            if bound is not None:
                expected_bindings = [{'sealed_column': name, 'dictionary_column': c['name']}
                                     for name, c in columns.items()]
                expected_lines = [
                    '|'.join([c['name'], c['type'],
                              '-' if c.get('precision') is None else str(c['precision']),
                              '-' if c.get('scale') is None else str(c['scale']),
                              str(c.get('char_length') or 0)]) for c in raw['columns']]
                require(len(tables) == 1 and sorted(bound.get('bindings', []), key=digest)
                        == sorted(expected_bindings, key=digest)
                        and sorted(bound['dictionary']['columns']) == sorted(expected_lines),
                        'type_dictionary_binding')
            require(not stored.get('rejects'), 'catalog_quality')
            require(raw['keys'] and not raw.get('issues'), 'source_inventory_issue')
            require({columns[c['logical_name']]['name'] for c in listed if c['key']}
                    == set(raw['keys']), 'catalog_primary_key')
            measured = [p for p in profiles if p['dataset'] == dataset]
            require(len(measured) == 1 and measured[0].get('full_scan') is True
                    and measured[0].get('rows') == stored['accepted_rows'], 'profile_table_binding')
            require(all(measured[0].get(k, entry[k]) == entry[k] for k in IDENTITY),
                    'profile_table_binding')
            measured_cols = measured[0].get('columns', [])
            require(len(measured_cols) == len(columns)
                    and {c['name'] for c in measured_cols} == set(columns), 'profile_column_binding')
            captured = {c['logical_name']: c.get('source_type') for c in listed}
            for c in measured_cols:
                native = columns[c['name']]
                allowed = {native['type'], native_type(native)}
                if profile['protocol'] == 'oo.oracle.native-full-profile/v1':
                    require(c.get('source_name') == native['name'], 'profile_column_binding')
                    allowed.add(captured[c['name']])
                require(c.get('source_type') in allowed, 'profile_column_binding')
            preimage += dataset + '\n'
            observed_columns = {}
            for c in listed:
                name, native = c['logical_name'], columns[c['logical_name']]
                temporal_proof = ((temporal_evidence or {}).get('columns', {}).get(
                    raw['owner'] + '.' + raw['table'] + '.' + native['name']))
                check_target(native, c, semantics, temporal_proof)
                preimage += f"{name}\t{c['oid']}\t{c['typmod']}\t{c.get('caveat') or ''}\n"
                observed_columns[native['name']] = dict(
                    source_type=native_type(native), source_engine='oracle',
                    nullable=native['nullable'] == 'Y', comment=native.get('comment') or '',
                    comment_state='observed' if 'comment' in native else 'unmeasured',
                    oracle_source=native, captured_type=c.get('source_type'), caveat=c.get('caveat'),
                    target_oid=c['oid'], target_typmod=c['typmod'],
                    target_contract=target_contract(native))
                if ('default_sql' in native
                        and native.get('default_observation', {}).get('state') != 'unmeasured'):
                    observed_columns[native['name']].update(
                        default_sql=native['default_sql'], default_present=native['default_sql'] is not None)
            binding = {k: entry[k] for k in IDENTITY}
            binding.update(stored_namespace=stored['stored_namespace'], stored_physical=stored['stored_physical'],
                           columns=listed, accepted_rows=stored['accepted_rows'],
                           column_mappings=[dict(sealed_column=c['logical_name'],
                               source_name=columns[c['logical_name']]['name'],
                               logical_column=logical(columns[c['logical_name']]['name']),
                               physical_name=c['stored_physical']) for c in listed])
            if proof is not None:
                binding['oracle_c5_binding'] = proof
                binding['profile_access'] = config.get('profile_access', 'internal_aggregate_api')
            result[dataset] = dict(
                schema=stored['logical_schema'], columns=observed_columns, primary_key=raw['keys'],
                ddl_sha256=digest(raw), schema_basis='oracle_dictionary',
                table_comment=raw.get('comment') or '', constraints_sql=[], table_options_sql=None,
                oracle=dict(protocol='oo.builder.oracle-source-observation/v1', pins=pins,
                            binding=binding, source_schema=raw['owner'], source_table=raw['table'],
                            source_metadata={k: v for k, v in schema.items() if k != 'tables'},
                            source_dictionary=raw, business_dictionary=business,
                            source_semantics=semantics, source_bound=bound, source_profile=measured[0]))
            next(i for i in inventory if i['dataset'] == dataset)['disposition'] = 'sealed_pending_observation'
        require(hashlib.sha256(preimage.encode()).hexdigest() == entry.get('schema_digest'),
                'sealed_schema_digest')
    require(set(result) == {p['dataset'] for p in profiles}, 'profile_table_coverage')
    require(sum(len(t['columns']) for t in result.values()) <= config.get('max_columns', 65536),
            'column_observation_budget')
    return dict(tables=result, inventory=inventory, pins=pins, business_dictionary=business)


def validate_target(source, fresh, mappings, attributes):
    binding = source['oracle']['binding']
    require(all(fresh.get(a) == binding[b] for a, b in (
        ('source', 'source_id'), ('source_generation', 'source_generation'),
        ('target_generation', 'target_generation'), ('physical_name', 'stored_physical'))),
        'current_generation_drift')
    expected = {(c['logical_column'], c['physical_name'], binding['stored_namespace'],
                 binding['stored_physical']) for c in binding['column_mappings']}
    actual = {(m['logical_column'], m['physical_name'], m['scope_namespace_physical'],
               m['scope_table_physical']) for m in mappings}
    require(len(mappings) == len(expected) and actual == expected, 'current_mapping_drift')
    for mapping in mappings:
        require(mapping.get('config_digest', binding['config_digest']) == binding['config_digest']
                and mapping.get('run_generation', binding['source_generation']) == binding['source_generation'],
                'current_mapping_drift')
    attrs = {a['attname']: a for a in attributes
             if a['nspname'] == '_oo' and a['relname'] == binding['stored_physical']}
    require(all(c['stored_physical'] in attrs and
                (attrs[c['stored_physical']].get('oid'), attrs[c['stored_physical']].get('typmod'))
                == (c['oid'], c['typmod']) for c in binding['columns']), 'current_type_drift')
    semantics = source['oracle']['source_semantics']
    if semantics.get('measurement_protocol') == 'oo.builder.oracle-target-observation/v1':
        expected_fresh = [f for f in semantics['target_observation']['freshness']
                          if f['physical_name'] == binding['stored_physical']]
        require(expected_fresh == [fresh], 'current_publication_drift')
