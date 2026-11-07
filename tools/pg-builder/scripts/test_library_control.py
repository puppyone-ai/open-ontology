import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from observation_test_support import observed_fixture

spec = importlib.util.spec_from_file_location('library_control', Path(__file__).with_name('library_control.py'))
control = importlib.util.module_from_spec(spec)
spec.loader.exec_module(control)


class LibraryControlTest(unittest.TestCase):
    @staticmethod
    def scheduling_cache(output, batch, config, failure=False):
        # Scheduling tests use synthetic batches; semantic binding is tested separately.
        return output / (('failure-' if failure else 'draft-') + str(batch['index']) + '.json')

    def test_readable_catalog_keeps_field_descriptions_without_approving_business_meanings(self):
        facts = self.facts()
        review = {'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
                  'columns': {'app.items.id': {'pii': 'none', 'semantic_type': 'identifier',
                                              'reviewer': 'operator'}},
                  'hidden_columns': ['app.items.phone'], 'datasets': {}}
        effective, visibility, audit = control.reviewed_documents(facts, [], review)
        self.assertIn('column_descriptions', effective['ontology']['publish']['include_unreviewed'])
        column = effective['datasets'][0]['columns'][0]
        self.assertEqual(column['description'], facts['tables'][0]['columns'][0]['comment'])
        self.assertNotIn('review', column)
        self.assertEqual(visibility['datasets'][0]['columns'], ['id'])
        self.assertFalse(audit['business_owner_signed'])

    def test_source_label_is_preserved_but_model_label_requires_review(self):
        facts = self.facts()
        facts['tables'][0]['table_comment'] = 'Item ledger'
        receipt = {'draft': {'datasets': [
            {'dataset': 'app.items', 'label': 'Model guess', 'columns': []}]}}
        review = {'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
                  'columns': {}, 'hidden_columns': [], 'datasets': {}}
        effective, _, _ = control.reviewed_documents(facts, [receipt], review)
        self.assertEqual(effective['datasets'][0]['label'], 'Item ledger')
        review['labels'] = {'app.items': 'Reviewed label'}
        effective, _, _ = control.reviewed_documents(facts, [receipt], review)
        self.assertEqual(effective['datasets'][0]['label'], 'Reviewed label')
        review['datasets'] = {'app.items': {'label': True}}
        effective, _, _ = control.reviewed_documents(facts, [receipt], review)
        self.assertEqual(effective['datasets'][0]['label'], 'Model guess')

    def test_grain_survives_only_its_explicit_review_and_is_accounted_for(self):
        facts = self.facts()
        receipt = {'draft': {'datasets': [
            {'dataset': 'app.items', 'grain': 'One item per id', 'columns': []}]}}
        review = {'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'reader',
                  'columns': {}, 'hidden_columns': [], 'datasets': {
                      'app.items': {'description': True, 'reviewer': 'fixture/test'}}}
        effective, _, audit = control.reviewed_documents(facts, [receipt], review)
        self.assertNotIn('grain', effective['datasets'][0])
        self.assertEqual(audit['grain_projection'][0]['state'], 'pending')
        review['datasets']['app.items']['grain'] = True
        effective, visibility, audit = control.reviewed_documents(facts, [receipt], review)
        self.assertEqual(effective['datasets'][0]['grain'], 'One item per id')
        self.assertEqual(audit['grain_projection'][0]['state'], 'projected')
        self.assertEqual(visibility['datasets'], [])
        self.assertFalse(audit['business_owner_signed'])
        from oos_tree import source_tree
        generated, _, _ = control.lib.effective_documents(facts, [receipt['draft']])
        files, _ = source_tree(generated, effective)
        self.assertNotIn('grain', files['generated/datasets/app/items.yaml'])
        self.assertEqual(files['curated/datasets/app/items.yaml']['grain'], 'One item per id')
        for invalid in ({'grain': True}, {'grain': True, 'reviewer': ''}):
            review['datasets']['app.items'] = invalid
            with self.assertRaisesRegex(ValueError, 'grain_review_incomplete'):
                control.reviewed_documents(facts, [receipt], review)
        review['datasets']['app.items'] = {'grain': True, 'reviewer': 'fixture/test'}
        with self.assertRaisesRegex(ValueError, 'grain_review_incomplete'):
            control.reviewed_documents(facts, [], review)
        review['datasets'] = {}
        _, _, audit = control.reviewed_documents(facts, [], review)
        self.assertEqual(audit['grain_projection'][0]['state'], 'absent')

    def facts(self):
        tables = [{'dataset': 'app.items', 'source_id': 'shard_a', 'source_generation': 1,
                   'target_generation': 2, 'primary_key': ['id'], 'columns': [
                       {'name': 'id', 'data_type': 'bigint', 'comment': 'identifier',
                        'profile': {'rows': 3, 'distinct': 3}},
                       {'name': 'phone', 'data_type': 'text', 'comment': 'phone',
                        'profile': {'rows': 3, 'distinct': 3}}]}]
        return control.lib.build_facts(observed_fixture(tables), lambda *a: {})

    def test_parallel_receipts_are_ordered_and_concurrency_is_bounded(self):
        import threading,time
        batches=[{'index':i,'facts_sha256':'facts'}for i in range(8)]
        active=0;peak=0;lock=threading.Lock()
        def provider(batch,config):
            nonlocal active,peak
            with lock:active+=1;peak=max(peak,active)
            time.sleep(.01*(8-batch['index']))
            with lock:active-=1
            return {'batch':batch['index'],'facts_sha256':'facts','model':'grok-4.7'}
        with tempfile.TemporaryDirectory()as root,patch.object(control.lib,'draft_batches',return_value=batches),patch.object(control.lib,'call_model',side_effect=provider),patch.object(control.lib,'draft_cache_path',side_effect=self.scheduling_cache):
            receipts,failures=control.drafts(self.facts(), {'model':'grok-4.7','max_draft_batches':8,'s5_concurrency':4},Path(root))
        self.assertGreater(peak,1);self.assertLessEqual(peak,4)
        self.assertEqual([r['batch']for r in receipts],list(range(8)));self.assertEqual(failures,[])

    def test_drafts_default_to_one_worker_and_reject_invalid_budgets(self):
        from concurrent.futures import ThreadPoolExecutor
        with tempfile.TemporaryDirectory() as root, \
                patch.object(control, 'ThreadPoolExecutor', wraps=ThreadPoolExecutor) as pool, \
                patch.object(control.lib, 'draft_batches', return_value=[]):
            control.drafts(self.facts(), {'model': 'fixture/test'}, Path(root))
            pool.assert_called_once_with(max_workers=1)
            for invalid in (True, 0, 33, "8"):
                with self.subTest(invalid=invalid), self.assertRaisesRegex(ValueError, 's5_concurrency_budget'):
                    control.drafts(self.facts(), {'model': 'fixture/test', 's5_concurrency': invalid}, Path(root))

    def test_retry_cooldown_is_local_to_failed_batch(self):
        import threading
        sleeping=threading.Event();other_done=threading.Event();calls=[]
        def provider(batch,config):
            calls.append(batch['index'])
            if batch['index']==0:raise ValueError('s5_coverage')
            other_done.set();return {'batch':1,'facts_sha256':'facts','model':'grok-4.7'}
        def sleep(delay):
            if delay>=60:
                sleeping.set();self.assertTrue(other_done.wait(2),'healthy batch blocked by unrelated cooldown')
        with tempfile.TemporaryDirectory()as root,patch.object(control.lib,'draft_batches',return_value=[{'index':i,'facts_sha256':'facts'}for i in range(2)]),patch.object(control.lib,'call_model',side_effect=provider),patch.object(control.time,'sleep',side_effect=sleep),patch.object(control.lib,'draft_cache_path',side_effect=self.scheduling_cache):
            receipts,failures=control.drafts(self.facts(), {'model':'grok-4.7','max_draft_batches':2,'s5_concurrency':2},Path(root))
        self.assertTrue(sleeping.is_set());self.assertEqual(calls.count(0),3)
        self.assertEqual(len(receipts),1);self.assertEqual(failures[0]['reason'],'s5_coverage')

    def test_weak_unmeasured_relationships_do_not_fragment_whole_library_batches(self):
        facts=self.facts();prototype=facts['tables'][0];facts['tables']=[]
        for i in range(658):
            table=json.loads(json.dumps(prototype));table['dataset']=f'app.t_{i:03}';facts['tables'].append(table)
        strong={'name':'cross_batch','from':{'dataset':'app.t_000','columns':['id']},'to':{'dataset':'app.t_657','columns':['id']},'confidence':'0.99','evidence':{'method':'declared','containment':'1'}}
        weak=[dict(strong,name=f'weak_{i}',confidence='0.50',evidence={'method':'empty_table','containment':'0'},description='unmeasured '*30)for i in range(1000)]
        facts['relationship_candidates']=[strong,*weak]
        batches=control.lib.draft_batches(facts)
        self.assertEqual(len(batches),42);self.assertEqual(len(facts['relationship_candidates']),1001)
        for batch in batches:
            self.assertLessEqual(len(batch['tables']),16);self.assertLessEqual(len(control.lib.canonical(batch).encode()),240000)
        self.assertIn(strong,batches[0]['relationships']);self.assertIn(strong,batches[-1]['relationships'])
        self.assertFalse(any(r['name'].startswith('weak_')for b in batches for r in b['relationships']))

    def test_s5_enum_context_is_bounded_without_changing_deterministic_values(self):
        facts=self.facts();facts['enum_candidates']=[{'enum':'app.items.id','status':'unexplained','values':[{'code':str(i),'rows':1}for i in range(20000)],'dictionary_candidates':[],'evidence':{'method':'aggregate'}}]
        batch=control.lib.draft_batches(facts)[0]
        self.assertLessEqual(len(control.lib.canonical(batch).encode()),240000)
        self.assertEqual(len(facts['enum_candidates'][0]['values']),20000)
        self.assertEqual(batch['enum_candidates'][0]['observed_code_count'],20000)

    def test_review_projection_never_approves_hidden_or_unreviewed_columns(self):
        facts = self.facts()
        effective, visibility, audit = control.reviewed_documents(facts, [], {
            'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'test_reader',
            'columns': {'app.items.id': {'pii': 'none', 'semantic_type': 'identifier',
                                        'reviewer': 'fixture/test', 'label_approved': True}},
            'hidden_columns': [], 'labels': {}, 'relationships': []})
        self.assertEqual(visibility['datasets'][0]['columns'], ['id'])
        self.assertFalse(audit['business_owner_signed'])
        self.assertEqual(effective['datasets'][0]['columns'][1]['classification']['state'], 'pending')
        self.assertEqual(effective['identity'] if 'identity' in effective else effective['project'], 'fixture')

    def test_sensitive_fact_cannot_be_downgraded_by_review(self):
        with self.assertRaisesRegex(ValueError, 'classification_conflict'):
            control.reviewed_documents(self.facts(), [], {
                'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'test_reader',
                'columns': {'app.items.phone': {'pii': 'none', 'semantic_type': 'free_text',
                                               'reviewer': 'fixture/test', 'label_approved': True}},
                'hidden_columns': [], 'labels': {}, 'relationships': []})

    def pii_review(self, source_pii, reviewed_pii=None):
        facts = self.facts()
        facts['tables'][0]['columns'][1]['classification_candidate']['pii'] = source_pii
        semantic = facts['tables'][0]['columns'][1]['classification_candidate']['semantic_type']
        review = {
            'project': 'fixture', 'role': 'oo_r_reader', 'principal': 'test_reader',
            'review_origin': 'operator_pinned_file', 'hidden_columns': [],
            'columns': {'app.items.phone': {
                'pii': reviewed_pii or source_pii, 'semantic_type': semantic,
                'reviewer': 'operator_full_read_scope', 'explicit_allow': True,
                'label_approved': False}}}
        return facts, review

    def test_declared_pii_read_grant_preserves_class_and_is_bound_to_review(self):
        for pii in ('personal', 'sensitive', 'restricted'):
            with self.subTest(pii=pii):
                facts, review = self.pii_review(pii)
                before = control.lib.canonical(facts)
                effective, visibility, audit = control.reviewed_documents(facts, [], review)
                column = effective['datasets'][0]['columns'][1]
                self.assertEqual(column['pii'], pii)
                self.assertEqual(column['classification'],
                                 {'state': 'confirmed', 'by': 'operator_full_read_scope'})
                self.assertNotIn('review', column)
                self.assertEqual(visibility['datasets'][0]['columns'], ['phone'])
                self.assertEqual(audit['review_document_sha256'], control.lib.digest(review))
                self.assertEqual(audit['pii_read_grants'], [{
                    'column': 'app.items.phone', 'source_pii': pii, 'pii': pii,
                    'reviewer': 'operator_full_read_scope', 'explicit_allow': True}])
                self.assertFalse(audit['business_owner_signed'])
                self.assertFalse(audit['ledger_bound'])
                self.assertEqual(control.lib.canonical(facts), before)

    def test_pii_requires_boolean_explicit_allow_and_a_named_reviewer(self):
        for pii in ('personal', 'sensitive', 'restricted'):
            for allow in (None, False, 'true', 1, [], {}):
                with self.subTest(pii=pii, allow=allow):
                    facts, review = self.pii_review(pii)
                    decision = review['columns']['app.items.phone']
                    if allow is None:
                        del decision['explicit_allow']
                    else:
                        decision['explicit_allow'] = allow
                    with self.assertRaisesRegex(ValueError, 'pii_read_grant_required'):
                        control.reviewed_documents(facts, [], review)
        for reviewer in (None, '', '  ', True):
            facts, review = self.pii_review('personal')
            review['columns']['app.items.phone']['reviewer'] = reviewer
            with self.subTest(reviewer=reviewer), self.assertRaisesRegex(
                    ValueError, 'classification_reviewer_required'):
                control.reviewed_documents(facts, [], review)
        facts, review = self.pii_review('none')
        with self.assertRaisesRegex(ValueError, 'pii_read_grant_required'):
            control.reviewed_documents(facts, [], review)
        review['columns']['app.items.phone']['explicit_allow'] = False
        effective, _, audit = control.reviewed_documents(facts, [], review)
        self.assertEqual(effective['datasets'][0]['columns'][1]['pii'], 'none')
        self.assertEqual(audit['pii_read_grants'], [])

    def test_unknown_requires_per_column_conservative_review_and_retains_evidence(self):
        facts, review = self.pii_review('unknown', 'sensitive')
        decision = review['columns']['app.items.phone']
        for reason in (None, '', '  ', True):
            decision['reason'] = reason
            with self.subTest(reason=reason), self.assertRaisesRegex(
                    ValueError, 'pii_conservative_review_required'):
                control.reviewed_documents(facts, [], review)
        decision['reason'] = 'Read scope authorized; unknown conservatively sensitive, not semantic review.'
        for pii in ('none', 'personal', 'restricted', 'unknown'):
            decision['pii'] = pii
            with self.subTest(pii=pii), self.assertRaisesRegex(
                    ValueError, 'pii_conservative_review_required'):
                control.reviewed_documents(facts, [], review)
        decision['pii'] = 'sensitive'
        decision['explicit_allow'] = False
        with self.assertRaisesRegex(ValueError, 'pii_read_grant_required'):
            control.reviewed_documents(facts, [], review)
        decision['explicit_allow'] = True
        effective, visibility, audit = control.reviewed_documents(facts, [], review)
        column = effective['datasets'][0]['columns'][1]
        self.assertEqual(column['pii'], 'sensitive')
        self.assertEqual(column['semantic_type'], decision['semantic_type'])
        self.assertNotIn('review', column)
        self.assertEqual(visibility['datasets'][0]['columns'], ['phone'])
        self.assertEqual(audit['pii_read_grants'][0]['source_pii'], 'unknown')
        self.assertEqual(audit['pii_read_grants'][0]['reason'], decision['reason'])
        self.assertEqual(facts['tables'][0]['columns'][1]['classification_candidate']['pii'], 'unknown')
        self.assertFalse(audit['business_owner_signed'])

    def test_pii_grant_does_not_override_hidden_missing_or_conflicting_review(self):
        for pii in ('personal', 'unknown'):
            facts, review = self.pii_review(pii, 'sensitive' if pii == 'unknown' else pii)
            review['columns']['app.items.phone']['reason'] = 'Explicit read scope only.'
            review['hidden_columns'] = ['app.items.phone']
            effective, visibility, audit = control.reviewed_documents(facts, [], review)
            self.assertEqual(visibility['datasets'], [])
            self.assertEqual(audit['pii_read_grants'], [])
            self.assertEqual(effective['datasets'][0]['columns'][1]['classification']['state'], 'pending')
            review['hidden_columns'] = []
            review['columns'] = {}
            effective, visibility, _ = control.reviewed_documents(facts, [], review)
            self.assertEqual(visibility['datasets'], [])
            self.assertEqual(effective['datasets'][0]['columns'][1]['classification']['state'], 'pending')
        for source, selected in (('sensitive', 'personal'), ('restricted', 'none'), ('invalid', 'invalid')):
            facts, review = self.pii_review(source, selected)
            with self.assertRaisesRegex(ValueError, 'classification_conflict'):
                control.reviewed_documents(facts, [], review)
        facts, review = self.pii_review('personal')
        review['columns']['app.items.phone']['semantic_type'] = 'identifier'
        with self.assertRaisesRegex(ValueError, 'classification_conflict'):
            control.reviewed_documents(facts, [], review)

    def test_checkpoint_replay_refuses_modified_artifact(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'facts.json'
            control.save(path, {'version': 1})
            control.save(path, {'version': 1})
            with self.assertRaisesRegex(ValueError, 'artifact_drift'):
                control.save(path, {'version': 2})

    def test_failed_batch_does_not_prevent_later_batches_and_retries_are_bounded(self):
        facts = self.facts()
        batches = [{'index': 0, 'facts_sha256': facts['facts_sha256']},
                   {'index': 1, 'facts_sha256': facts['facts_sha256']}]
        calls = []
        def provider(batch, config):
            calls.append(batch['index'])
            if batch['index'] == 0:
                raise RuntimeError('provider_failure')
            return {'model': 'fixture/test', 'facts_sha256': facts['facts_sha256'], 'draft': {'datasets': []}}
        with tempfile.TemporaryDirectory() as root, patch.object(control.lib, 'draft_batches', return_value=batches), patch.object(control.lib, 'call_model', side_effect=provider), patch.object(control.time, 'sleep') as sleeper, patch.object(control.lib, 'draft_cache_path', side_effect=self.scheduling_cache):
            receipts, failures = control.drafts(facts, {'model': 'fixture/test', 'max_draft_batches': 2, 's5_concurrency': 1}, Path(root))
            self.assertEqual(calls, [0, 0, 0, 1])
            self.assertEqual(len(receipts), 1)
            self.assertEqual(failures[0]['attempts'], 3)
            self.assertEqual(failures[0]['batch'], 0)
            self.assertGreaterEqual(sum(call.args[0] >= 60 for call in sleeper.call_args_list), 3)

    def test_provider_quota_stops_new_batches_and_resume_without_sleeping(self):
        facts = self.facts()
        batches = [{'index': i, 'facts_sha256': facts['facts_sha256']} for i in range(3)]
        failure = control.lib.model_provider.http_failure(
            429, b'{"error":{"type":"usage_limit_reached"}}')
        with tempfile.TemporaryDirectory() as root, \
                patch.object(control.lib, 'draft_batches', return_value=batches), \
                patch.object(control.lib, 'call_model', side_effect=failure) as provider, \
                patch.object(control.time, 'sleep') as sleeper, \
                patch.object(control.lib, 'draft_cache_path', side_effect=self.scheduling_cache):
            config = {'model': 'fixture/test', 'max_draft_batches': 3, 's5_concurrency': 1}
            receipts, failed = control.drafts(facts, config, Path(root))
            self.assertEqual(receipts, [])
            self.assertEqual(provider.call_count, 1)
            self.assertEqual(len(failed), 3)
            self.assertEqual(failed[0]['attempts'], 1)
            self.assertFalse(failed[0]['retryable'])
            self.assertEqual([f['attempts'] for f in failed[1:]], [0, 0])
            receipts, failed_again = control.drafts(facts, config, Path(root))
            self.assertEqual(receipts, [])
            self.assertEqual(provider.call_count, 1)
            self.assertEqual(failed_again[0], failed[0])
            self.assertTrue(all(call.args[0] == 0 for call in sleeper.call_args_list))

    def test_standalone_provider_quota_resume_refuses_without_another_call(self):
        facts = self.facts()
        config = {'model': 'fixture/test', 'draft_datasets': [facts['tables'][0]['dataset']]}
        failure = control.lib.model_provider.http_failure(
            429, b'{"error":{"code":"model_cooldown"}}')
        with tempfile.TemporaryDirectory() as root, \
                patch.object(control.lib, 'call_model', side_effect=failure) as provider, \
                patch.object(control.lib, 'time', wraps=control.lib.time) as module_time:
            # Patch only the module's own time binding: patching the global
            # time.sleep also counted unrelated sleeps (stdlib waits, other
            # threads) and made this check flaky on loaded CI runners.
            sleeper = module_time.sleep
            with self.assertRaisesRegex(RuntimeError, 's5_provider_model_cooldown'):
                control.lib.run_drafts(facts, config, Path(root))
            with self.assertRaisesRegex(RuntimeError, 's5_provider_retry_requires_operator'):
                control.lib.run_drafts(facts, config, Path(root))
            self.assertEqual(provider.call_count, 1)
            sleeper.assert_not_called()

    def test_knowledge_budgets_are_explicit_and_do_not_expand_visibility(self):
        review={'project':'fixture','role':'oo_r_reader','principal':'reader','columns':{},'hidden_columns':[],'labels':{},'relationships':[]}
        effective,visibility,_=control.reviewed_documents(self.facts(),[],review,{'readme_tokens':30000,'table_file_tokens':20000})
        self.assertEqual(effective['ontology']['compile']['budgets']['table_file_tokens'],20000)
        self.assertEqual(visibility['datasets'],[])
        with self.assertRaises(ValueError):
            control.reviewed_documents(self.facts(),[],review,{'readme_tokens':0,'table_file_tokens':20000})

    def test_library_manifest_binds_shard_ownership_not_only_table_names(self):
        tables=self.facts()['tables']
        manifest={'protocol':'oo.library.manifest/v1','inventory':[{'schema':'app','table':'items'}],'exclusions':[],
                  'sources':[{'source_id':'shard_a','schema':'app','tables':['items']}]}
        self.assertEqual(control.select_tables(tables,manifest),tables)
        manifest['sources'][0]['source_id']='wrong_shard'
        with self.assertRaisesRegex(ValueError,'library_source_binding'):
            control.select_tables(tables,manifest)

    def test_s0_callers_forward_oracle_input_without_fabricating_mysql_ddl(self):
        facts = self.facts()
        tables = facts['tables']
        manifest = {'protocol': 'oo.library.manifest/v1',
                    'inventory': [{'schema': 'app', 'table': 'items'}], 'exclusions': [],
                    'sources': [{'source_id': tables[0]['source_id'], 'schema': 'app', 'tables': ['items']}]}
        oracle_input = {'source_id': 'caller-contract-only', 'catalog': {'path': '/private/pinned'}}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = {'psql_command': ['false'], 'oracle_observation_input': oracle_input}
            control.lib.write(root / 'manifest.json', manifest)
            control.lib.write(root / 'config.json', config)
            request = {'stage': 'S0', 'config': str(root / 'config.json'),
                       'manifest': str(root / 'manifest.json'), 'work_dir': str(root / 'new'),
                       'datasets': ['app.items'], 'max_tables': 1}
            with patch.object(control.lib, 'collect_tables', return_value=tables) as collect:
                control.run(request)
                self.assertIsNone(collect.call_args.args[1])
                self.assertEqual(collect.call_args.kwargs, {'oracle_input': oracle_input})
            control.lib.write(root / 'retained.json', tables)
            config.update(s0_input=str(root / 'retained.json'),
                          s0_sha256=hashlib.sha256((root / 'retained.json').read_bytes()).hexdigest())
            control.lib.write(root / 'config.json', config)
            request['work_dir'] = str(root / 'retained-output')
            with patch.object(control.lib, 'validate_retained_tables') as validate:
                control.run(request)
                self.assertIsNone(validate.call_args.args[2])
                self.assertEqual(validate.call_args.kwargs, {'oracle_input': oracle_input})

    def test_retained_drafts_refuse_corrupted_or_different_facts(self):
        import hashlib
        facts=self.facts()
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/'retained.json'
            path.write_text(json.dumps({'receipts':[{'facts_sha256':'wrong','model':'fixture/test','draft':{'datasets':[]}}]}))
            config={'model':'fixture/test','retained_drafts':{'path':str(path),'sha256':'0'*64}}
            with self.assertRaisesRegex(ValueError,'retained_draft_digest'):
                control.retained_drafts(facts,config,Path(root))
            config['retained_drafts']['sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
            with self.assertRaisesRegex(ValueError,'retained_draft_binding'):
                control.retained_drafts(facts,config,Path(root))

    def test_retained_oracle_observation_stages_recheck_source_and_target(self):
        facts = self.facts()
        oracle_input = {'source_id': 'caller-contract-only'}
        for stage in ('S1', 'S2', 'S3', 'S4'):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                control.lib.write(root / 's0-s2.json', facts['tables'])
                control.lib.write(root / 'facts.json', facts)
                control.lib.write(root / 'manifest.json', {})
                control.lib.write(root / 'config.json', {
                    'psql_command': ['false'], 'oracle_observation_input': oracle_input,
                    'facts_input': str(root / 'facts.json'),
                    'facts_sha256': hashlib.sha256((root / 'facts.json').read_bytes()).hexdigest()})
                request = {'stage': stage, 'work_dir': str(root),
                           'config': str(root / 'config.json'),
                           'manifest': str(root / 'manifest.json')}
                with patch.object(control.lib, 'validate_retained_tables',
                                  side_effect=ValueError('oracle_retained_source_drift')) as validate:
                    with self.assertRaisesRegex(ValueError, 'oracle_retained_source_drift'):
                        control.run(request)
                    self.assertEqual(validate.call_args.kwargs, {'oracle_input': oracle_input})
                self.assertFalse((root / (stage + '-receipt.json')).exists())
                with patch.object(control.lib, 'validate_retained_tables') as validate:
                    control.run(request)
                    validate.assert_called_once()

    def test_model_cannot_inject_approved_glossary_without_a_bound_review(self):
        facts=self.facts()
        receipt={'draft':{'datasets':[],'glossary':[{'term':'Anonymous record','definition':'A fixture item','dataset':'app.items'}]}}
        review={'project':'fixture','role':'oo_r_reader','principal':'reader','columns':{},'hidden_columns':[],'labels':{},'relationships':[],'datasets':{}}
        effective,_,_=control.reviewed_documents(facts,[receipt],review)
        self.assertEqual(effective['glossary'][0]['status'],'draft')
        review['datasets']['app.items']={'description':True,'reviewer':'fixture/test'}
        effective,_,_=control.reviewed_documents(facts,[receipt],review)
        self.assertEqual(effective['glossary'][0]['status'],'draft')
        review['datasets']['app.items']['glossary_terms']=[control.lib.digest(receipt['draft']['glossary'][0])]
        effective,_,_=control.reviewed_documents(facts,[receipt],review)
        self.assertEqual(effective['glossary'][0]['status'],'approved')
        self.assertEqual(effective['glossary'][0]['owner'],'fixture/test')
