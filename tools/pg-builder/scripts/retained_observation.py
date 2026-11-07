"""D15: observe one sealed oo-load retained generation as Builder facts.

Read-only Agent DB metadata and value-free aggregates. The source database is
never contacted; the retained cut (recipe, relation proofs, mapped view) is the
data source. Volatile cut identity stays in `retained` and is projected out of
data-only equivalence by library_semantics.retained_data_only.
"""
import hashlib
import json
import re
import unicodedata

import library_observation as observation
import library_selfbuild as lib

PROTOCOL = 'oo.builder.retained-generation-observation/v1'
RECEIPT_PROTOCOL = 'oo.load.per-table-receipt/v1'
ENGINES = ('postgres', 'mysql', 'oracle')
TOKEN = re.compile(r'[A-Za-z0-9._-]{1,128}')
HEX64 = re.compile(r'[0-9a-f]{64}')


def rust_digest(value):
    """serde_json bytes as oo-pipeline's `digest`: sorted keys, compact, UTF-8."""
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                     separators=(',', ':')).encode()).hexdigest()


def candidate_sql(library_id, generation):
    if not TOKEN.fullmatch(library_id) or not TOKEN.fullmatch(generation):
        raise ValueError('retained_identity')
    where = ('g.library_id=' + lib.literal(library_id) + ' and g.run=' + lib.literal(generation)
             + ' and g.sealed_at is not null')
    # Same projection as _oo.native_library_relation_proof, readable without EXECUTE.
    proof = ("(select jsonb_build_object('schema',n.nspname,'relation',c.relname,'kind',c.relkind,"
             "'definition',case when c.relkind='v' then pg_get_viewdef(c.oid,false) else null end,"
             "'columns',(select jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull)"
             " order by a.attnum) from pg_attribute a where a.attrelid=c.oid and a.attnum>0 and not a.attisdropped))"
             " from pg_class c join pg_namespace n on n.oid=c.relnamespace"
             " where n.nspname=p.value->>'schema' and c.relname=p.value->>'relation')")
    return (
        "select json_build_object('candidate',(select json_build_object("
        "'generation',g.generation,'run',g.run,'library_id',g.library_id,'source',g.source,"
        "'receipt_sha256',g.receipt_sha256,'request',g.request,'shard',g.shard,"
        "'data_vector',g.data_vector,'data_vector_sha256',g.data_vector_sha256,"
        "'proofs_current',(select bool_and(p.value is not distinct from " + proof + ")"
        " from jsonb_array_elements(g.relation_proofs) p),"
        "'proof_count',jsonb_array_length(g.relation_proofs))"
        " from _oo_load.gateway_publications g where " + where + "),"
        "'mappings',(select coalesce(json_agg(row_to_json(m)),'[]'::json) from _oo.physical_mappings m"
        " join _oo_load.gateway_publications g on m.source=g.source and m.run_generation=g.generation"
        " and m.config_digest=g.request->>'binding_sha256' where m.level='c' and " + where + "),"
        "'attributes',(select coalesce(json_agg(json_build_object('relname',c.relname,'attname',a.attname,"
        "'data_type',format_type(a.atttypid,a.atttypmod),'not_null',a.attnotnull)),'[]'::json)"
        " from _oo_load.gateway_publications g cross join lateral jsonb_array_elements(g.data_vector) e"
        " join pg_namespace n on n.nspname='_oo' join pg_class c on c.relnamespace=n.oid"
        " and c.relname=e.value->>'physical_name' and c.relkind='v'"
        " join pg_attribute a on a.attrelid=c.oid and a.attnum>0 and not a.attisdropped where " + where + "))")


def original(logical):
    """Undo connector pgident/v1 q_ encoding for key matching; never guess otherwise."""
    if logical.startswith('q_') and re.fullmatch(r'(?:[0-9a-f]{2})+', logical[2:]):
        try:
            return bytes.fromhex(logical[2:]).decode('utf-8')
        except UnicodeDecodeError:
            raise ValueError('retained_identifier') from None
    return logical


def key_columns(shape):
    kind = shape.get('shape')
    if kind in ('integer_primary_key', 'string_primary_key'):
        return [shape['column']]
    if kind in ('composite_primary_key', 'unique_index'):
        return list(shape['columns'])
    if kind == 'no_key':
        return []
    raise ValueError('retained_key_shape')


def validate_candidate(candidate, library_id, generation, engine):
    if engine not in ENGINES:
        raise ValueError('retained_engine')
    if candidate is None:
        raise ValueError('retained_candidate_missing')
    shard = candidate['shard']
    receipt = shard['receipt']
    request = candidate['request']
    vector = candidate['data_vector']
    if (candidate['library_id'] != library_id or candidate['run'] != generation
            or receipt.get('protocol') != RECEIPT_PROTOCOL
            or receipt.get('consistency') != 'retained_generation'
            or receipt.get('consistency_grade') != 'B' or receipt.get('run_id') != generation
            or receipt.get('source_id') != candidate['source'] or shard.get('anchor_run') != generation
            or receipt.get('source_generation') != candidate['generation']
            or request.get('source_id') != candidate['source']
            or not HEX64.fullmatch(request.get('binding_sha256', ''))
            or rust_digest(receipt) != shard.get('receipt_sha256')
            or rust_digest(vector) != candidate['data_vector_sha256']
            or len(vector) != len(receipt['tables']) or not vector):
        raise ValueError('retained_candidate_binding')
    # A changed view, column or stage after sealing is not the reviewed cut.
    if candidate['proofs_current'] is not True or candidate['proof_count'] < 2:
        raise ValueError('retained_relation_proof_drift')


def observe(db, library_id, generation, engine, source_catalog=None):
    """Return (tables, provenance) for build_facts. Raises fixed ValueError codes."""
    metadata = db.query(candidate_sql(library_id, generation))
    candidate = metadata['candidate']
    validate_candidate(candidate, library_id, generation, engine)
    receipt = candidate['shard']['receipt']
    recipes = candidate['request']['recipes']
    tables = []
    for entry in candidate['data_vector']:
        published = [t for t in receipt['tables'] if t['physical_name'] == entry['physical_name']]
        if (len(published) != 1 or entry['source_id'] != candidate['source']
                or entry['generation'] != candidate['generation']
                or entry['dataset'] != published[0]['storage_schema'] + '.' + published[0]['storage_table']
                or entry['recipe_sha256'] != published[0]['recipe_sha256']):
            raise ValueError('retained_candidate_binding')
        published = published[0]
        recipe = [r for r in recipes if r['plan']['table'] == {'schema': published['schema'], 'name': published['table']}]
        if len(recipe) != 1 or rust_digest(recipe[0]) != entry['recipe_sha256']:
            raise ValueError('retained_recipe_binding')
        schema, table = entry['dataset'].split('.', 1)
        mapped = [m for m in metadata['mappings'] if m['logical_namespace'] == schema
                  and m['logical_table'] == table and m['scope_table_physical'] == entry['physical_name']]
        attributes = {a['attname']: a for a in metadata['attributes'] if a['relname'] == entry['physical_name']}
        expected = {c['name']: c['data_type'] for c in entry['expected_columns']}
        if ({m['logical_column'] for m in mapped} != set(expected) or len(mapped) != len(expected)
                or set(attributes) != {m['physical_name'] for m in mapped}):
            raise ValueError('retained_column_mapping')
        columns = []
        for m in sorted(mapped, key=lambda m: m['logical_column']):
            attribute = attributes[m['physical_name']]
            if attribute['data_type'] != expected[m['logical_column']]:
                raise ValueError('retained_column_type_drift')
            column = dict(name=m['logical_column'], physical_name=m['physical_name'],
                          data_type=attribute['data_type'], nullable=not attribute['not_null'])
            if original(m['logical_column']) != m['logical_column']:
                column['source_name'] = original(m['logical_column'])
            columns.append(column)
        names = {unicodedata.normalize('NFC', original(c['name'])): c['name'] for c in columns}
        try:
            primary_key = [names[unicodedata.normalize('NFC', k)] for k in key_columns(recipe[0]['plan']['key'])]
        except KeyError:
            raise ValueError('retained_key_mapping') from None
        shape = dict(protocol=PROTOCOL, engine=engine, key=recipe[0]['plan']['key'],
                     columns=[[c['name'], c['data_type'], c['nullable']] for c in columns])
        item = dict(dataset=entry['dataset'], source_id=candidate['source'], source_engine=engine,
                    source_generation=candidate['generation'], target_generation=candidate['generation'],
                    physical=lib.quote('_oo') + '.' + lib.quote(entry['physical_name']),
                    primary_key=primary_key, ddl_sha256=lib.digest(shape), table_comment='',
                    constraints_sql=[], table_options_sql='', columns=columns,
                    retained=dict(protocol=PROTOCOL, receipt_protocol=RECEIPT_PROTOCOL,
                                  consistency='retained_generation', library_id=library_id,
                                  config_digest=candidate['request']['binding_sha256'],
                                  generation=generation, source_generation=candidate['generation'],
                                  receipt_sha256=candidate['receipt_sha256'],
                                  data_vector_sha256=candidate['data_vector_sha256'],
                                  recipe_sha256=entry['recipe_sha256'], storage_relation=entry['storage_relation'],
                                  physical_name=entry['physical_name'], key=recipe[0]['plan']['key'],
                                  expected_columns=entry['expected_columns']))
        observed = db.query(observation.profile_sql(item['physical'], columns))
        profiles = observation.profiles(observed, [c['name'] for c in columns])
        if observed['rows'] != published['accepted_rows']:
            raise ValueError('retained_row_count_drift')
        for column in columns:
            column['profile'] = profiles[column['name']]
        item['observation'] = dict(protocol=observation.PROTOCOL, state='complete',
                                   expected_columns=[c['name'] for c in columns],
                                   snapshot=observed['snapshot'], observed_at=observed['observed_at'],
                                   binding=observation.table_binding(item))
        tables.append(item)
    if source_catalog is not None:
        import retained_catalog
        retained_catalog.apply(tables, source_catalog, receipt)
    observation.require_complete_columns(tables)
    provenance = dict(protocol=PROTOCOL, library_id=library_id, generation=generation,
                      receipt_sha256=candidate['receipt_sha256'],
                      data_vector_sha256=candidate['data_vector_sha256'])
    # The cut must not move while it is observed.
    if db.query(candidate_sql(library_id, generation))['candidate'] != candidate:
        raise ValueError('retained_observation_drift')
    return sorted(tables, key=lambda t: t['dataset']), provenance
