"""Offline acceptance with the real Rust bridge and a deterministic model seam."""
import copy
import json
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
from unittest.mock import patch
import io

import model_provider
import selfbuild_budget as lib


def fixture(count, columns=4):
    tables = [dict(dataset=f'app.item{i}', primary_key=['id'], columns=[
        dict(name='id' if c == 0 else f'field{c}', data_type='bigint', profile={'rows':10})
        for c in range(columns)]) for i in range(count)]
    edges = [dict(provenance='declared_fk', confidence=1,
                  **{'from':{'dataset':f'app.item{i}', 'columns':['id']},
                     'to':{'dataset':f'app.item{i-1}', 'columns':['id']}})
             for i in range(1,count)]
    return tables, edges


class Model:
    def __init__(self, omit_once=False):
        self.requests = []
        self.omit_once = omit_once

    def __call__(self, request):
        self.requests.append(request)
        items = json.loads(request['messages'][1]['content'])['items']
        values = []
        for item in items:
            value = dict(id=item['id'],kind=item['kind'])
            if item['kind'] in ('table','column'):
                value.update(label='Fixture',description='Synthetic fixture field or table.')
            elif item['kind'] == 'entity':
                name = item['input'].get('dataset', item['input'].get('main_table'))
                value.update(canonical_name=name,aliases=[],main_table=name,related_tables=[])
            else:
                value.update(decision='proposed',name='Fixture reference',cardinality='many_to_one')
            values.append(value)
        partial = self.omit_once and len(self.requests) == 1
        if partial:
            values = values[:-1]
        return dict(choices=[dict(finish_reason='length' if partial else 'stop',
                                 message={'content':'\n'.join(lib.canonical(v) for v in values)})])


class Acceptance(unittest.TestCase):
    def test_cli_dry_run_has_all_passes_without_provider_configuration(self):
        tables,edges = fixture(2,2)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lib.write_private(root/'input.json',dict(tables=tables,relationship_candidates=edges))
            lib.write_private(root/'config.json',self.config())
            result = subprocess.run([sys.executable,str(Path(lib.__file__)),
                '--input',str(root/'input.json'),'--config',str(root/'config.json'),
                '--output',str(root/'output')],capture_output=True,text=True,check=True)
            report = json.loads(result.stdout)
            self.assertEqual(set(report['passes']),{'in_community','cut_edges','merge'})
            self.assertGreater(report['batches'],0)
            self.assertEqual(report['cost']['status'],'unknown')
            self.assertEqual(report['model_calls'],0)
            self.assertFalse((root/'output'/'receipts').exists())

    def test_preflight_prices_rust_split_and_current_concurrency(self):
        batches = [dict(input_tokens=100,output_reserve=200)]*3
        config = dict(model='fixture',price_table={'fixture':dict(
            currency='USD',input_per_million=2,output_per_million=8)},
            seconds_per_batch=[10,30])
        estimate = lib.preflight_estimate(config,batches,2,2500)
        self.assertEqual(estimate['batches'],3)
        self.assertEqual(estimate['estimated_input_tokens'],7800)
        self.assertEqual(estimate['estimated_output_tokens'],600)
        self.assertAlmostEqual(estimate['cost']['range'][0],.0156)
        self.assertAlmostEqual(estimate['cost']['range'][1],.0204)
        self.assertEqual(estimate['duration']['range_seconds'],[20,60])
        unknown = lib.preflight_estimate(dict(config,model='unpriced'),batches,1)
        self.assertEqual(unknown['cost']['status'],'unknown')
        self.assertEqual(unknown['duration']['range_seconds'],[30,90])
        self.assertEqual(lib.preflight_estimate({'model':'fixture'},[],1)['duration']['status'],'unknown')
        for value in [-1,True,float('nan'),float('inf')]:
            bad = copy.deepcopy(config)
            bad['price_table']['fixture']['input_per_million'] = value
            with self.assertRaises(ValueError):
                lib.preflight_estimate(bad,batches,1)
        with self.assertRaises(ValueError):
            lib.preflight_estimate(dict(config,seconds_per_batch=[30,10]),batches,1)

    def test_preflight_is_written_and_printed_before_any_call(self):
        tables,edges = fixture(2,2)
        with tempfile.TemporaryDirectory() as output:
            model = Model()
            stdout = io.StringIO()
            def checked(request):
                report = json.loads((Path(output)/'preflight.json').read_text())
                self.assertGreater(report['passes']['in_community'],0)
                self.assertGreater(report['passes']['merge'],0)
                self.assertEqual(report['model_calls'],0)
                path = Path(output)/'preflight'/(lib.digest(request)+'.json')
                self.assertTrue(path.exists())
                self.assertIn('oo.selfbuild.preflight/v1',stdout.getvalue())
                self.assertIn(lib.digest(request),stdout.getvalue())
                return model(request)
            with patch('sys.stdout',stdout):
                result = lib.Runner(self.config(),output,checked).run(tables,edges)
                replay = lib.Runner(self.config(),output,checked).run(tables,edges,previous=result)
            self.assertFalse(result['unresolved'])
            self.assertEqual(replay['model_calls'],0)
            self.assertEqual(json.loads((Path(output)/'preflight.json').read_text())['batches'],0)

    def test_probe_limits_and_credential_free_egress(self):
        probe = dict(protocol='oo.model-capabilities/v1', context_window=272000,
                     max_output_tokens=128000, proxy_injected_prompt_tokens=2490)
        config = dict(model='fixture', capability_probe=probe,
                      transport='model-egress', endpoint='http://127.0.0.1:12345/v1')
        caps = lib.capability_input(config)
        self.assertEqual((caps['window'], caps['max_output'], caps['system_overhead']),
                         (272000,128000,2490))
        with self.assertRaises(ValueError):
            lib.capability_input(dict(config, capabilities={'window':1050000}))
        requests = []
        def response(request, timeout):
            requests.append(request)
            return io.BytesIO(b'{"choices":[]}')
        with patch.object(model_provider, 'credential', side_effect=AssertionError('no key read')), \
             patch.object(model_provider, 'open_request', side_effect=response):
            lib.model_call(config, {'stream':False})
            self.assertFalse(requests[0].has_header('Authorization'))
            with self.assertRaises(ValueError):
                lib.model_call(dict(config,endpoint='https://example.test/v1'),{})

    def config(self):
        return dict(model='offline-fixture',capabilities=dict(window=272000,max_output=8000,
                    system_overhead=2500,probed=True),orphan_threshold=0.2)

    def test_repair_sends_only_missing_and_receipts_are_model_bound(self):
        tables,_ = fixture(1,300)
        batches = lib.bridge('tables',tables=tables,budget={'capabilities':{'max_output':4000}})
        items = [item for batch in batches for item in batch['items']]
        self.assertEqual(len(items),301)
        with tempfile.TemporaryDirectory() as output:
            model = Model(omit_once=True)
            runner = lib.Runner(self.config(), output,model)
            values,missing = runner.generate(items,{})
            self.assertFalse(missing)
            self.assertEqual(len(values),301)
            first = {i['id'] for i in json.loads(model.requests[0]['messages'][1]['content'])['items']}
            repair = json.loads(model.requests[1]['messages'][1]['content'])['items']
            self.assertEqual(len(repair),1)
            self.assertIn(repair[0]['id'],first)
            self.assertIn('validation_feedback',model.requests[1]['messages'][1]['content'])
            cached_model = Model()
            lib.Runner(self.config(), output,cached_model).generate(items,{})
            self.assertFalse(cached_model.requests)
            different_model = Model()
            lib.Runner(dict(self.config(),model='another-model'),output,different_model).generate(items,{})
            self.assertTrue(different_model.requests)

    def test_three_pass_660_coverage_and_zero_call_data_cycle(self):
        tables,edges = fixture(660)
        with tempfile.TemporaryDirectory() as output:
            model = Model()
            result = lib.Runner(self.config(),output,model).run(tables,edges)
            self.assertFalse(result['unresolved'])
            self.assertEqual(len([v for v in result['accepted'].values() if v['kind']=='column']),2640)
            self.assertEqual(len([v for v in result['accepted'].values() if v['kind']=='table']),660)
            self.assertEqual(result['review']['summary']['mapped_fk_count'],659)
            self.assertEqual(result['review']['summary']['unmapped_without_reason'],[])
            self.assertEqual(result['review']['summary']['connected_components'],1)
            passes = {json.loads(r['messages'][1]['content'])['context']['pass_name'] for r in model.requests}
            self.assertEqual(passes,{'in_community','cut_edges','merge'})
            changed = copy.deepcopy(tables)
            changed[0]['columns'][0]['profile']['rows'] = 999
            replay = Model()
            cycle = lib.Runner(self.config(),output,replay).run(changed,edges,previous=result)
            self.assertEqual(cycle['model_calls'],0)
            self.assertFalse(replay.requests)
            self.assertEqual(cycle['accepted'],result['accepted'])
            changed[0]['columns'][0]['data_type']='text'
            schema = lib.Runner(self.config(),output,Model()).run(changed,edges,previous=result)
            self.assertEqual(len(schema['work']['communities']),1)
            self.assertEqual(len(schema['work']['merge_items']),1)

    def test_thousand_table_graph_budget_and_coverage(self):
        tables,edges = fixture(1000,2)
        graph = lib.bridge('graph',tables=tables,candidates=edges,
                           budget={'capabilities':self.config()['capabilities']})
        self.assertEqual(sum(len(v) for v in graph['families'].values()),1000)
        ids = []
        for community in graph['communities']:
            if len(community['tables']) > 1:
                self.assertEqual(len(community['batches']),1)
            for batch in community['batches']:
                self.assertLessEqual(batch['input_tokens'],batch['input_available'])
                self.assertLessEqual(batch['output_reserve'],8000)
                ids.extend(i['id'] for i in batch['items'])
        self.assertEqual(len(set(ids)),3000)

    def test_quota_stops_new_calls_and_keeps_receipts(self):
        tables,_ = fixture(4,60)
        batches = lib.bridge('tables',tables=tables)
        items=[item for batch in batches for item in batch['items']]
        model=Model()
        calls=[]
        def quota(request):
            calls.append(request)
            if len(calls)>1:
                raise model_provider.ProviderFailure('s5_provider_http_429',False,True)
            return model(request)
        with tempfile.TemporaryDirectory() as output:
            runner=lib.Runner(dict(self.config(),capabilities={'window':32000,'max_output':4000}),output,quota)
            with self.assertRaises(model_provider.ProviderFailure):
                runner.generate(items,{})
            self.assertEqual(len(calls),2)
            self.assertTrue(list((Path(output)/'receipts').glob('*.json')))

    def test_g14_fk_reasons_conflicts_and_review_binding(self):
        tables,edges=fixture(2)
        graph=lib.bridge('graph',tables=tables,candidates=edges)
        review=lib.bridge('review',graph=graph,registry={'entities':{}},mapped=[],rejections={},core=[],orphan_threshold=.2)
        self.assertEqual(review['reason'],'g14_unmapped_fk')
        self.assertEqual(review['summary']['fk_mapping_rate'],0)


class ReceiptAndFamilyTest(unittest.TestCase):
    def test_resume_preserves_attempts_and_retries_missing_finish_frame(self):
        tables,_=fixture(1,1)
        items=lib.bridge('tables',tables=tables)[0]['items']
        with tempfile.TemporaryDirectory() as output:
            model=Model()
            def incomplete(request):
                reply=model(request)
                reply['choices'][0]['finish_reason']=None
                return reply
            values,missing=lib.Runner(Acceptance().config(),output,incomplete).generate(items,{})
            self.assertTrue(missing)
            resumed=lib.Runner(Acceptance().config(),output,Model())
            values,missing=resumed.generate(items,{})
            self.assertFalse(missing)
            self.assertEqual(resumed.calls,1)
            self.assertEqual(len(resumed.receipts[0]['attempts']),2)
            self.assertEqual(resumed.successes,0)

    def test_reviewed_registry_compiles_pack_and_rejects_stale_review(self):
        tables,edges = fixture(2,2)
        root = lib.ROOT / 'testing/real-db/gateway-compiled-pack/fixtures'
        effective = json.loads((root/'effective.json').read_text())
        visibility = json.loads((root/'visibility-picker.json').read_text())
        effective['ontology']['include_schemas'] = ['app']
        effective['datasets'] = [
            dict(schema_version='oos/v1', dataset=t['dataset'], tier='file',
                 label=t['dataset'],primary_key=['id'],
                 columns=[dict(name=c['name'],pii='none',
                               classification={'state':'confirmed','by':'fixture.reviewer'})
                          for c in t['columns']]) for t in tables]
        visibility['datasets'] = [
            dict(dataset=t['dataset'],row_security='none',global_stats='denied',
                 columns=[c['name'] for c in t['columns']]) for t in tables]
        with tempfile.TemporaryDirectory() as output:
            draft = lib.Runner(Acceptance().config(),output,Model()).run(tables,edges)
            review = dict(reviewer='fixture.reviewer',reviewed_at='2026-11-04',
                          mode='self',review_origin='operator_pinned_file',
                          draft_sha256=lib.digest(draft),effective_sha256=lib.digest(effective),
                          summary_sha256=draft['review']['summary_sha256'],
                          entities={id_:dict(key='id',display='id') for id_ in draft['registry']['entities']},
                          decisions={})
            command = dict(protocol='oo.builder.reviewed-community/v1',
                           draft=draft,effective=effective,visibility=visibility,
                           review=review,orphan_threshold=.2,ceilings={},
                           classification_review=dict(review_origin='operator_pinned_file',
                               project=effective['project'],role=visibility['role'],
                               principal=visibility['principal'],hidden_columns=[],
                               columns={t['dataset']+'.'+c['name']:
                                   dict(pii='none',semantic_type=None,reviewer='fixture.reviewer')
                                   for t in tables for c in t['columns']}))
            compiled = lib.bridge('reviewed',command=command)
            self.assertFalse(compiled['published'])
            self.assertEqual(len(compiled['effective']['entities']),2)
            self.assertEqual(len(compiled['effective']['relationships']),1)
            artifact = lib.canonical(compiled['producer']['artifact'])
            self.assertIn('relationships/graph.json',artifact)
            hidden = copy.deepcopy(command)
            hidden['visibility']['datasets'] = hidden['visibility']['datasets'][:1]
            limited = lib.bridge('reviewed',command=hidden)
            files = {f['path']:f['text'] for f in limited['producer']['artifact']['files']}
            graph = json.loads(files['relationships/graph.json'])
            self.assertEqual(len(graph['entities']),1)
            self.assertEqual(graph['relationships'],[])
            self.assertNotIn('app.item1',files['relationships/graph.json'])
            missing = copy.deepcopy(command)
            missing['draft']['accepted'].pop(next(iter(
                id_ for id_,value in missing['draft']['accepted'].items()
                if value['kind']=='relation')))
            missing['review']['draft_sha256']=lib.digest(missing['draft'])
            with self.assertRaises(ValueError):
                lib.bridge('reviewed',command=missing)
            command['draft']['registry']['entities'][next(iter(review['entities']))]['canonical_name']='changed'
            with self.assertRaises(ValueError):
                lib.bridge('reviewed',command=command)
            command['review']['draft_sha256']=lib.digest(command['draft'])
            command['review']['reviewer']='model'
            with self.assertRaises(ValueError):
                lib.bridge('reviewed',command=command)

    def test_added_column_and_new_table_invalidate_only_their_communities(self):
        tables,edges=fixture(20,3)
        config=Acceptance().config()
        with tempfile.TemporaryDirectory() as output:
            initial=lib.Runner(config,output,Model()).run(tables,edges)
            tables[0]['columns'].append(dict(name='added',data_type='text'))
            added=lib.Runner(config,output,Model()).run(tables,edges,previous=initial)
            self.assertEqual(len(added['work']['communities']),1)
            self.assertEqual(len(added['work']['merge_items']),1)
            self.assertIn(lib.stable_id('column','app.item0','added'),added['accepted'])
            tables.append(dict(dataset='app.created',primary_key=['id'],
                               columns=[dict(name='id',data_type='bigint')]))
            created=lib.Runner(config,output,Model()).run(tables,edges,previous=added)
            self.assertEqual(len(created['work']['communities']),1)
            self.assertEqual(len(created['work']['merge_items']),1)
            self.assertIn(lib.stable_id('table','app.created'),created['accepted'])

    def test_usage_calibration_is_replayed_on_resume(self):
        tables,_ = fixture(8,30)
        items = [i for batch in lib.bridge('tables',tables=tables) for i in batch['items']]
        config = dict(model='usage-fixture',capabilities={'window':32000,'max_output':4000})
        model = Model()
        def with_usage(request):
            result = model(request)
            result['usage'] = {'prompt_tokens':6000,'completion_tokens':2500,'total_tokens':8500}
            return result
        with tempfile.TemporaryDirectory() as output:
            initial=lib.Runner(config,output,with_usage)
            initial.generate(items,{})
            def no_call(_):
                self.fail('completed receipts must not reopen the provider on resume')
            resumed=lib.Runner(config,output,no_call)
            resumed.generate(items,{})
            self.assertEqual(resumed.calls,0)
            self.assertEqual(initial.budget,resumed.budget)

    def test_folded_columns_inherit_without_truncation(self):
        tables,_=fixture(1,300)
        tables[0]['dataset']='app.events'
        tables.append(copy.deepcopy(tables[0]))
        tables[1]['dataset']='app.events_2023_01'
        with tempfile.TemporaryDirectory() as output:
            result=lib.Runner(Acceptance().config(),output,Model()).run(tables,[])
            self.assertEqual(len(result['family_inheritance']),301)
            self.assertEqual(len([v for v in result['accepted'].values() if v['kind']=='column']),600)

if __name__ == '__main__':
    unittest.main()
