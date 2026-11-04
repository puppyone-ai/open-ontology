#!/usr/bin/env python3
"""Owner accept-all review file and the reviewed-registry unit-test fixture.

    python3 tools/release-coordinator/tests/retained_fixture.py OUT_DIR

Regenerates tests/fixtures/retained_reviewed.json from a replay model (no
network), the real community runner/bridge and the real `oo review` compiler.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'tools/pg-builder/scripts'))
import selfbuild_budget  # noqa: E402

BIN = ROOT / 'target/debug'


def accept_all(command, reviewed_at='2026-11-04'):
    """Every review item exactly once, accepted, plus the recomputed G14 summary."""
    draft = command['draft']
    items = {'entity/' + k: {'action': 'accept'} for k in draft['registry']['entities']}
    for key, value in draft['accepted'].items():
        kind = {'relation': 'relationship', 'table': 'description', 'column': 'description'}.get(value['kind'])
        if kind:
            items[kind + '/' + key] = {'action': 'accept'}
    for metric in command['effective'].get('metrics', []):
        items['metric/' + metric['metric']] = {'action': 'accept'}
    for dataset in command['effective']['datasets']:
        for column in dataset['columns']:
            items['classification/' + dataset['dataset'] + '/' + column['name']] = {'action': 'accept'}
    mapped = [k for k, v in draft['accepted'].items() if v['kind'] == 'relation' and v.get('decision') == 'proposed']
    rejections = {k: v['reason'] for k, v in draft['accepted'].items()
                  if v['kind'] == 'relation' and v.get('decision') == 'rejected'}
    report = selfbuild_budget.bridge('review', binary=BIN / 'selfbuild-budget', graph=draft['graph'],
                                     registry=draft['registry'], mapped=mapped, rejections=rejections,
                                     core=draft['review']['summary']['core_entities'],
                                     orphan_threshold=command['orphan_threshold'])
    return {'protocol': 'oo.owner-review/v1', 'input_sha256': selfbuild_budget.digest(command),
            'reviewed_at': reviewed_at, 'items': items,
            'g14': {'summary_sha256': selfbuild_budget.digest(report['summary']), 'action': 'accept',
                    'reason': 'Owner accepted the retained-generation draft (self-review)'}}


def owner_review(input_path, out_path):
    command = json.loads(Path(input_path).read_text())
    decisions = Path(out_path).with_name('decisions.json')
    selfbuild_budget.write_private(decisions, accept_all(command))
    result = subprocess.run([str(BIN / 'oo'), 'review', '--input', str(input_path), '--review-file',
                             str(decisions), '--out', str(out_path)], capture_output=True, timeout=120)
    Path(out_path).with_name('review.log').write_bytes(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError('owner_review_refused')
    return Path(out_path).read_bytes()


def main():
    import retained_build
    from test_selfbuild_budget import Acceptance, Model
    out = Path(sys.argv[1])
    out.mkdir(mode=0o700)
    os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(BIN / 'selfbuild-budget')
    tables = [dict(dataset='app.items', primary_key=['amount'], comment=None, foreign_keys=[],
                   columns=[dict(name='amount', data_type='bigint', nullable=False, comment=None)])]
    draft = selfbuild_budget.Runner(Acceptance().config(), out / 'model-replay', Model()).run(tables, [])
    selfbuild_budget.write_private(out / 'input.json', retained_build.review_input(tables, draft, 'owned'))
    raw = owner_review(out / 'input.json', out / 'reviewed.json')
    (ROOT / 'tools/release-coordinator/tests/fixtures/retained_reviewed.json').write_bytes(raw)


if __name__ == '__main__':
    main()
