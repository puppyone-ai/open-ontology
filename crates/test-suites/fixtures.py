#!/usr/bin/env python3
"""Discover opt-in Rust fixtures; run exactly one explicitly selected fixture.

  python3 crates/test-suites/fixtures.py list
  python3 crates/test-suites/fixtures.py run oo-connectors live_source_catalog

No databases, containers, credentials or evidence are provisioned implicitly.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TEST = re.compile(
    r'#\[(?:tokio::)?test[^\n]*\]\s*'
    r'(?P<attrs>(?:#\[[\s\S]*?\]\s*)*)(?:async )?fn (?P<name>\w+)'
)
EVIDENCE_MODULES = {
    'c1_buffer_position.rs', 'c1_oracle_source_resume.rs', 'oracle_capacity_admission.rs',
    'native_binding.rs', 'native_arrow.rs',
}


def inventory():
    rows = []
    for package in sorted((ROOT / 'crates').glob('oo-*')):
        for path in sorted(package.rglob('*.rs')):
            if 'vendor' in path.parts:
                continue
            source = path.read_text()
            for match in TEST.finditer(source):
                attrs = match['attrs']
                if '#[ignore' not in attrs and 'ignore =' not in attrs:
                    continue
                reason = re.search(r'ignore\s*=\s*"([^"]+)"', attrs)
                reason = reason[1] if reason else 'explicit owned fixture (see source)'
                kind = 'live'
                if ('retained evidence:' in reason or path.name in EVIDENCE_MODULES
                        or 'offline' in reason or 'no source/target sessions' in reason
                        or 'no source or target connection' in reason
                        or 'preflight without target writes' in reason):
                    kind = 'evidence'
                if 'Linux' in reason:
                    kind = 'platform'
                if 'OO_BSON_EMIT_DIR' in reason:
                    kind = 'artifact'
                rows.append(dict(package=package.name, name=match['name'], suite=kind,
                                 source=str(path.relative_to(ROOT)), reason=reason))
    return rows


def select(rows, package, name):
    matches = [r for r in rows if r['package'] == package and r['name'] == name.split('::')[-1]]
    if len(matches) != 1:
        raise ValueError(f'expected one classified fixture, found {len(matches)}: {package} {name}')
    return matches[0]


def run(args):
    row = select(inventory(), args.package, args.name)
    print(json.dumps(row), flush=True)
    command = ['cargo', 'test', '--locked', '-p', args.package, '--no-run', '--message-format=json']
    if args.features:
        command += ['--features', args.features]
    build = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
    if build.returncode:
        return build.returncode
    candidates = []
    for line in build.stdout.splitlines():
        artifact = json.loads(line)
        if artifact.get('reason') != 'compiler-artifact' or not artifact.get('executable'):
            continue
        if not artifact.get('profile', {}).get('test'):
            continue
        executable = artifact['executable']
        listing = subprocess.run([executable, '--ignored', '--list'], cwd=ROOT,
                                 check=True, capture_output=True, text=True).stdout
        for test in listing.splitlines():
            if not test.endswith(': test'):
                continue
            name = test[:-6]
            if name == args.name or ('::' not in args.name and name.endswith('::' + args.name)):
                candidates.append((executable, name))
    if len(candidates) != 1:
        raise ValueError(f'expected one compiled ignored test, found {len(candidates)}; '
                         'check platform/features or use the fully qualified name')
    executable, name = candidates[0]
    print(f'Running {name} (missing fixtures are failures)', flush=True)
    # Cargo normally supplies the package directory as the test working directory.
    return subprocess.run([executable, name, '--ignored', '--exact', '--nocapture',
                           '--test-threads=1'], cwd=ROOT / 'crates' / args.package).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    listing = sub.add_parser('list')
    listing.add_argument('--suite', choices=['live', 'evidence', 'platform', 'artifact'])
    running = sub.add_parser('run')
    running.add_argument('package')
    running.add_argument('name')
    running.add_argument('--features')
    args = parser.parse_args()
    if args.action == 'list':
        for row in inventory():
            if not args.suite or row['suite'] == args.suite:
                print(json.dumps(row))
        return 0
    return run(args)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
