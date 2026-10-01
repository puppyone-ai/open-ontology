#!/usr/bin/env python3
"""Check OO-PLAN-001 coverage and source links; no product verification."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

sys.dont_write_bytecode = True
from check_spec import BASELINE, SPEC, inspect as inspect_spec

PLAN = SPEC.with_name('open-ontology-v0.1-plan.md')
TASK = r'SP\d{2}-W\d{2}'


def acceptance_ids(text: str) -> set[str]:
    """Expand the specification's compact references, e.g. AT-PL-01/04."""
    result = set()
    for domain, first, rest in re.findall(r'AT-([A-Z]+)-(\d{2})((?:/\d{2})*)', text):
        result.add(f'AT-{domain}-{first}')
        result.update(f'AT-{domain}-{number}' for number in re.findall(r'/(\d{2})', rest))
    return result


def inspect(text: str, spec: str) -> tuple[list[str], dict]:
    errors = []
    stats = {}
    expected_tasks = {}
    for line in spec.splitlines():
        match = re.match(rf'^\| ({TASK}) \|', line)
        if match:
            expected_tasks[match[1]] = acceptance_ids(line)

    headings = re.findall(rf'^#### ({TASK}) —', text, re.M)
    expected = set(expected_tasks)
    actual = set(headings)
    if len(expected) != 87:
        errors.append(f'Spec baseline must contain 87 work items, found {len(expected)}')
    if actual != expected:
        errors.append(f'Task coverage: missing={sorted(expected-actual)}, unknown={sorted(actual-expected)}')
    duplicates = [item for item, count in Counter(headings).items() if count != 1]
    if duplicates:
        errors.append(f'Duplicate task cards: {duplicates}')
    if set(re.findall(TASK, text)) - expected:
        errors.append('Undefined task reference')
    stats['work_items'] = len(actual)

    all_acceptance = set(re.findall(r'^\| (AT-[A-Z]+-\d{2}) \|', spec, re.M))
    unknown_at = acceptance_ids(text) - all_acceptance
    if unknown_at:
        errors.append(f'Undefined AT references: {sorted(unknown_at)}')
    missing_at = all_acceptance - acceptance_ids(text)
    if missing_at:
        errors.append(f'Acceptance execution coverage missing: {sorted(missing_at)}')
    stats['acceptance_references'] = len(acceptance_ids(text))
    for match in re.finditer(rf'^#### ({TASK}) —[^\n]*\n(.*?)(?=^###? |^#### |\Z)', text, re.M | re.S):
        task_id, body = match.groups()
        if task_id not in expected_tasks:
            continue
        missing = expected_tasks[task_id] - acceptance_ids(body)
        if missing:
            errors.append(f'{task_id} missing source acceptance: {sorted(missing)}')
        for field in ('执行：', '交付/PR：', '验收/停止：'):
            if field not in body:
                errors.append(f'{task_id} missing field {field}')

    expected_phases = {f'SP{i:02d}' for i in range(16)}
    phases = re.findall(r'^### (SP\d{2}) —', text, re.M)
    if set(phases) != expected_phases or len(phases) != 16:
        errors.append('Expected one phase heading for every SP00–SP15')
    if set(re.findall(r'SP\d{2}', text)) - expected_phases:
        errors.append('Undefined phase reference')
    if text.count('**Phase签收：**') != 16:
        errors.append('Expected 16 phase exit statements')
    stats['phases'] = len(set(phases))
    prototypes = re.findall(r'^\| (P\d+) \|', text, re.M)
    if set(prototypes) != {f'P{i}' for i in range(1, 17)} or len(prototypes) != 16:
        errors.append('Expected one execution row per P1–P16')
    stats['prototypes'] = len(set(prototypes))

    known_req = set(re.findall(r'REQ-[A-Z]+-\d{2}', spec))
    if set(re.findall(r'REQ-[A-Z]+-\d{2}', text)) - known_req:
        errors.append('Undefined REQ reference')
    chapters = [int(n) for n in re.findall(r'^## (\d+)\.', text, re.M)]
    if chapters != list(range(1, 8)):
        errors.append('Expected consecutive chapters 1–7')
    stats['chapters'] = len(chapters)
    digest = hashlib.sha256(spec.encode()).hexdigest()
    if not re.search(rf'^\| Spec SHA-256 \| `{digest}` \|$', text, re.M):
        errors.append('Spec hash baseline mismatch; review plan against changed spec')

    fence = None
    prose = []
    for number, line in enumerate(text.splitlines(), 1):
        if line.rstrip() != line:
            errors.append(f'Trailing whitespace at line {number}')
        match = re.match(r'^(`{3,}|~{3,})(.*)$', line)
        if fence is None:
            if match:
                fence = (match[1][0], len(match[1]), number)
            else:
                prose.append(line)
        elif match and match[1][0] == fence[0] and len(match[1]) >= fence[1] and not match[2].strip():
            fence = None
    if fence:
        errors.append(f'Unclosed Markdown fence at line {fence[2]}')
    if not text.endswith('\n'):
        errors.append('Missing final newline')
    count = 0
    for target in re.findall(r'\[[^\]]+\]\(([^)]+)\)', '\n'.join(prose)):
        parsed = urlsplit(target.strip().strip('<>'))
        if parsed.scheme or target.startswith('#'):
            continue
        if not (PLAN.parent / unquote(parsed.path)).resolve().exists():
            errors.append(f'Missing local link: {target}')
        count += 1
    stats['local_links'] = count
    for marker in ('## 2.', '### 2.2', '## 5.', '## 6.', '### 6.1', '### 6.2', '### 6.3', '### 7.1'):
        if marker not in text:
            errors.append(f'Missing execution guidance: {marker}')
    return errors, stats


def self_test(text: str, spec: str) -> list[str]:
    mutants = {
        'missing task': text.replace('#### SP00-W01 —', '#### REMOVED —', 1),
        'missing phase': text.replace('### SP02 —', '### REMOVED —', 1),
        'missing prototype': text.replace('| P16 |', '| REMOVED |', 1),
        'missing task acceptance': text.replace('AT-CORE-01；干净clone', 'REMOVED；干净clone', 1),
        'unknown acceptance': text + '\nAT-UNKNOWN-99\n',
        'missing cross-layer acceptance': text.replace('AT-SEC-02', 'REMOVED'),
        'bad link': text + '\n[bad](nonexistent-plan-file.md)\n',
        'unclosed fence': text + '\n```\n',
        'spec drift': text.replace(hashlib.sha256(spec.encode()).hexdigest(), '0' * 64, 1),
        'missing execution field': text.replace('- 交付/PR：工具链', '- REMOVED：工具链', 1),
    }
    failures = []
    for name, mutated in mutants.items():
        if mutated == text:
            failures.append(f'Mutation not applied: {name}')
        elif not inspect(mutated, spec)[0]:
            failures.append(f'Mutation not detected: {name}')
    if not failures:
        print(f'Self-test: {len(mutants)} plan mutations correctly rejected.')
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    try:
        spec = SPEC.read_text()
        text = PLAN.read_text()
        source_errors, source_stats = inspect_spec(spec, json.loads(BASELINE.read_text()))
        errors, stats = inspect(text, spec)
        errors = [f'Spec baseline: {e}' for e in source_errors] + errors
        stats['source_documents'] = source_stats['source_documents']
        if args.self_test and not errors:
            errors.extend(self_test(text, spec))
    except (OSError, ValueError, KeyError, IndexError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f'FAIL: {error}', file=sys.stderr)
        return 1
    print('PASS: plan coverage and document integrity only; product execution remains pending.')
    print(json.dumps(stats, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
