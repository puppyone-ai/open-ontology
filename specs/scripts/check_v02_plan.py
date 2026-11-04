#!/usr/bin/env python3
"""Check the v0.2 product-loop plan and docs/product structure; never claims product verification."""
from __future__ import annotations

import argparse
from collections import Counter
import os
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / 'specs/product-loops-v0.2-plan.md'
PRODUCT = ROOT / 'docs/product'
LANES = set('MABCDEFGH')
TODO = re.compile(r'^\| (S(\d)-([A-Z])-(\d{2})) \|(.*)$', re.M)
FORBIDDEN = [
    (re.compile(r'(?<![A-Za-z])sk-[A-Za-z0-9]{12,}'), 'API key'),
    (re.compile(r'\b(?:ooct|oost)_[A-Za-z0-9_-]{20,}'), 'gateway token'),
    (re.compile(r'/Users/[A-Za-z0-9._-]+/'), 'private absolute path'),
    (re.compile(r'\b(?:\d{1,3}\.){3}\d{1,3}\b'), 'IP address'),
    (re.compile(r'-----BEGIN [A-Z ]*PRIVATE KEY-----'), 'private key'),
]


def private_terms() -> list[tuple[re.Pattern, str]]:
    """Customer names and endpoints live outside the repository (one term per line)."""
    path = Path(os.environ.get('OO_PRIVATE_TERMS_FILE', Path.home() / '.config/open-ontology/private-terms.txt'))
    if not path.is_file():
        return []
    terms = [t.strip() for t in path.read_text().splitlines() if t.strip() and not t.startswith('#')]
    return [(re.compile(re.escape(t), re.I), 'private term') for t in terms]


def links(path: Path, text: str) -> list[str]:
    errors = []
    prose = re.sub(r'```.*?```', '', text, flags=re.S)
    for label, target in re.findall(r'\[([^\]]+)\]\(([^)]+)\)', prose):
        parsed = urlsplit(target.strip().strip('<>'))
        if parsed.scheme or not parsed.path:
            continue
        if not (path.parent / unquote(parsed.path)).resolve().exists():
            errors.append(f'{path.relative_to(ROOT)}: missing link [{label}]({target})')
    return errors


def inspect(plan: str, docs: dict[Path, str]) -> tuple[list[str], dict]:
    errors: list[str] = []
    stats: dict = {}
    chapters = [int(n) for n in re.findall(r'^## (\d+)\.', plan, re.M)]
    if chapters != list(range(0, 12)):
        errors.append(f'Plan chapters must be 0–11 consecutive, found {chapters}')
    rows = TODO.findall(plan)
    ids = [r[0] for r in rows]
    dup = sorted(k for k, n in Counter(ids).items() if n > 1)
    if dup:
        errors.append(f'Duplicate TODO IDs: {dup}')
    for todo_id, sprint, lane, _, rest in rows:
        cells = [c.strip() for c in rest.split('|')]
        if int(sprint) > 6:
            errors.append(f'{todo_id}: sprint out of range')
        if lane not in LANES:
            errors.append(f'{todo_id}: unknown lane {lane}')
        if len(cells) < 5 or cells[0] != lane or not all(cells[1:4]):
            errors.append(f'{todo_id}: row needs lane, task, deliverable and test/exit')
    stats['todo_items'] = len(set(ids))
    per_sprint = Counter(r[1] for r in rows)
    missing = [f'S{i}' for i in range(7) if not per_sprint.get(str(i))]
    if missing:
        errors.append(f'Sprints without TODO items: {missing}')
    referenced = set(re.findall(r'\bS\d-[A-Z]-\d{2}\b', plan))
    if referenced - set(ids):
        errors.append(f'Undefined TODO references: {sorted(referenced - set(ids))}')
    spec_path = ROOT / 'specs/open-ontology-v0.1-spec.md'
    if spec_path.exists():
        v02_ats = set(re.findall(r'^\| (AT-(?:ONB|TOK|BLD|ENG)-\d{2}) \|', spec_path.read_text(), re.M))
        missing_at = v02_ats - set(re.findall(r'AT-(?:ONB|TOK|BLD|ENG)-\d{2}', plan))
        if missing_at:
            errors.append(f'Spec 7B acceptance cases without a v0.2 TODO: {sorted(missing_at)}')
    decisions = set(re.findall(r'^\| (D\d+)a? \|', plan, re.M))
    if decisions != {f'D{i}' for i in range(1, 12)}:
        errors.append(f'Decisions must be D1–D11 (+D11a), found {sorted(decisions)}')
    for heading in ('### 10.1 ', '### 10.2 ', '### 10.3 ', '### 10.4 '):
        if heading not in plan:
            errors.append(f'Missing commit-standard section {heading.strip()}')
    for doc in sorted(p.name for p in docs if p.parent == PRODUCT and p.name != 'README.md'):
        if f'docs/product/{doc}' not in plan and doc not in docs.get(PRODUCT / 'README.md', ''):
            errors.append(f'docs/product/{doc} is not indexed')
    for path, text in docs.items():
        errors.extend(links(path, text))
        for pattern, label in FORBIDDEN + private_terms():
            if path.name == 'check_v02_plan.py':
                continue
            hit = pattern.search(text)
            if hit and not (label == 'IP address' and hit.group(0).count('.') == 3 and hit.group(0).startswith(('0.', '127.'))):
                errors.append(f'{path.relative_to(ROOT)}: forbidden {label}')
    stats['documents'] = len(docs)
    return errors, stats


def self_test(plan: str, docs: dict[Path, str]) -> list[str]:
    row = TODO.search(plan).group(0)
    tests = {
        'duplicate id': (plan.replace(row, row + '\n' + row, 1), docs),
        'empty exit': (plan.replace(row, re.sub(r'\|[^|]*\|$', '|  |', row), 1), docs),
        'unknown lane': (plan.replace(row, row.replace('-M-', '-Z-', 1).replace('-A-', '-Z-', 1).replace('-H-', '-Z-', 1), 1), docs),
        'missing decision': (re.sub(r'^\| D7 \|.*\n', '', plan, count=1, flags=re.M), docs),
        'leaked key': (plan + '\nkey sk-abcdef1234567890\n', docs),
    }
    plan_path = PLAN
    failures = []
    for name, (mutated, d) in tests.items():
        d2 = dict(d)
        d2[plan_path] = mutated
        if mutated == plan:
            failures.append(f'Mutation not applied: {name}')
        elif not inspect(mutated, d2)[0]:
            failures.append(f'Mutation not detected: {name}')
    if not failures:
        print(f'Self-test: {len(tests)} plan mutations correctly rejected.')
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    plan = PLAN.read_text()
    docs = {PLAN: plan}
    docs.update({p: p.read_text() for p in sorted(PRODUCT.glob('*.md'))})
    errors, stats = inspect(plan, docs)
    if args.self_test and not errors:
        errors.extend(self_test(plan, docs))
    if errors:
        for e in errors:
            print(f'FAIL: {e}', file=sys.stderr)
        return 1
    print('PASS: v0.2 plan structure and document hygiene only; product execution remains pending.')
    print(stats)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
