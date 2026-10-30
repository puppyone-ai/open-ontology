import argparse
import hashlib
import json
import re
import statistics
import struct
import subprocess
from pathlib import Path


def source_digest(container):
    if not container.startswith('oo-wf-chunk-'):
        raise ValueError('only an owned chunk fixture is permitted')
    process = subprocess.Popen([
        'docker', 'exec', container, 'mysql', '-uroot', '--quick', '--batch',
        '--raw', '--skip-column-names', '-e',
        'SELECT id,payload FROM fixture.large_items ORDER BY id',
    ], stdout=subprocess.PIPE)
    digest = hashlib.sha256()
    rows = 0
    for line in process.stdout:
        cells = line.removesuffix(b'\n').split(b'\t')
        assert len(cells) == 2
        for cell in cells:
            digest.update(b'\1')
            digest.update(struct.pack('<Q', len(cell)))
            digest.update(cell)
        rows += 1
    assert process.wait() == 0
    return dict(rows=rows, ordered_sha256=digest.hexdigest())


def captured(root, name, digest_binary):
    out = root / name
    capturer = json.loads((out / 'capturer.json').read_text())
    entries = json.loads((out / 'chunks.json').read_text())
    assert sum(entry['rows'] for entry in entries) == 10_000_000
    previous = None
    elapsed = []
    for index, entry in enumerate(entries):
        assert entry['chunk_id'] == index and entry['lo'] == previous
        assert entry['rows'] <= 50_000 and entry['bytes'] <= 8_388_608
        assert hashlib.sha256((out / f'{index}.arrow').read_bytes()).hexdigest() == entry['arrow_sha256']
        previous = entry['hi']
        elapsed.append(json.loads((out / f'{index}.metrics.json').read_text())['elapsed_ms'])
    canonical = root / f'{name}.canonical'
    result = subprocess.run([str(digest_binary), str(out / 'table-0.arrow'), str(canonical)], check=True, capture_output=True, text=True)
    result = json.loads(result.stdout)
    assert result['rows'] == 10_000_000
    assert capturer['tables'][0]['consistency'] == 'B'
    values = sorted(elapsed)
    result['chunk_count'] = len(entries)
    result['chunk_ms'] = dict(min=min(values), median=statistics.median(values), p95=values[int((len(values)-1)*.95)], max=max(values))
    time_path = root / f'{name}.time'
    if time_path.exists():
        timing = time_path.read_text()
        peak = re.search(r'(\d+)\s+maximum resident set size', timing)
        wall = re.search(r'([\d.]+) real', timing)
        result['peak_client_rss_bytes'] = int(peak[1]) if peak else None
        result['wall_seconds'] = float(wall[1]) if wall else None
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--container', required=True)
    parser.add_argument('--digest-binary', type=Path, required=True)
    parser.add_argument('--once', default='large-clean')
    parser.add_argument('--resumed', default='large-resumed')
    args = parser.parse_args()
    source = source_digest(args.container)
    once = captured(args.root, args.once, args.digest_binary)
    resumed = captured(args.root, args.resumed, args.digest_binary)
    assert source['rows'] == once['rows'] == resumed['rows'] == 10_000_000
    assert source['ordered_sha256'] == once['ordered_sha256'] == resumed['ordered_sha256']
    print(json.dumps(dict(source=source, uninterrupted=once, resumed=resumed, equal=True), indent=2))


if __name__ == '__main__':
    main()
