#!/usr/bin/env python3
"""Validate OO-SPEC-001 documentation; never claims product verification."""
from __future__ import annotations

import argparse
from collections import Counter
import copy
import hashlib
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
SPEC = ROOT / 'specs/open-ontology-v0.1-spec.md'
BASELINE = ROOT / 'specs/sources-baseline.json'
ID = r'REQ-[A-Z]+-\d{2}'
AT = r'AT-[A-Z]+-\d{2}'


def section(text: str, start: str, end: str) -> str:
    if start not in text or end not in text:
        return ''
    return text.split(start, 1)[1].split(end, 1)[0]


def duplicate_ids(values: list[str]) -> list[str]:
    return sorted(k for k, n in Counter(values).items() if n > 1)


def inspect(text: str, baseline: dict) -> tuple[list[str], dict]:
    errors: list[str] = []
    stats: dict = {}
    body = text.split('\n## 22.', 1)[0]
    definitions = re.findall(rf'^({ID})：', body, re.M)
    definitions += re.findall(r'^\| (REQ-PROD-\d{2}) \|', body, re.M)
    trace = section(text, '### 22.2 ', '\n## 23.')
    traced = re.findall(rf'^\| ({ID}) \|', trace, re.M)
    reqs = set(definitions)
    stats['requirements'] = len(reqs)
    if len(reqs) != 80:
        errors.append(f'Expected 80 requirements, found {len(reqs)}')
    for label, values in [('requirement definitions', definitions), ('trace rows', traced)]:
        if duplicate_ids(values):
            errors.append(f'Duplicate {label}: {duplicate_ids(values)}')
    if reqs != set(traced):
        errors.append(f'Requirement trace mismatch: missing={sorted(reqs-set(traced))}; unknown={sorted(set(traced)-reqs)}')
    unknown = set(re.findall(ID, text)) - reqs
    if unknown:
        errors.append(f'Unknown requirement references: {sorted(unknown)}')

    acceptance_section = section(text, '### 18.2 ', '### 18.3 ')
    acceptance = re.findall(rf'^\| ({AT}) \|', acceptance_section, re.M)
    ats = set(acceptance)
    stats['acceptance_cases'] = len(ats)
    if len(ats) != 96 or duplicate_ids(acceptance):
        errors.append(f'AT definitions: expected 96 unique; count={len(ats)}, duplicates={duplicate_ids(acceptance)}')
    unknown = set(re.findall(AT, text)) - ats
    if unknown:
        errors.append(f'Undefined acceptance references: {sorted(unknown)}')
    for line in trace.splitlines():
        if re.match(rf'^\| {ID} \|', line):
            if not re.search(r'SP\d{2}', line) or not re.search(AT, line):
                errors.append(f'Trace row missing phase/AT: {line}')

    invariant_section = section(text, '### 22.1 ', '### 22.2 ')
    invariants = set(re.findall(r'^\| (I-[A-Z]+-\d+) \|', invariant_section, re.M))
    source_invariants: set[str] = set()
    for p in (ROOT / 'docs').glob('*.md'):
        source_invariants.update(re.findall(r'\*\*(I-[A-Z]+-\d+)\*\*', p.read_text()))
    stats['source_invariants'] = len(invariants)
    if invariants != source_invariants or len(invariants) != 26:
        errors.append(f'Invariant mismatch: missing={sorted(source_invariants-invariants)}, extra={sorted(invariants-source_invariants)}')

    expected_phases = {f'SP{i:02d}' for i in range(16)}
    phase_headers = re.findall(r'^### 17\.\d+ (SP\d{2}) ', text, re.M)
    phase_table = section(text, '### 17.1 ', '### 17.2 ')
    phase_rows = re.findall(r'^\| (SP\d{2}) \|', phase_table, re.M)
    if set(phase_headers) != expected_phases or set(phase_rows) != expected_phases:
        errors.append('Phase headers/table must cover SP00–SP15')
    if duplicate_ids(phase_headers) or duplicate_ids(phase_rows):
        errors.append('Duplicate phase heading/table row')
    stats['phases'] = len(set(phase_headers))
    graph: dict[str, set[str]] = {}
    for line in phase_table.splitlines():
        if re.match(r'^\| SP\d{2} \|', line):
            cells = line.split('|')
            graph[cells[1].strip()] = set(re.findall(r'SP\d{2}', cells[3]))
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(node: str) -> None:
        if node in visiting:
            errors.append(f'Phase dependency cycle at {node}')
            return
        if node in visited:
            return
        visiting.add(node)
        for dependency in sorted(graph.get(node, set())):
            if dependency not in expected_phases:
                errors.append(f'Unknown phase dependency: {dependency}')
            else:
                visit(dependency)
        visiting.remove(node)
        visited.add(node)

    for node in sorted(graph):
        visit(node)
    work = re.findall(r'^\| (SP\d{2}-W\d{2}) \|', text, re.M)
    if len(work) != 87 or duplicate_ids(work):
        errors.append(f'Expected 87 unique work items; found {len(work)}')
    if set(re.findall(r'SP\d{2}-W\d{2}', text)) - set(work):
        errors.append('Undefined work-item reference')
    if set(re.findall(r'SP\d{2}', text)) - expected_phases:
        errors.append('Unknown phase reference')
    stats['work_items'] = len(work)

    requirements = [
        ('M0 prototypes', section(text, '### 17.3 ', '### 17.4 '), {f'P{i}' for i in range(1, 17)}, r'^\| (P\d+) \|'),
        ('OOS gates', section(text, '### 10.6 ', '\n## 11.'), {'G1','G2','G3','G4','G5a','G5b','G6','G7','G8','G9','G10','G11','G12','G13'}, r'^\| (G\d+[ab]?) \|'),
        ('Release states', section(text, '### 13.4 ', '\n## 14.'), {f'S{i}' for i in range(1, 12)}, r'^\| (S\d+) \|'),
        ('Source risks', section(text, '### 23.1 ', '### 23.2 '), {f'R{i}' for i in range(1, 17)}, r'^\| (R\d+)'),
    ]
    for name, part, expected, pattern in requirements:
        actual = set(re.findall(pattern, part, re.M))
        if actual != expected:
            errors.append(f'{name} mismatch: missing={sorted(expected-actual)}, extra={sorted(actual-expected)}')
    chapters = [int(x) for x in re.findall(r'^## (\d+)\.', text, re.M)]
    if chapters != list(range(1, 25)):
        errors.append('Chapter numbers must be consecutive 1–24')
    stats['chapters'] = len(chapters)

    # Strip fenced content for link checks and parse strict JSON examples.
    fence = None
    code: list[str] = []
    prose: list[str] = []
    json_count = 0
    for number, line in enumerate(text.splitlines(), 1):
        match = re.match(r'^(`{3,}|~{3,})(.*)$', line)
        if fence is None:
            if match:
                fence = (match.group(1)[0], len(match.group(1)), match.group(2).strip(), number)
                code = []
            else:
                prose.append(line)
        elif match and match.group(1)[0] == fence[0] and len(match.group(1)) >= fence[1] and not match.group(2).strip():
            if fence[2] == 'json':
                try:
                    json.loads('\n'.join(code))
                    json_count += 1
                except (ValueError, TypeError) as exc:
                    errors.append(f'Invalid JSON block at line {fence[3]}: {exc}')
            fence = None
        else:
            code.append(line)
    if fence:
        errors.append(f'Unclosed code fence at line {fence[3]}')
    stats['strict_json_examples'] = json_count
    if json_count != 1:
        errors.append(f'Expected one strict JSON example, found {json_count}')
    local_links = 0
    for label, target in re.findall(r'\[([^\]]+)\]\(([^)]+)\)', '\n'.join(prose)):
        target = target.strip().strip('<>')
        parsed = urlsplit(target)
        if parsed.scheme or target.startswith('#'):
            continue
        link = (SPEC.parent / unquote(parsed.path)).resolve()
        if not link.exists():
            errors.append(f'Missing local link [{label}]: {target}')
        local_links += 1
    stats['local_links'] = local_links

    entries = baseline.get('sources', [])
    source_paths = {str(p.relative_to(ROOT)) for p in (ROOT/'docs').glob('*.md')}
    listed = [e.get('path', '') for e in entries]
    if len(entries) != 19 or baseline.get('source_count') != 19 or set(listed) != source_paths or len(set(listed)) != len(listed):
        errors.append('Source baseline must contain all 19 docs exactly once')
    for entry in entries:
        p = ROOT / entry['path']
        if not p.is_file():
            errors.append(f'Missing baseline source: {entry["path"]}')
            continue
        data = p.read_bytes()
        if hashlib.sha256(data).hexdigest() != entry.get('sha256'):
            errors.append(f'Source hash drift: {entry["path"]}')
        if len(data) != entry.get('bytes') or len(data.decode().splitlines()) != entry.get('lines'):
            errors.append(f'Source size/line drift: {entry["path"]}')
    if baseline.get('baseline_head') not in text:
        errors.append('Spec/source baseline HEAD mismatch')
    for path in listed:
        if f'../{path}' not in text:
            errors.append(f'Source not referenced in spec: {path}')
    stats['source_documents'] = len(entries)
    return errors, stats


def self_test(text: str, baseline: dict) -> list[str]:
    tests = {}
    trace_row = next(line for line in section(text, '### 22.2 ', '\n## 23.').splitlines() if line.startswith('| REQ-CORE-01 |'))
    tests['missing requirement trace'] = (text.replace(trace_row+'\n', '', 1), baseline)
    tests['undefined acceptance'] = (text+'\nAT-BOGUS-99\n', baseline)
    tests['bad link'] = (text.replace('[README](../docs/README.md)', '[README](../docs/nonexistent.md)', 1), baseline)
    tests['bad JSON'] = (text.replace('"spec_id": "OO-SPEC-001",', '"spec_id": ,', 1), baseline)
    tests['unclosed fence'] = (text+'\n```\n', baseline)
    phase = next(line for line in text.splitlines() if line.startswith('| SP00 |'))
    tests['phase cycle'] = (text.replace(phase, phase.replace('| 无 |', '| SP01 |'), 1), baseline)
    inv = next(line for line in text.splitlines() if line.startswith('| I-SRC-1 |'))
    tests['missing invariant'] = (text.replace(inv+'\n', '', 1), baseline)
    drift = copy.deepcopy(baseline)
    drift['sources'][0]['sha256'] = '0'*64
    tests['source drift'] = (text, drift)
    failures = []
    for name, (mutated, source) in tests.items():
        if mutated == text and source == baseline:
            failures.append(f'Mutation not applied: {name}')
        elif not inspect(mutated, source)[0]:
            failures.append(f'Mutation not detected: {name}')
    if not failures:
        print(f'Self-test: {len(tests)} document mutations correctly rejected.')
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true', help='Check in-memory document mutations without modifying files')
    args = parser.parse_args()
    try:
        text = SPEC.read_text()
        baseline = json.loads(BASELINE.read_text())
        errors, stats = inspect(text, baseline)
        if args.self_test and not errors:
            errors.extend(self_test(text, baseline))
    except (OSError, ValueError, KeyError, IndexError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f'FAIL: {error}', file=sys.stderr)
        return 1
    print('PASS: document integrity only; product validation remains pending.')
    print(json.dumps(stats, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
