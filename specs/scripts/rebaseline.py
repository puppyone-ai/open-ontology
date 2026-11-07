#!/usr/bin/env python3
"""Cut a new source-baseline revision from the bytes at a given commit.

Preserves the current manifest as sources-baseline-revision-<N>.json, writes
revision N+1 for docs/*.md and docs/product/*.md at HEAD, points the spec's
input-baseline row at it, and refreshes the v0.1 plan's Spec SHA-256 row.
The review record (specs/source-baseline-revision-<N+1>.md) is written by hand.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BASELINE = ROOT / 'specs/sources-baseline.json'
SPEC = ROOT / 'specs/open-ontology-v0.1-spec.md'
PLAN = ROOT / 'specs/open-ontology-v0.1-plan.md'
SOURCE_DIRS = ('docs', 'docs/product')


def git(*args: str) -> str:
    return subprocess.run(['git', '-C', str(ROOT), *args], check=True, capture_output=True, text=True).stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--date', required=True, help='YYYY-MM-DD, Asia/Shanghai')
    parser.add_argument('--status', required=True, help='review_status text')
    args = parser.parse_args()
    if git('status', '--porcelain', '--', *SOURCE_DIRS).strip():
        raise SystemExit('source docs have uncommitted changes; commit them first')
    head = git('rev-parse', 'HEAD').strip()
    old = json.loads(BASELINE.read_text())
    rev = old['revision']
    kept = BASELINE.with_name(f'sources-baseline-revision-{rev}.json')
    kept.write_bytes(BASELINE.read_bytes())
    sources = []
    for d in SOURCE_DIRS:
        for p in sorted((ROOT / d).glob('*.md')):
            b = p.read_bytes()
            sources.append({'path': str(p.relative_to(ROOT)), 'sha256': hashlib.sha256(b).hexdigest(),
                            'bytes': len(b), 'lines': len(b.decode().splitlines())})
    new = {
        'schema_version': old['schema_version'], 'spec_id': old['spec_id'], 'captured_date': args.date,
        'timezone': 'Asia/Shanghai', 'revision': rev + 1, 'supersedes_revision': rev,
        'supersedes_baseline_head': old['baseline_head'],
        'supersedes_sha256': hashlib.sha256(kept.read_bytes()).hexdigest(),
        'review_record': f'specs/source-baseline-revision-{rev + 1}.md', 'baseline_head': head,
        'source_count': len(sources), 'sources': sources, 'review_status': args.status,
    }
    BASELINE.write_text(json.dumps(new, ensure_ascii=False, indent=2) + '\n')

    spec = SPEC.read_text()
    row = re.search(r'^\| 输入基线 \|.*$', spec, re.M).group(0)
    spec = spec.replace(row, (
        f'| 输入基线 | `docs/` 与 `docs/product/` 共 {len(sources)} 份文档；来源 revision {rev + 1}，Git HEAD `{head}`；'
        f'[revision {rev + 1} 记录](source-baseline-revision-{rev + 1}.md)；保留 [revision {rev} 原清单](sources-baseline-revision-{rev}.json)、更早的 sources-baseline-revision-*.json 与'
        '[revision 4 历史记录](../execution/decisions/source-baseline-revision-4.md)保留。扩展设计目录 `docs/database-support/` '
        '不在哈希基线内；不宣称独立审阅或产品验收。 |'))
    SPEC.write_text(spec)
    digest = hashlib.sha256(SPEC.read_bytes()).hexdigest()
    plan = re.sub(r'^\| Spec SHA-256 \| `[0-9a-f]{64}` \|$', f'| Spec SHA-256 | `{digest}` |', PLAN.read_text(), flags=re.M)
    PLAN.write_text(plan)
    print(f'revision {rev + 1} at {head}; preserved {kept.name} sha256 {new["supersedes_sha256"]}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
