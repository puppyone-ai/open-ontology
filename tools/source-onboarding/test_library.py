import copy
import json
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

import library as m


class LibraryTests(unittest.TestCase):
    def inventory(self, n=40):
        return [dict(schema='sample', table=f't_{i:03}', exact_rows=i + 1,
                     primary_key=[dict(name='id', type='bigint')]) for i in range(n)]

    def test_plan_complete_deterministic_multishard(self):
        tables = self.inventory()
        result = m.plan('sample-library', tables)
        self.assertEqual(result, m.plan('sample-library', tables[::-1]))
        self.assertEqual([len(s['tables']) for s in result['sources']], [16, 16, 8])
        self.assertEqual(len(result['inventory']), 40)
        self.assertEqual(result['scheduling'], dict(concurrency=2, stagger_seconds=10))
        m.validate(result)

    def test_layout_options_and_library_identity(self):
        ts = self.inventory()
        size = m.plan('a', ts, split='size', table_cap=10, concurrency=3)
        self.assertEqual([s['rows'] for s in size['sources']], [205] * 4)
        self.assertEqual(size['scheduling']['concurrency'], 3)
        self.assertNotEqual(size['sources'][0]['source_id'], m.plan('b', ts, split='size', table_cap=10)['sources'][0]['source_id'])
        for t in ts:
            t['domain'] = 'first' if t['table'] < 't_020' else 'second'
        explicit = m.plan('a', ts, split='explicit', table_cap=12)
        self.assertEqual([len(s['tables']) for s in explicit['sources']], [12, 8, 12, 8])
        ts[0].pop('domain')
        self.assertRaisesRegex(ValueError, 'explicit_group', m.plan, 'a', ts, split='explicit')

    def test_exclusions_and_inventory_accounting(self):
        ts = self.inventory(3)
        ts[0]['exclude_reason'] = 'policy review pending'
        ts[1]['exact_rows'] = 0
        ts[1]['primary_key'] = []
        p = m.plan('a', ts)
        status = m.status(p, [])
        self.assertEqual((status['expected'], status['excluded'], status['pending']), (3, 2, 1))
        self.assertFalse(status['complete'])
        self.assertEqual(len(p['exclusions']), 2)

    def test_manifest_tampering_fails_closed(self):
        p = m.plan('a', self.inventory())
        for mutate in [lambda x: x['inventory'].pop(),
                       lambda x: x['sources'][0]['tables'].pop(),
                       lambda x: x['sources'][1].update(source_id=x['sources'][0]['source_id']),
                       lambda x: x['scheduling'].update(concurrency=True),
                       lambda x: x.update(unexpected=True)]:
            bad = copy.deepcopy(p)
            mutate(bad)
            self.assertRaises(ValueError, m.validate, bad)

    def binding(self, plan):
        return dict(sources=[dict(key=s['key'], provision=[sys.executable, '-c', 'pass'],
                                 tables=[dict(table=t, argv=[sys.executable, '-c', 'pass'])
                                         for t in s['tables']]) for s in plan['sources']])

    def test_initial_import_and_resume_cumulative_status(self):
        p = m.plan('a', self.inventory(40), stagger_seconds=0)
        binding = self.binding(p)
        binding['sources'][1]['tables'][0]['argv'][-1] = 'raise SystemExit(1)'
        with tempfile.TemporaryDirectory() as d:
            first = m.import_library(p, binding, Path(d)/'first')
            self.assertEqual((first['coverage']['imported'], first['coverage']['failed']), (39, 1))
            binding['sources'][1]['tables'][0]['argv'][-1] = 'pass'
            resumed = m.import_library(p, binding, Path(d)/'retry', first)
            self.assertEqual(len(resumed['attempt']['results']), 1)
            self.assertEqual(resumed['coverage']['imported'], 40)
            self.assertTrue(resumed['coverage']['complete'])
            self.assertFalse(resumed['published'])
            self.assertEqual(m.status(p, [first, resumed]), resumed['coverage'])

    def test_binding_coverage_before_any_launch(self):
        p = m.plan('a', self.inventory(3))
        b = self.binding(p)
        b['sources'][0]['tables'].pop()
        with tempfile.TemporaryDirectory() as d:
            self.assertRaisesRegex(ValueError, 'binding_coverage', m.import_library, p, b, Path(d)/'run')
            self.assertFalse((Path(d)/'run').exists())

    def test_wrong_library_retry_and_unknown_results_rejected(self):
        p = m.plan('a', self.inventory(1), stagger_seconds=0)
        with tempfile.TemporaryDirectory() as d:
            previous = m.import_library(p, self.binding(p), Path(d)/'one')
            other = m.plan('b', self.inventory(1))
            self.assertRaises(ValueError, m.status, other, [previous])
            previous['results'].append(dict(source='unknown', table='t', reason='ok'))
            self.assertRaises(ValueError, m.status, p, [previous])

    def test_spawn_failure_preserves_every_unfinished_table_for_resume(self):
        p = m.plan('a', self.inventory(3), table_cap=1, stagger_seconds=0)
        b = self.binding(p)
        b['sources'][0]['provision'] = ['/nonexistent/anonymous-provision']
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)/'run'
            self.assertRaises(OSError, m.import_library, p, b, out)
            checkpoint = json.loads((out/'library-result.json').read_text())
            self.assertEqual(checkpoint['coverage']['failed'], 3)
            self.assertEqual(len(checkpoint['attempt']['failed']), 3)
            self.assertFalse(checkpoint['coverage']['complete'])
            b = self.binding(p)
            resumed = m.import_library(p, b, Path(d)/'retry', checkpoint)
            self.assertEqual(resumed['coverage']['imported'], 3)

    def test_layout_numeric_boundaries_and_no_cross_schema_collision(self):
        ts = self.inventory(1)
        ts.append(dict(ts[0], schema='other'))
        self.assertEqual(len(m.plan('a', ts)['sources']), 2)
        self.assertEqual(len(m.plan('a', self.inventory(40), table_cap=40)['sources']), 1)
        for option, values in [('table_cap', [0, 4097, True]),
                               ('concurrency', [0, 33, True]),
                               ('stagger_seconds', [-1, float('nan'), True])]:
            for value in values:
                self.assertRaises(ValueError, m.plan, 'a', ts, **{option: value})

    def test_publication_canonical_digest_preserves_utf8(self):
        import hashlib
        value={'library_id':'anonymous-库','schema':'样本'}
        expected=hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',', ':'),ensure_ascii=False).encode()).hexdigest()
        self.assertEqual(m.digest(value),expected)

    def test_adoption_membership_preserves_existing_sources_and_rejects_ambiguity(self):
        planned=m.plan('a', self.inventory(40))
        members=m.membership(planned)
        self.assertEqual([s['source_id'] for s in members['sources']],
                         [s['source_id'] for s in planned['sources']])
        self.assertEqual(m.membership(members),members)
        bad=copy.deepcopy(members)
        bad['sources'][1]['source_id']=bad['sources'][0]['source_id']
        self.assertRaises(ValueError,m.membership,bad)
        bad=copy.deepcopy(members)
        bad['sources'][1]['tables'].append(bad['sources'][0]['tables'][0])
        self.assertRaises(ValueError,m.membership,bad)

    def test_publish_and_adopt_reuse_precheck_and_only_accept_verified_release(self):
        import hashlib
        from types import SimpleNamespace
        manifest=m.membership(m.plan('a',self.inventory(1)))
        receipt=dict(library=dict(protocol='oo.pipeline.native-library-publication/v1',manifest=manifest),shards=[])
        mapping=dict(publication=dict(receipt=receipt,receipt_sha256=m.digest(receipt)))
        prepared=dict(precheck='rolled_back',native_mapping=mapping)
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            binary=root/'binary'
            binary.write_text('anonymous fixture')
            pin=dict(path=str(binary),sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            binding=dict(pipeline=pin,coordinator=pin,target={},release_config='private.json',
                         activation=dict(project_id='p',candidate_id='c',source_hash='a'*64,
                                         expected_version=None,schema_name='sample',relation_name='t_000',role_name='oo_r_all'))
            def completed(value):
                return SimpleNamespace(returncode=0,stdout=json.dumps(value))
            verified=dict(knowledge_published=True,native_publication=dict(receipt_sha256=m.digest(receipt)))
            with patch.object(m.subprocess,'run',side_effect=[completed(prepared),completed(verified)]) as run:
                result=m.publish_library(manifest,binding,root/'published')
                self.assertTrue(result['published'])
                self.assertEqual(run.call_args_list[1].args[0][1],'maintain-activate')
            with patch.object(m.subprocess,'run',side_effect=[completed(prepared),completed(dict(knowledge_published=True))]):
                self.assertFalse(m.publish_library(manifest,binding,root/'unproven')['published'])
            with patch.object(m.subprocess,'run',side_effect=[completed(prepared),completed(verified)]) as run:
                self.assertTrue(m.publish_library(manifest,binding,root/'recovered',recover=True)['published'])
                self.assertEqual(run.call_args_list[1].args[0][1],'maintain-recover')
            wrong=copy.deepcopy(binding);wrong['pipeline']['sha256']='0'*64
            self.assertRaisesRegex(ValueError,'binary_digest',m.publish_library,manifest,wrong,root/'bad')
            self.assertFalse((root/'bad').exists())

    def test_cli_plan_import_retry_and_status(self):
        entry = Path(m.__file__)
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            inventory = root/'inventory.json'
            inventory.write_text(json.dumps(dict(tables=self.inventory(40))))

            def cli(*args, code=0):
                result = subprocess.run([sys.executable, str(entry), *map(str, args)],
                                        text=True, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, code, result.stderr)
                return json.loads(result.stdout)

            planned = cli('plan', '--library-id', 'sample-library', '--input', inventory,
                          '--out', root/'layout', '--stagger-seconds', 0)
            self.assertEqual(planned['shards'], 3)
            manifest_path = root/'layout'/'library.json'
            p = json.loads(manifest_path.read_text())
            binding = self.binding(p)
            binding['sources'][1]['tables'][0]['argv'][-1] = 'raise SystemExit(1)'
            binding_path = root/'binding.json'
            binding_path.write_text(json.dumps(binding))
            imported = cli('import', '--manifest', manifest_path, '--binding', binding_path,
                           '--out', root/'first', code=1)
            self.assertEqual((imported['imported'], imported['failed']), (39, 1))
            binding['sources'][1]['tables'][0]['argv'][-1] = 'pass'
            binding_path.write_text(json.dumps(binding))
            resumed = cli('import', '--manifest', manifest_path, '--binding', binding_path,
                          '--out', root/'retry', '--retry-failed', root/'first'/'library-result.json')
            self.assertEqual(resumed['imported'], 40)
            checked = cli('status', '--manifest', manifest_path,
                          '--result', root/'retry'/'library-result.json')
            self.assertTrue(checked['complete'])
            self.assertEqual(checked['publication'], 'unverified')
            cli('status', '--manifest', manifest_path, code=1)

    def test_cost_guard_missing_metrics_pauses_initial_pull(self):
        p = m.plan('a', self.inventory(1), stagger_seconds=0)
        b = self.binding(p)
        b['cost_guard'] = dict(metrics_file='/nonexistent/anonymous-metrics',
                               max_egress_bytes=10, min_balance=1)
        with tempfile.TemporaryDirectory() as d:
            result = m.import_library(p, b, Path(d)/'run')
            self.assertEqual(result['attempt']['paused'], 'metrics_unavailable')
            self.assertEqual(result['coverage']['failed'], 1)


if __name__ == '__main__':
    unittest.main()
