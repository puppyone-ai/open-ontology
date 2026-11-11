"""D15 retained-generation Builder route with a scripted read-only Agent DB.

Synthetic metadata only. Live cuts: tools/release-coordinator/tests/retained_cycle_live.py.
"""
import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import library_semantics as semantics
import retained_build as build
import retained_observation as observation


def cut(generation, rows=1, engine='postgres', data_type='bigint', column='amount'):
    run = 'cycle-%d' % generation
    physical = 'pt_' + ('%060x' % (1000 + generation))
    relation = 'g_' + ('%040x' % generation)
    stage = 's_' + ('%040x' % generation)
    schema, table = ('APP', 'ITEMS') if engine == 'oracle' else ('app', 'items')
    storage = ('q_415050', 'q_4954454d53') if engine == 'oracle' else ('app', 'items')
    source_column = column.upper() if engine == 'oracle' else column
    logical = 'q_' + source_column.encode().hex() if engine == 'oracle' else column
    recipe = {'protocol': 'oo.load.storage-generation/v1', 'definition': '"%s" %s' % (logical, data_type),
              'plan': {'protocol': 'oo.engine/v1', 'table': {'schema': schema, 'name': table},
                       'key': {'shape': 'integer_primary_key', 'column': source_column},
                       'strategy': {'single_read': None}, 'estimated_rows': rows, 'chunks': []},
              'parts': [{'stage': stage, 'receipt': {'rows': rows, 'chunk_id': generation}}]}
    recipe_sha = observation.rust_digest(recipe)
    tables = [{'schema': schema, 'table': table, 'storage_schema': storage[0], 'storage_table': storage[1],
               'run_id': run, 'physical_name': physical, 'accepted_rows': rows, 'recipe_sha256': recipe_sha,
               'source_generation': generation, 'target_generation': generation, 'applied_pos': '0',
               'capture_method': 'c5_diff', 'delete_detection': 'reconcile', 'consistency_grade': 'B'}]
    receipt = {'protocol': observation.RECEIPT_PROTOCOL, 'source_id': 'owned', 'run_id': run, 'run_ids': [run],
               'source_generation': generation, 'target_generation': generation,
               'position_codec': 'snapshot-zero/v1', 'position_lineage': 'oo-load.retained', 'applied_pos': '0',
               'consistency': 'retained_generation', 'consistency_grade': 'B',
               'cross_table_consistent': False, 'tables': tables}
    vector = [{'dataset': storage[0] + '.' + storage[1], 'source_id': 'owned', 'generation': generation,
               'recipe_sha256': recipe_sha, 'storage_relation': relation, 'physical_name': physical,
               'expected_columns': [{'name': logical, 'data_type': data_type}], 'source_committed_at': None}]
    candidate = {'generation': generation, 'run': run, 'library_id': 'owned', 'source': 'owned',
                 'receipt_sha256': hashlib.sha256(run.encode()).hexdigest(),
                 'request': {'source_id': 'owned', 'binding_sha256': 'c' * 64, 'tables': [], 'recipes': [recipe]},
                 'shard': {'source_id': 'owned', 'anchor_run': run, 'receipt_sha256': observation.rust_digest(receipt),
                           'receipt': receipt},
                 'data_vector': vector, 'data_vector_sha256': observation.rust_digest(vector),
                 'proofs_current': True, 'proof_count': 3}
    mapped = 'pc_' + ('%060x' % generation)
    return {'candidate': candidate,
            'mappings': [{'source': 'owned', 'logical_namespace': storage[0], 'logical_table': storage[1],
                          'logical_column': logical, 'physical_name': mapped, 'scope_table_physical': physical,
                          'level': 'c'}],
            'attributes': [{'relname': physical, 'attname': mapped, 'data_type': data_type, 'not_null': True}]}


class Db:
    def __init__(self, metadata, rows=1, distinct=1):
        self.metadata, self.rows, self.distinct, self.queries = metadata, rows, distinct, []

    def query(self, sql):
        self.queries.append(sql)
        if sql.startswith("select json_build_object('candidate'"):
            return copy.deepcopy(self.metadata)
        if "'columns'," in sql and 'pg_current_snapshot' in sql:
            name = self.metadata['mappings'][0]['logical_column']
            return {'rows': self.rows, 'snapshot': '1:1:', 'observed_at': 'now', 'columns': [
                {'name': name, 'non_null': self.rows, 'distinct': self.distinct,
                 'min_length': 2 if self.rows else None, 'max_length': 2 if self.rows else None,
                 'blank_rows': 0, 'digits_rows': self.rows, 'leading_zero_rows': 0}]}
        if "'entropy'" in sql:
            return dict(rows=self.rows, non_null=self.rows, distinct=self.distinct,
                        readable=0, phone=0, id_card=0, email=0, bank_card=0, entropy=0)
        if "'distinct',count(distinct ROW(" in sql:
            return dict(rows=self.rows, non_null=self.rows, distinct=self.distinct)
        raise AssertionError('unexpected query')


def facts(metadata, engine='postgres', generation=None, **db):
    run = generation or (metadata['candidate'] or {}).get('run', 'cycle-1')
    config = {'library_id': 'owned', 'generation': run, 'source_engine': engine}
    return build.facts_for(Db(metadata, **db), config)[0]


class RetainedRoute(unittest.TestCase):
    def test_each_engine_is_data_only_across_generations(self):
        for engine, data_type in [('postgres', 'bigint'), ('mysql', 'bigint'), ('oracle', 'text')]:
            old = facts(cut(1, engine=engine, data_type=data_type), engine)
            new = facts(cut(2, engine=engine, data_type=data_type), engine)
            self.assertNotEqual(old['facts_sha256'], new['facts_sha256'], engine)
            self.assertEqual(semantics.data_only_digest(old), semantics.data_only_digest(new), engine)
            self.assertEqual(old['tables'][0]['retained']['protocol'], semantics.RETAINED)
        oracle = facts(cut(1, engine='oracle', data_type='text'), 'oracle')
        self.assertEqual(oracle['tables'][0]['primary_key'], ['q_414d4f554e54'])
        self.assertEqual(oracle['tables'][0]['columns'][0]['source_name'], 'AMOUNT')

    def test_shape_and_identity_changes_are_semantic(self):
        base = semantics.data_only_digest(facts(cut(1)))
        for changed in [facts(cut(2, data_type='numeric')), facts(cut(2, column='total')),
                        facts(cut(2, rows=0), distinct=0, rows=0)]:
            self.assertNotEqual(base, semantics.data_only_digest(changed))
        other_source = cut(2)
        other_source['candidate']['request']['binding_sha256'] = 'd' * 64
        self.assertNotEqual(base, semantics.data_only_digest(facts(other_source)))

    def test_legacy_refusals_remain_and_retained_route_is_closed(self):
        current = facts(cut(1))
        for mutate, code in [
            (lambda t: t['retained'].update(protocol='x'), 'data_only_retained_generation_required'),
            (lambda t: t['retained'].update(consistency='per_table'), 'data_only_retained_generation_required'),
            (lambda t: t.update(source_engine='sqlserver'), 'data_only_retained_generation_required'),
            (lambda t: t.update(oracle={}), 'data_only_retained_generation_required'),
            (lambda t: t['retained'].pop('key'), 'data_only_retained_generation_required'),
            (lambda t: t.pop('retained'), 'data_only_source_route_required'),
        ]:
            altered = copy.deepcopy(current)
            mutate(altered['tables'][0])
            altered['all_tables'] = altered['tables']
            with self.assertRaisesRegex(ValueError, code):
                semantics.data_only_digest(altered)
        with self.assertRaisesRegex(ValueError, 'data_only_source_route_required'):
            semantics.data_only_digest({'tables': [{'source_engine': 'postgres'}]})

    def test_observation_refuses_unsealed_or_drifted_cuts(self):
        for mutate, code in [
            (lambda m: m.update(candidate=None), 'retained_candidate_missing'),
            (lambda m: m['candidate'].update(proofs_current=False), 'retained_relation_proof_drift'),
            (lambda m: m['candidate']['shard']['receipt'].update(consistency='transactional'),
             'retained_candidate_binding'),
            (lambda m: m['candidate']['request']['recipes'][0].update(definition='x'), 'retained_recipe_binding'),
            (lambda m: m['attributes'][0].update(data_type='text'), 'retained_column_type_drift'),
            (lambda m: m['mappings'].clear(), 'retained_column_mapping'),
        ]:
            metadata = cut(1)
            mutate(metadata)
            with self.assertRaisesRegex(ValueError, code):
                facts(metadata)
        with self.assertRaisesRegex(ValueError, 'retained_row_count_drift'):
            facts(cut(1), rows=2, distinct=2)
        with self.assertRaisesRegex(ValueError, 'retained_engine'):
            facts(cut(1), engine='sqlite')


class RetainedBuild(unittest.TestCase):
    def test_resume_copies_only_receipts_without_changing_failure_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            old = root / 'builder-failed-1/community/receipts'
            old.mkdir(parents=True)
            raw = b'{"accepted_hash":"must still be checked by Runner"}'
            name = 'a' * 64 + '.json'
            (old / name).write_bytes(raw)
            (old.parent.parent / 'facts.json').write_text('not reusable')
            output = root / 'builder'
            output.mkdir()
            build.restore_receipts(output)
            self.assertEqual((output / 'community/receipts' / name).read_bytes(), raw)
            self.assertEqual((old / name).read_bytes(), raw)
            self.assertFalse((output / 'facts.json').exists())
            self.assertEqual((output / 'community/receipts' / name).stat().st_mode & 0o777, 0o600)

    def test_resume_refuses_symlinks_and_unexpected_names(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'builder-failed-1/community/receipts'
            source.mkdir(parents=True)
            (source / 'unexpected.json').write_text('{}')
            output = root / 'builder'
            output.mkdir()
            with self.assertRaisesRegex(ValueError, 'retained_cache_name'):
                build.restore_receipts(output)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'builder-failed-1').symlink_to(root, target_is_directory=True)
            output = root / 'builder'
            output.mkdir()
            with self.assertRaisesRegex(ValueError, 'retained_cache_symlink'):
                build.restore_receipts(output)

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_model_egress_and_client_use_300_second_timeout(self):
        import selfbuild_budget
        stack = self.root / 'stack.json'
        stack.write_text(json.dumps({'model': {'model': 'test-model'}}))
        config = {'model': {'stack': str(stack), 'egress': 'unused', 'call_limit': 3}}
        process = Mock()
        process.poll.return_value = None
        with patch.object(build.subprocess, 'Popen', return_value=process) as spawn, patch.object(
                build.socket, 'create_connection'), patch.object(selfbuild_budget, 'Runner') as runner:
            runner.return_value.run.return_value = {'unresolved': [], 'published': False}
            build.draft({'tables': []}, config, self.root)
        argv = spawn.call_args.args[0]
        self.assertEqual(argv[argv.index('--timeout') + 1], '300')
        self.assertEqual(runner.call_args.args[0]['request_timeout_seconds'], 300)
        process.terminate.assert_called_once()

    def pin(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value))
        return {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}

    def config(self, generation, origin=None):
        value = {'protocol': build.PROTOCOL, 'psql_command': ['unused'], 'library_id': 'owned',
                 'generation': 'cycle-%d' % generation, 'source_engine': 'postgres', 'project': 'owned'}
        if origin:
            value['origin'] = origin
        path = self.root / ('config-%d.json' % generation)
        path.write_text(json.dumps(value))
        return path

    def reviewed(self, data_type='bigint'):
        return {'protocol': 'oo.owner-reviewed-registry/v1', 'status': 'reviewed',
                'review_decision': {'reviewer': 'owner', 'mode': 'self'},
                'compiled': {'effective': {'datasets': [{'dataset': 'app.items', 'columns': [
                    {'name': 'amount', 'data_type': data_type}]}]}}}

    def run_build(self, generation, origin=None):
        original = build.review_input
        def review_input(tables, draft, project):
            # Supply the real graph shape omitted by the old replay stubs.
            return original(tables, dict(draft, graph={"edges": []}), project)
        with patch.object(build.lib, 'ReadOnlyPostgres', return_value=Db(cut(generation))), patch.object(
                build, 'review_input', side_effect=review_input):
            return build.run(self.config(generation, origin), self.root / ('out-%d' % generation))

    def test_data_only_cycle_reuses_review_with_zero_model_calls(self):
        with patch.object(build, 'draft', return_value=({'protocol': 'oo.selfbuild.community-draft/v1'}, 2)) as model:
            first = self.run_build(1)
        self.assertEqual((first['stage'], first['model_calls']), ('review_required', 2))
        command = json.loads((self.root / 'out-1/review-input.json').read_text())
        self.assertEqual(command['effective']['datasets'][0]['columns'][0]['data_type'], 'bigint')
        self.assertEqual(command['visibility']['role'], 'oo_r_owner')
        decisions = json.loads((self.root / 'out-1/adjudication.json').read_text())
        self.assertEqual(decisions['input_sha256'], hashlib.sha256(
            (self.root / 'out-1/review-input.json').read_bytes()).hexdigest())
        self.assertEqual({i['class'] for i in decisions['items']}, {'sensitive', 'entity_key'})
        origin = {'facts': self.pin('facts.json', json.loads((self.root / 'out-1/facts.json').read_text())),
                  'reviewed_registry': self.pin('reviewed.json', self.reviewed())}
        with patch.object(build, 'draft', side_effect=AssertionError('model called')):
            second = self.run_build(2, origin)
        self.assertEqual((second['stage'], second['model_calls']), ('data_only', 0))
        reuse = json.loads((self.root / 'out-2/data-only-reuse.json').read_text())
        self.assertEqual(reuse['new_model_calls'], 0)
        self.assertEqual(reuse['reviewed_registry_sha256'], origin['reviewed_registry']['sha256'])
        model.assert_called_once()

    def test_semantic_change_or_bad_origin_never_reuses_review(self):
        with patch.object(build, 'draft', return_value=({}, 1)):
            self.run_build(1)
        old = json.loads((self.root / 'out-1/facts.json').read_text())
        old['tables'][0]['columns'][0]['data_type'] = 'numeric'
        origin = {'facts': self.pin('facts.json', old), 'reviewed_registry': self.pin('r.json', self.reviewed())}
        with patch.object(build, 'draft', return_value=({}, 1)) as model:
            result = self.run_build(2, origin)
        self.assertEqual((result['stage'], result['semantic_change']), ('review_required', True))
        model.assert_called_once()
        origin['facts']['sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'retained_origin_digest'):
            self.run_build(3, origin)

    def test_reviewed_shape_must_match_the_new_cut(self):
        current = facts(cut(2))
        with self.assertRaisesRegex(ValueError, 'retained_origin_shape_drift'):
            build.reviewed_shape(self.reviewed('text'), current)
        bad = self.reviewed()
        bad['review_decision']['mode'] = 'independent'
        with self.assertRaisesRegex(ValueError, 'retained_origin_review_required'):
            build.reviewed_shape(bad, current)


    def test_confirmed_origin_replays_compiler_before_data_only_reuse(self):
        current = facts(cut(1))
        command = build.review_input(build.inventory(current), {'graph': {'edges': []}}, 'test')
        command['adjudication_replica'] = cut(1)
        raw = build.lib.canonical(command)
        document = build.adjudication.collect(
            Db(cut(1)), command, current['tables'], hashlib.sha256(raw.encode()).hexdigest())
        overrides = dict(protocol='oo.adjudication-override/v1',
                         summary_sha256=document['summary_sha256'], items={})
        confirmation = dict(protocol='oo.publish-confirm/v1',
                            summary_sha256=document['summary_sha256'],
                            overrides_sha256=build.lib.digest(overrides),
                            reason='Synthetic confirmation', confirmed_at='2026-11-09T12:00:00Z')
        envelope = dict(adjudication_input=raw, adjudication=document,
                        overrides=overrides, confirmation=confirmation)
        compiled = build.lib.selfbuild_budget.bridge('reviewed', command=envelope)
        reviewed = dict(protocol='oo.owner-reviewed-registry/v1', status='reviewed',
                        published=False, authentication='not_claimed', review_decision=confirmation,
                        command=envelope, compiled=compiled)
        origin = dict(facts=self.pin('facts.json', current),
                      reviewed_registry=self.pin('reviewed.json', reviewed))
        with patch.object(build, 'draft', side_effect=AssertionError('model called')):
            result = self.run_build(2, origin)
        self.assertEqual((result['stage'], result['model_calls']), ('data_only', 0))
        compiled['effective']['datasets'][0]['columns'][0]['data_type'] = 'text'
        with self.assertRaisesRegex(ValueError, 'retained_origin_compiled_mismatch'):
            build.reviewed_shape(reviewed, current)
        confirmation['summary_sha256'] = '0' * 64
        with self.assertRaises(ValueError):
            build.reviewed_shape(reviewed, current)

    def test_changed_retained_cut_never_writes_adjudication(self):
        config = self.config(1)
        current, before = build.facts_for(Db(cut(1)), json.loads(config.read_text()))
        after = dict(before, receipt_sha256='0' * 64)
        with patch.object(build.lib, 'ReadOnlyPostgres', return_value=Db(cut(1))), \
                patch.object(build, 'facts_for', side_effect=[(current, before), (current, after)]), \
                patch.object(build, 'draft', return_value=({'graph': {'edges': []}}, 0)):
            with self.assertRaisesRegex(ValueError, 'adjudication_retained_cut_changed'):
                build.run(config, self.root / 'changed-cut')
        self.assertFalse((self.root / 'changed-cut/adjudication.json').exists())

if __name__ == '__main__':
    unittest.main()
