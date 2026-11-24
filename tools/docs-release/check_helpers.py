#!/usr/bin/env python3
"""write|check DIR COMMIT: HELPERS.json lists oo and every helper in helpers.txt with its sha256 and the commit
they were built from. `check` fails when a helper is missing, differs from the list, or the commit is another one."""
import hashlib, json, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMES = ['oo'] + [l.strip() for l in (HERE / 'helpers.txt').read_text().splitlines() if l.strip() and not l.startswith('#')]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(argv):
    if len(argv) != 4 or argv[1] not in ('write', 'check'):
        sys.exit('usage: check_helpers.py write|check DIR COMMIT')
    mode, out, commit = argv[1], Path(argv[2]), argv[3]
    manifest = out / 'HELPERS.json'
    if mode == 'write':
        files = {n: digest(out / n) for n in NAMES}
        manifest.write_text(json.dumps({'protocol': 'oo.helpers/v1', 'commit': commit, 'files': files}, indent=2) + '\n')
        return
    if not manifest.is_file():
        sys.exit('HELPERS.json is missing')
    data = json.loads(manifest.read_text())
    problems = []
    if data.get('commit') != commit:
        problems.append('built from commit %s, not %s' % (data.get('commit'), commit))
    for n in NAMES:
        f = out / n
        if not f.is_file():
            problems.append('helper missing: ' + n)
        elif data.get('files', {}).get(n) != digest(f):
            problems.append('helper from another build: ' + n)
    if problems:
        sys.exit('\n'.join(problems))
    print('helpers ok: ' + ', '.join(NAMES))


main(sys.argv)
