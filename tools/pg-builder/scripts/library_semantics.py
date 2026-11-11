"""S5 dependencies, not approval: observations remain bound to the current facts."""
import copy
import hashlib
import json
from pathlib import Path

import relation_keys


PROTOCOL = 'oo.library.s5-semantics/v1'


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def ordered(values):
    return sorted(values, key=canonical)


def profile(value):
    # Counts are observation evidence; these state changes affect interpretation.
    result = {k: v for k, v in value.items() if k not in {
        'rows', 'observed_rows', 'non_null', 'null_count', 'null_frac', 'distinct',
        'min_length', 'max_length', 'blank_rows', 'digits_rows', 'leading_zero_rows'}}
    for name, state in (('null_count', 'null_present'), ('blank_rows', 'blank_present'),
                        ('leading_zero_rows', 'leading_zero_present')):
        result[state] = value[name] > 0 if name in value else value.get(state, False)
    if 'non_null' in value:
        result['all_digits'] = bool(value['non_null']) and value.get('digits_rows') == value['non_null']
    else:
        result.setdefault('all_digits', False)
    return result


def table(value):
    result = {k: copy.deepcopy(v) for k, v in value.items() if k not in {
        'source_generation', 'target_generation', 'physical', 'observation', 'columns'}}
    result['columns'] = []
    for column in value['columns']:
        result['columns'].append({
            **{k: copy.deepcopy(v) for k, v in column.items() if k not in {'physical_name', 'profile'}},
            'profile': profile(column.get('profile', {})),
        })
    result['columns'].sort(key=lambda c: c['name'])
    return result

def prompt_table(value):
    """Keep table-local meaning in the prompt; full proofs still bind S5 reuse."""
    result = table(value)
    oracle = result.get('oracle')
    if isinstance(oracle, dict) and oracle.get('protocol') == 'oo.builder.oracle-source-observation/v1':
        semantics = oracle.get('source_semantics')
        if isinstance(semantics, dict):
            # Each table carries the same whole-source verification packet.
            # Hash references avoid sending every other table's raw proof again.
            for key in ('binding_vector', 'source_catalog_observation', 'target_observation'):
                if key in semantics:
                    proof = semantics[key]
                    if (isinstance(proof, dict) and set(proof) == {'evidence_sha256', 'included_in_prompt'}
                            and proof['included_in_prompt'] is False):
                        continue
                    semantics[key] = {
                        'evidence_sha256': digest(proof), 'included_in_prompt': False,
                    }
            receipt = semantics.get('publication_receipt')
            if isinstance(receipt, dict):
                for key in ('tables', 'run_ids'):
                    if key in receipt:
                        proof = receipt[key]
                        if (isinstance(proof, dict)
                                and set(proof) == {'evidence_sha256', 'included_in_prompt', 'members'}
                                and proof['included_in_prompt'] is False):
                            continue
                        receipt[key] = {
                            'evidence_sha256': digest(proof), 'included_in_prompt': False,
                            'members': len(proof) if isinstance(proof, list) else None,
                        }
    return result


def overlap(value):
    result = {k: copy.deepcopy(v) for k, v in value.items() if k not in {
        'non_null', 'matched', 'non_null_rows', 'matched_rows', 'dictionary_distinct',
        'child_distinct', 'comment_label_matches', 'containment', 'row_containment'}}
    for key in ('containment', 'row_containment'):
        if key in value:
            ratio = None if value[key] is None else float(value[key])
            result[key] = ('unmeasured' if ratio is None else 'none' if ratio == 0
                           else 'complete' if ratio == 1 else 'accepted_partial' if ratio >= .98
                           else 'partial')
    if 'comment_label_matches' in value:
        result['comment_label_match'] = value['comment_label_matches'] > 0
    if 'evidence' in value:
        result['evidence'] = overlap(value['evidence'])
    return result


def enum(value):
    result = copy.deepcopy(value)
    result['evidence'] = {k: v for k, v in value.get('evidence', {}).items()
                          if k not in {'rows', 'distinct'}}
    result['values'] = ordered([{k: v for k, v in row.items() if k != 'rows'}
                                for row in value.get('values', [])])
    result['dictionary_candidates'] = ordered([overlap(v) for v in value.get('dictionary_candidates', [])])
    return result


BOUND_FACTS = frozenset({
    'protocol', 'tables', 'all_tables', 'binding_vector', 'generation_semantics',
    'relationships', 'relationship_candidates', 'enum_candidates',
    'provenance', 'business_owner_signed', 'facts_sha256', 'parent_facts_sha256',
    'declared_enum_observation', 'report_definitions_observation', 'dictionary_evidence'})


def extensions(facts):
    return {k: v for k, v in facts.items() if k not in BOUND_FACTS}


def bind_batch(batch, facts):
    names = {t['dataset'] for t in batch['tables']}
    inventory = {t['dataset']: t for t in facts.get('all_tables', facts['tables'])}
    related = [r for r in facts['relationship_candidates']
               if r['from']['dataset'] in names or r['to']['dataset'] in names]
    enums = [enum(e) for e in facts['enum_candidates'] if e['enum'].rsplit('.', 1)[0] in names]
    dependencies = {r[end]['dataset'] for r in related for end in ('from', 'to')} - names
    opaque_dictionaries = set()
    for item in enums:
        dependencies.update(d['dataset'] for d in item['dictionary_candidates'] if 'dataset' in d)
        opaque_dictionaries.update(d['dataset'] for d in item['dictionary_candidates'] if 'dataset' in d)
        binding = item.get('declared_binding', {})
        if binding.get('dictionary_dataset'):
            dependencies.add(binding['dictionary_dataset'])
    if not dependencies <= inventory.keys():
        raise ValueError('semantic_dependency_inventory')
    dictionary = facts.get('dictionary_evidence')
    dictionary = dictionary if dictionary and dictionary['document']['dataset'] in names else None
    declared = facts.get('declared_enum_observation')
    # Retain all candidate outcomes, including empty/weak/absent discoveries and
    # values beyond the prompt truncation. Fresh S1-S4 must rerun discovery first.
    manifest = {
        'protocol': PROTOCOL,
        'tables': [table(inventory[n]) for n in sorted(names)],
        'relationships': ordered([overlap(r) for r in related]),
        'enums': ordered(enums),
        'neighbors': [table(inventory[n]) for n in sorted(dependencies - names) if n in inventory],
        # Overlap-only discovery does not capture code->label content. Without
        # complete dictionary evidence, a new generation cannot prove equivalence.
        'opaque_dictionary_observations': [
            {k: inventory[n][k] for k in ('dataset', 'source_id', 'source_generation', 'target_generation')}
            for n in sorted(opaque_dictionaries) if n in inventory],
        'dictionary': ({'input': dictionary['input'], 'document': dictionary['document']}
                       if dictionary else None),
        'discovery': {
            'datasets': sorted(names),
            'declared_enum_config': declared.get('config_sha256') if declared else None,
            'missing_enums': sorted(t['dataset'] + '.' + c['name'] for t in batch['tables']
                                    for c in t['columns']
                                    if t['dataset'] + '.' + c['name'] not in {e['enum'] for e in enums}),
        },
        'extensions': extensions(facts),
    }
    # Stored report proposals are consumed deterministically after S5, not by the
    # model. Their fresh facts/review/Pack bindings are intentionally not reused.
    batch['semantic_input_digest'] = digest(manifest)
    batch['data_observation_version'] = facts['facts_sha256']
    batch['dependency_manifest'] = {
        'protocol': PROTOCOL, 'datasets': sorted(names), 'neighbors': sorted(dependencies),
        **{k + '_sha256': digest(v) for k, v in manifest.items() if k != 'protocol'},
    }
    return batch


def prompt_batch(batch):
    result = {k: copy.deepcopy(v) for k, v in batch.items()
              if k not in {'index', 'facts_sha256', 'data_observation_version'}}
    result['tables'] = [prompt_table(t) for t in batch['tables']]
    result['relationships'] = ordered([overlap(r) for r in batch.get('relationships', [])])
    result['enum_candidates'] = ordered([enum(e) for e in batch.get('enum_candidates', [])])
    return result


def rules_digest():
    root = Path(__file__).parent
    return digest({name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in (
        'library_semantics.py', 'library_selfbuild.py', 'relation_keys.py', 'library_metrics.py',
        'library_control.py', 'library_observation.py', 'model_provider.py',
        'library_assets.py', 'declared_enums.py', 'dictionary_evidence.py')})


def identity(batch, config, request_sha256):
    scope = config.get('semantic_scope')
    if scope is not None and (
            not isinstance(scope, dict) or set(scope) != {'project', 'role', 'principal', 'permissions_sha256'}
            or any(not isinstance(v, str) or not v for v in scope.values())
            or len(scope['permissions_sha256']) != 64
            or any(c not in '0123456789abcdef' for c in scope['permissions_sha256'])):
        raise ValueError('semantic_scope_invalid')
    return {
        'protocol': PROTOCOL, 'semantic_input_digest': batch['semantic_input_digest'],
        'request_sha256': request_sha256,
        'model': config.get('model', 'grok-4.7'),
        'model_revision': config.get('model_revision'),
        'provider': config.get('endpoint', '').rstrip('/'),
        'rules_sha256': rules_digest(), 'rules_version': config.get('semantic_rules_version'),
        'scope': scope,
        # Missing authorization context must not allow cross-generation reuse.
        'unscoped_facts': batch['facts_sha256'] if scope is None else None,
    }


def without(value, keys):
    return {k: copy.deepcopy(v) for k, v in value.items() if k not in keys}


def oracle_data_only(value):
    """Project known C1 transport evidence; unknown fields remain dependencies."""
    result = copy.deepcopy(value)
    semantics = result['source_semantics']
    receipt = semantics['publication_receipt']
    if (result['protocol'] != 'oo.builder.oracle-source-observation/v1'
            or result['source_bound'] is not None
            or semantics['measurement_protocol'] != 'oo.builder.oracle-target-observation/v1'
            or receipt['position_codec'] != 'oracle-capture-sequence/v1'
            or receipt['consistency_grade'] != 'T' or receipt['cross_table_consistent'] is not True):
        raise ValueError('data_only_oracle_c1_required')
    volatile = {'run_id', 'source_generation', 'target_generation', 'input_digest',
                'stored_namespace', 'stored_physical'}
    result['binding'] = without(result['binding'], volatile | {'accepted_rows'})
    for key, physical in (('columns', 'stored_physical'), ('column_mappings', 'physical_name')):
        result['binding'][key] = [without(c, {physical}) for c in result['binding'][key]]
    result['pins'] = without(result['pins'], {'catalog_sha256', 'profile_sha256', 'semantics_sha256'})
    observed = result['source_profile']
    result['source_profile'] = without(observed, volatile | {'rows', 'observed_at', 'snapshot'})
    result['source_profile']['columns'] = [
        data_profile(dict(c, rows=observed['rows'])) for c in observed['columns']]
    semantics = without(semantics, {
        'source_generation', 'target_generation', 'binding_vector_sha256',
        'collection_sha256', 'measurement_sha256', 'profile_sha256'})
    semantics['binding_vector'] = [without(b, volatile) for b in semantics['binding_vector']]
    semantics['input_sha256'] = without(semantics['input_sha256'], {'catalog', 'publication'})
    semantics['source_clock'] = without(semantics['source_clock'], {'source_committed_at'})
    projected = without(receipt, {
        'run_id', 'run_ids', 'source_generation', 'target_generation', 'applied_pos'})
    projected['tables'] = [without(t, {
        'run_id', 'physical_name', 'source_generation', 'target_generation',
        'accepted_rows', 'canonical_digest', 'applied_pos'}) for t in receipt['tables']]
    projected['oracle_buffer'] = without(receipt['oracle_buffer'], {
        'final_cursor', 'commit_scn', 'context_sha256', 'bundle_sha256', 'spool_sha256',
        'connector_offsets_sha256', 'connector_history_sha256'})
    semantics['publication_receipt'] = projected
    target = semantics['target_observation']
    clocks = {'applied_at', 'last_pos', 'physical_name', 'source_committed_at',
              'source_generation', 'target_generation', 'applied_pos'}
    target['freshness'] = [without(v, clocks) for v in target['freshness']]
    # Historical run/receipt vectors are validated by the observation adapter,
    # not interpreted as schema. Current identity, quality and watermark stay.
    target['metadata'] = without(target['metadata'], {'full_import_run', 'native_publication_receipt'})
    target['metadata']['source_watermark'] = [
        without(v, clocks) for v in target['metadata']['source_watermark']]
    result['source_semantics'] = semantics
    return result


RETAINED = 'oo.builder.retained-generation-observation/v1'
RETAINED_STABLE = ('protocol', 'receipt_protocol', 'consistency', 'library_id', 'config_digest',
                   'key', 'expected_columns')


def retained_data_only(item):
    """D15 route: an oo-load retained cut is the data source for PG/MySQL/Oracle.

    Generation, receipt, recipe and relation identities change on every cut and
    are not semantics. Library, source binding, key and typed columns remain
    dependencies, so any shape change still requires a new S5 request/review.
    """
    retained = item.get('retained')
    if (not isinstance(retained, dict) or retained.get('protocol') != RETAINED
            or retained.get('receipt_protocol') != 'oo.load.per-table-receipt/v1'
            or retained.get('consistency') != 'retained_generation'
            or item.get('source_engine') not in ('postgres', 'mysql', 'oracle')
            or 'oracle' in item or any(k not in retained for k in RETAINED_STABLE)):
        raise ValueError('data_only_retained_generation_required')
    item['retained'] = {k: copy.deepcopy(retained[k]) for k in RETAINED_STABLE}


def data_profile(value):
    result = profile(value)
    if 'rows' in value:
        result['empty'] = value['rows'] == 0
        if 'non_null' in value:
            result['null_present'] = value['non_null'] < value['rows']
    if 'distinct' in value and 'non_null' in value:
        result['unique_non_null'] = value['distinct'] == value['non_null']
    return result


def data_only_digest(facts, *, mysql_ddl=False):
    """A separate equivalence proof, never a replacement for an S5 request hash."""
    projected = copy.deepcopy(facts)
    for collection in ('tables', 'all_tables'):
        if collection not in projected:
            continue
        for item in projected[collection]:
            if 'retained' in item:
                retained_data_only(item)
            elif item.get('source_engine') == 'oracle':
                item['oracle'] = oracle_data_only(item['oracle'])
            elif mysql_ddl and item.get('source_engine') in (None, 'mysql') and 'oracle' not in item:
                # Legacy MySQL facts are identified by their validated DDL route,
                # not by guessing from source IDs or target PostgreSQL types.
                ddl = item.get('ddl_sha256', '')
                if (not isinstance(ddl, str) or len(ddl) != 64
                        or any(c not in '0123456789abcdef' for c in ddl)
                        or not isinstance(item.get('table_options_sql'), str)
                        or not isinstance(item.get('constraints_sql'), list)
                        or any(not isinstance(c.get('source_type'), str)
                               or not isinstance(c.get('definition_sql'), str) for c in item['columns'])):
                    raise ValueError('data_only_mysql_ddl_required')
            else:
                raise ValueError('data_only_source_route_required')
            item.pop('mapping_namespace', None)
            for column in item['columns']:
                column['profile'] = data_profile(column['profile'])
    batch = {'tables': projected['tables']}
    bind_batch(batch, projected)
    # Report definitions are not consumed by S5 but can change the same Pack.
    # Key quality: uniqueness, duplicates and blanks of keys and relationship
    # ends are part of the proof a reused review relies on.
    return digest({
        'protocol': 'oo.library.data-only-semantics/v2',
        'dependencies': batch['dependency_manifest'],
        'reports': facts.get('report_definitions_observation'),
        'key_quality': relation_keys.key_quality(facts),
    })

def table_data_only_digests(facts):
    """Table-local review boundaries; leave the legacy v1 library hash unchanged."""
    result = {}
    for item in facts['tables']:
        name = item['dataset']
        local = dict(facts)
        local['tables'] = [item]
        local['all_tables'] = local['tables']
        # Cross-table inferred edges are redrafted at review, not a reason to
        # mark neighbors pending when a local comment changes. Declared FKs
        # remain bound in the declaring table itself. Library-level inputs
        # (inferred edges, cross-table enum bindings, reports) are compared by
        # library_data_only_digest instead of marking every table pending.
        local['relationship_candidates'] = []
        local.pop('report_definitions_observation', None)
        local['enum_candidates'] = [
            enum(e) for e in facts['enum_candidates'] if e['enum'].rsplit('.', 1)[0] == name]
        local['table_enums'] = copy.deepcopy(local['enum_candidates'])
        for candidate in local['enum_candidates']:
            candidate['dictionary_candidates'] = []
            candidate.pop('declared_binding', None)
        result[name] = data_only_digest(local)
    return result

def library_remainder(facts):
    """The non-table remainder of the semantic inputs, compared library-wide.

    Table digests drop inferred cross-table edges, cross-table enum dictionary
    bindings and report definitions. Those still feed semantic drafts, so a
    change here stops publication for owner review. Declared FK edges and
    column comments stay bound in their tables' digests; counts are
    normalized exactly as in the table digests, so unchanged sources are
    stable across generations.
    """
    relationships = []
    for item in facts['relationship_candidates']:
        if item.get('provenance') == 'declared':
            continue
        projected = overlap(item)
        for key in ('from_comment', 'to_comment'):
            projected.get('evidence', {}).pop(key, None)
        relationships.append(projected)
    bindings = []
    for item in facts['enum_candidates']:
        normalized = enum(item)
        if normalized['dictionary_candidates'] or 'declared_binding' in item:
            bindings.append({'enum': item['enum'],
                             'dictionary_candidates': normalized['dictionary_candidates'],
                             'declared_binding': item.get('declared_binding')})
    declared = facts.get('declared_enum_observation')
    dictionary = facts.get('dictionary_evidence')
    return {
        'relationship_candidates': ordered(relationships),
        'enum_bindings': ordered(bindings),
        'reports': facts.get('report_definitions_observation'),
        'declared_enum_config': declared.get('config_sha256') if declared else None,
        'dictionary': ({'input': dictionary['input'], 'document': dictionary['document']}
                       if dictionary else None),
        'extensions': extensions(facts),
    }


def library_data_only_digest(facts):
    return digest({'protocol': 'oo.library.data-only-remainder/v1', **library_remainder(facts)})


def library_review_pending(old, facts, library):
    """A library-scoped pending entry; it blocks the whole cycle, not one table."""
    before, after = library_remainder(old), library_remainder(facts)
    changes = [key for key in before if before[key] != after[key]]
    return [{'table': 'library:' + library, 'scope': 'library', 'changes': changes}] if changes else []


def table_review_pending(old, facts):
    before, after = table_data_only_digests(old), table_data_only_digests(facts)
    previous = {t['dataset']: t for t in old['tables']}
    current = {t['dataset']: t for t in facts['tables']}
    pending = []
    for name in sorted(before.keys() | after.keys()):
        if before.get(name) == after.get(name):
            continue
        changes = []
        if name not in before or name not in after:
            changes.append('table_added' if name in after else 'table_removed')
        else:
            a, b = previous[name], current[name]
            for key, label in (('table_comment', 'comment'), ('foreign_keys', 'foreign_keys'),
                               ('excluded_foreign_keys', 'foreign_keys'),
                               ('primary_key', 'primary_key')):
                if a.get(key) != b.get(key) and label not in changes:
                    changes.append(label)
            if a.get('unpublished_columns') != b.get('unpublished_columns'):
                changes.extend('unreviewed_column:' + c['name'] for c in b.get('unpublished_columns', []))
            ac = {c['name']: c for c in a['columns']}
            bc = {c['name']: c for c in b['columns']}
            for column in sorted(ac.keys() | bc.keys()):
                if column not in ac or column not in bc:
                    changes.append('column_' + ('added:' if column in bc else 'removed:') + column)
                elif any(ac[column].get(k) != bc[column].get(k)
                         for k in ('comment', 'data_type', 'nullable')):
                    changes.append('column_metadata:' + column)
        pending.append({'table': name, 'changes': changes or ['semantic_inputs']})
    return pending
