#!/usr/bin/env python3
"""Derive support levels from evidence; the only way a cell becomes verified (S5-M-01).

Input: execution/v0.2/evidence-index.json, rows
  {"target", "version", "suite", "variant", "result": "pass|fail|skip", "run_id", "sha256", "emulated",
   "observed_at"}
where `variant` names the capability path a suite exercised (e.g. "c1", "c5", "snapshot") and the
optional `observed_at` (ISO 8601 UTC) orders runs; run ids are opaque and only break ties.

Output: docs/database-support/support-matrix.generated.json (valid YAML), one entry per target/version
with a level per capability, and the suites and run ids behind it. The hand-written
support-matrix.yaml must never contain `verified` or `certified`; --check enforces both rules.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
INDEX = ROOT / 'execution/v0.2/evidence-index.json'
OUT = ROOT / 'docs/database-support/support-matrix.generated.json'
HAND = ROOT / 'docs/database-support/support-matrix.yaml'

# Capability -> suites that must all pass (with the variant, when given) for `verified`.
# Every requirement names its variant: a row of another variant never satisfies it.
REQUIRED = {
    'full': [('T0', 'snapshot'), ('T1', 'snapshot'), ('T7', 'snapshot'), ('T8', 'snapshot')],
    'cdc': [('T2', 'c1'), ('T3', 'c1'), ('T7', 'c1')],
    'digest': [('T2', 'c5'), ('T3', 'c5'), ('T7', 'c5')],
    'schema_change': [('T4', 'c5')],
    'selfbuild': [('T5', 'snapshot')],
    'dialogue': [('T6', 'snapshot')],
}
BASELINE = ROOT / 'execution/v0.2/matrix-baseline.json'


def canonical(version: str) -> tuple[int, ...] | None:
    """Numeric components without trailing zeros: 21.3.0 and 21.3.0.0.0 are one version."""
    # Marketing suffixes name the same release: 19c, 23ai, 11g.
    version = re.sub(r'(?<=\d)(c|g|ai|i)$', '', version)
    parts = version.split('.')
    if not all(p.isdigit() for p in parts):
        return None
    nums = [int(p) for p in parts]
    while len(nums) > 1 and nums[-1] == 0:
        nums.pop()
    return tuple(nums)


def unify_versions(rows: list[dict]) -> list[dict]:
    """Fold image identifiers into the measured version they prefix (23.5 -> 23.5.0.24.07)."""
    by_target = defaultdict(set)
    for r in rows:
        by_target[r['target']].add(r['version'])
    alias = {}
    for target, versions in by_target.items():
        keyed = {v: canonical(v) for v in versions}
        for v, k in keyed.items():
            if k is None:
                continue
            longer = [w for w, kw in keyed.items() if kw and kw != k and kw[:len(k)] == k]
            same = sorted((w for w, kw in keyed.items() if kw == k), key=len, reverse=True)
            if len(longer) == 1:
                alias[(target, v)] = longer[0]
            elif same and same[0] != v:
                alias[(target, v)] = same[0]
    return [dict(r, version=alias.get((r['target'], r['version']), r['version'])) for r in rows]


def baseline_filter(rows: list[dict], base: dict | None = None) -> list[dict]:
    """With a baseline, rows of its suites count only when measured on that exact tree."""
    if base is None:
        if not BASELINE.exists():
            return rows
        base = json.loads(BASELINE.read_text())
    covered = set(base['suites'])
    # Per-target pins are allowed only where the product diff between pins is confined to that
    # target's connector (recorded with the baseline); every other target uses product_sha.
    pins = base.get('target_overrides', {})
    return [r for r in rows if r['suite'] not in covered
            or r.get('product_sha') == pins.get(r['target'], base['product_sha'])]
RESULTS = {'pass', 'fail', 'skip'}
SHA = re.compile(r'^[0-9a-f]{64}$')


def validate(rows: list[dict]) -> list[str]:
    errors = []
    for i, r in enumerate(rows):
        for k in ('target', 'version', 'suite', 'result', 'run_id', 'sha256'):
            if not r.get(k):
                errors.append(f'row {i}: missing {k}')
        if r.get('result') not in RESULTS:
            errors.append(f'row {i}: result must be pass|fail|skip')
        if r.get('sha256') and not SHA.match(r['sha256']):
            errors.append(f'row {i}: sha256 is not 64 hex')
    return errors


def derive(rows: list[dict]) -> list[dict]:
    rows = unify_versions(rows)
    by = defaultdict(list)
    for r in rows:
        by[(r['target'], r['version'])].append(r)
    out = []
    for (target, version), rs in sorted(by.items()):
        caps = {}
        for cap, needs in REQUIRED.items():
            hits, failed, missing = [], False, False
            for suite, variant in needs:
                match = [r for r in rs if r['suite'] == suite and r.get('variant') == variant]
                if not match:
                    missing = True
                    continue
                latest = max(match, key=lambda r: (r.get('observed_at') or '', r['run_id']))
                hits.append(latest)
                failed |= latest['result'] != 'pass'
                # T5 over a gold-table subset is not full-fixture Self-build.
                if cap == 'selfbuild' and 'not full-fixture T5' in latest.get('scope', ''):
                    missing = True
            if not hits:
                continue
            level = 'verified' if not failed and not missing else ('experimental' if any(h['result'] == 'pass' for h in hits) else 'designed')
            caps[cap] = {
                'level': level,
                'emulated': any(h.get('emulated') for h in hits),
                'evidence': [{'suite': h['suite'], 'variant': h.get('variant'), 'result': h['result'],
                              'run_id': h['run_id'], 'sha256': h['sha256']} for h in hits],
            }
        out.append({'target': target, 'version': version, 'caps': caps})
    return out


def render(rows: list[dict]) -> str:
    doc = {'schema': 'oo-support-matrix-generated/v1',
           'note': 'Generated by specs/scripts/gen_support_matrix.py from execution/v0.2/evidence-index.json. Do not edit.',
           'targets': derive(rows)}
    return json.dumps(doc, ensure_ascii=False, indent=1) + '\n'


def hand_errors() -> list[str]:
    text = HAND.read_text()
    bad = [n for n, line in enumerate(text.splitlines(), 1)
           if not line.lstrip().startswith('#') and re.search(r':\s*(verified|certified)\b', line)]
    return [f'support-matrix.yaml line {n}: verified/certified may only come from the generated matrix' for n in bad]


def self_test() -> list[str]:
    sha = '0' * 64
    base = [{'target': 'mysql8', 'version': '8.4', 'suite': s, 'variant': v, 'result': 'pass', 'run_id': 'r1', 'sha256': sha}
            for s, v in REQUIRED['full'] + REQUIRED['digest']]
    failures = []
    caps = derive(base)[0]['caps']
    if caps['full']['level'] != 'verified' or caps['digest']['level'] != 'verified':
        failures.append('all-pass rows did not verify')
    if 'cdc' in caps:
        failures.append('capability without evidence was emitted')
    one_fail = base[:-1] + [dict(base[-1], result='fail')]
    if derive(one_fail)[0]['caps']['digest']['level'] == 'verified':
        failures.append('a failed suite still verified')
    newer = base + [dict(base[0], run_id='r2', result='fail')]
    if derive(newer)[0]['caps']['full']['level'] == 'verified':
        failures.append('latest failing run did not override an older pass')
    timed = [dict(r, observed_at='2026-11-05T00:00:00Z') for r in base] + [
        dict(base[0], run_id='z-older', result='fail', observed_at='2026-11-01T00:00:00Z')]
    if derive(timed)[0]['caps']['full']['level'] != 'verified':
        failures.append('an older run with a later-sorting id overrode a newer pass')
    partial = [r for r in base if r['suite'] != 'T7']
    if derive(partial)[0]['caps']['full']['level'] != 'experimental':
        failures.append('missing suite did not downgrade to experimental')
    other_variant = [dict(r, variant='c5') if r['suite'] == 'T1' else r for r in base]
    if derive(other_variant)[0]['caps']['full']['level'] == 'verified':
        failures.append('a row of another variant satisfied a requirement')
    split = [dict(r, version='8.4.0') if r['suite'] == 'T7' else r for r in base]
    if len(derive(split)) != 1:
        failures.append('equivalent version strings were not unified')
    pinned = {'product_sha': 'a' * 40, 'suites': ['T1']}
    mixed = [dict(r, product_sha='a' * 40) for r in base] + [
        dict(base[0], suite='T1', run_id='r9', result='fail', product_sha='b' * 40)]
    if derive(baseline_filter(mixed, pinned))[0]['caps']['full']['level'] != 'verified':
        failures.append('a row from another product tree was counted under a baseline')
    if derive(baseline_filter([dict(r, product_sha='b' * 40) if r['suite'] == 'T1' else r for r in base], pinned))[0]['caps']['full']['level'] == 'verified':
        failures.append('a baseline suite verified without a row from the pinned tree')
    split = dict(pinned, target_overrides={'mysql8': 'c' * 40})
    if derive(baseline_filter(mixed, split))[0]['caps']['full']['level'] == 'verified':
        failures.append('a target override still admitted rows from the default tree')
    if not validate([{'target': 'x'}]):
        failures.append('invalid row accepted')
    if not failures:
        print('Self-test: 12 derivation cases correct.')
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    rows = json.loads(INDEX.read_text()) if INDEX.exists() else []
    errors = validate(rows) + hand_errors()
    rows = baseline_filter(rows)
    if args.self_test:
        errors += self_test()
    text = render(rows)
    if args.check:
        if rows and (not OUT.exists() or OUT.read_text() != text):
            errors.append('support-matrix.generated.json is stale; run gen_support_matrix.py')
    elif not errors and rows:
        OUT.write_text(text)
    for e in errors:
        print(f'FAIL: {e}', file=sys.stderr)
    if errors:
        return 1
    print(f'PASS: {len(rows)} evidence rows; generated levels consistent; product acceptance is only what the evidence says.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
