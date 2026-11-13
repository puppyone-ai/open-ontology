"""No-network adapter tests using the actual community planner and compiler."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import community_adapter as adapter
import draft

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'tools/pg-builder/scripts'))
import selfbuild_budget as builder
from test_selfbuild_budget import Model


def oo_binary():
    # The L0 Rust suites test libraries only; build the CLI this test drives.
    path = ROOT/'target/debug/oo'
    if not path.is_file():
        subprocess.run(['cargo', 'build', '--locked', '-p', 'oo-cli', '--bin', 'oo'], cwd=ROOT,
                       env=dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'target')), check=True)
    return path


class Tests(unittest.TestCase):
    def test_embedded_modules_at_shallow_container_path(self):
        sources = {name: (ROOT/'tools/pg-builder/scripts'/(name+'.py')).read_text()
                   for name in ('model_provider', 'selfbuild_budget')}
        sources['community_adapter'] = Path(adapter.__file__).read_text()
        with patch.dict(sys.modules):
            loaded = draft.load_community_modules(sources, Path('/work/.oo'))
            self.assertTrue(callable(loaded.generate))
            self.assertEqual(sys.modules['selfbuild_budget'].ROOT, Path('/work/.oo'))

    def fixture(self):
        catalog = dict(tables=[dict(schema='app', name='regions',
            primary_key=[dict(name='id')],
            columns=[dict(name='id'), dict(name='name')], foreign_keys=[])])
        cards = dict(protocol='oo.pg-builder.cards/v1', tables=[dict(dataset='app.regions',
            columns=[dict(name='id', type_name='bigint', not_null=True),
                     dict(name='name', type_name='text', not_null=False)])])
        return cards, catalog

    def test_coverage_and_foreign_keys(self):
        cards, catalog = self.fixture()
        self.assertEqual(adapter.inventory(cards, catalog)[0]['primary_key'], ['id'])
        bad = copy.deepcopy(cards)
        bad['tables'][0]['columns'].pop()
        with self.assertRaisesRegex(ValueError, 'COMMUNITY_COLUMN_COVERAGE'):
            adapter.inventory(bad, catalog)
        catalog['tables'][0]['foreign_keys'] = [dict(columns=[
            dict(name='id', ref_schema='app', ref_table='parents', ref_name='key')])]
        self.assertEqual(adapter.inventory(cards, catalog)[0]['foreign_keys'],
            [dict(columns=['id'], referenced_dataset='app.parents', referenced_columns=['key'])])

    def test_generate_review_and_cached_resume(self):
        os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(ROOT/'target/debug/selfbuild-budget')
        with tempfile.TemporaryDirectory(dir=ROOT/'evidence') as tmp:
            root = Path(tmp)
            cards, catalog = self.fixture()
            draft.save(root/'cards.json', cards)
            draft.save(root/'catalog.json', catalog)
            model = Model()
            read = lambda p: json.loads(draft.read_private(p))
            stack = dict(model=dict(model='fixture'), sources=[dict(id='fixture')])
            with patch.object(builder, 'model_call', side_effect=lambda c, r: model(r)):
                adapter.generate(root, stack, ROOT/'target/debug', read, draft.save)
                count = len(model.requests)
                self.assertGreaterEqual(count, 2)
                adapter.generate(root, stack, ROOT/'target/debug', read, draft.save)
                self.assertEqual(len(model.requests), count)
            command = read(root/'review-input.json')
            self.assertEqual(command['source_draft_sha256'], builder.digest(read(root/'draft.json')))
            self.assertEqual(command['draft']['unresolved'], [])
            self.assertFalse(command['draft']['published'])
            decisions = {}
            for id_ in command['draft']['registry']['entities']:
                decisions['entity/'+id_] = dict(action='accept')
            for id_, item in command['draft']['accepted'].items():
                if item['kind'] != 'entity':
                    decisions['description/'+id_] = dict(action='accept')
            for table in command['effective']['datasets']:
                for col in table['columns']:
                    self.assertEqual(col['pii'], 'restricted')
                    decisions['classification/'+table['dataset']+'/'+col['name']] = dict(action='accept')
            draft.save(root/'decisions.json', dict(protocol='oo.owner-review/v1',
                input_sha256=builder.digest(command), reviewed_at='2026-11-04', items=decisions,
                g14=dict(action='accept', reason='Synthetic isolated fixture',
                         summary_sha256=command['draft']['review']['summary_sha256'])))
            result = subprocess.run([str(oo_binary()), 'review', '--input',
                str(root/'review-input.json'), '--review-file', str(root/'decisions.json'),
                '--out', str(root/'reviewed.json'), '--json'], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertEqual(read(root/'reviewed.json')['status'], 'reviewed')


if __name__ == '__main__':
    unittest.main()
