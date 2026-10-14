#!/usr/bin/python3
"""Seal one admitted pgoutput transaction for an operator-pinned occurrence.

A transaction may name one table. Unmentioned tables keep their mappings.
"""
import sys, json, pathlib, subprocess, time, shutil, fcntl, os
from prepare import prepare, read
from transaction_group import already_applied, assemble, load_admitted
R = pathlib.Path('/finish/continuous')
B = 'b942d94a-1d31-40c6-8072-a987912af946'


def reusable(pin, generation, acknowledged_scope):
    # ponytail: only the nearest capture-ready pin. A second exhausted spool needs that pin named.
    root = R / 'runtime' / B
    for index in range(pin - 1, 0, -1):
        prior = root / str(index)
        if not (prior / 'capture-ready').exists():
            continue
        produced = prior / 'producer.json'
        if not produced.exists():
            entries, scopes, vector_entries = load_admitted(prior / 'capture')
            _, scope, _ = assemble(entries, scopes, vector_entries, generation)
            if already_applied(scope, acknowledged_scope):
                return None
            return prior, None
        body = read(produced)
        if body.get('source_generation') == generation:
            if already_applied(body.get('transaction_scope'), acknowledged_scope):
                return None
            return prior, body
        return None
    return None


def write_body(out, generation, pin, scope, chosen, vec):
    vector = {'protocol': 'oo.pipeline.native-transaction-registry-vector/v1', 'entries': vec}
    body = {
        'protocol': 'oo.jobs.native-transaction-producer/v1', 'binding': B, 'pin': pin,
        'source_id': 'oo_pg46_finish', 'capture_method': 'c1_cdc', 'source_generation': generation,
        'transaction_scope': scope, 'entries': chosen,
    }
    (out / 'vector.json').write_text(json.dumps(vector, separators=(',', ':')) + '\n')
    cached = out / 'producer.json'
    if not cached.exists():
        with cached.open('x') as handle:
            handle.write(json.dumps(body, separators=(',', ':')) + '\n')
            handle.flush()
            os.fsync(handle.fileno())
    return cached.read_bytes()


def main():
    c = json.load(sys.stdin)
    assert c['protocol'] == 'oo.jobs.native-producer/v1' and c['binding'] == B and c['source_id'] == 'oo_pg46_finish' and c['method'] == 'C1' and c['cycle'] == 'pull'
    pin = c['pin']
    base = c['acknowledged_source_generation']
    assert isinstance(pin, int) and pin > 0 and isinstance(base, int) and base >= 1
    generation = base + 1
    root = R / 'runtime' / B / str(pin)
    root.mkdir(parents=True, exist_ok=True)
    lock = (root / 'producer.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX)
    cached = root / 'producer.json'
    if cached.exists():
        body = read(cached)
        assert body['source_generation'] == generation and body['pin'] == pin
        sys.stdout.buffer.write(cached.read_bytes())
        return
    ack = R / 'runtime' / B / str(c.get('acknowledged_pin')) / 'producer.json'
    ack_scope = read(ack).get('transaction_scope') if ack.is_file() else None
    if (root / 'capture-ready').exists():
        entries, scopes, vector_entries = load_admitted(root / 'capture')
        chosen, scope, vec = assemble(entries, scopes, vector_entries, generation)
        if already_applied(scope, ack_scope):
            raise RuntimeError('capture-ready transaction was already acknowledged; use a new pin')
        sys.stdout.buffer.write(write_body(root, generation, pin, scope, chosen, vec))
        return
    found = reusable(pin, generation, ack_scope)
    if found is not None:
        prior, previous = found
        if previous is None:
            entries, scopes, vector_entries = load_admitted(prior / 'capture')
            chosen, scope, vec = assemble(entries, scopes, vector_entries, generation)
            write_body(prior, generation, int(prior.name), scope, chosen, vec)
        else:
            chosen, scope, vec = previous['entries'], previous['transaction_scope'], json.loads((prior / 'vector.json').read_text())['entries']
        sys.stdout.buffer.write(write_body(root, generation, pin, scope, chosen, vec))
        return
    cap = root / 'capture'
    if not cap.exists():
        previous = R / 'capture' if pin == 1 else R / 'runtime' / B / str(c['acknowledged_pin']) / 'capture'
        shutil.copytree(previous / 'native-admit', cap / 'native-admit')
    config = read(R / 'source-config.json')
    config['out_dir'] = str(cap)
    config['bins']['preview'] = '/finish/continuous/postgres-preview-fixed'
    config_path = root / 'source-config.json'
    config_path.write_text(json.dumps(config, indent=2) + '\n')
    attempt = 0
    while not (root / 'capture-ready').exists():
        attempt += 1
        ran = subprocess.run([str(R / 'postgres-preview-fixed'), 'capture-receipt-transaction', '--config', str(config_path)], capture_output=True)
        (root / ('capture-%s.stdout' % attempt)).write_bytes(ran.stdout)
        (root / ('capture-%s.stderr' % attempt)).write_bytes(ran.stderr)
        if ran.returncode == 0:
            (root / 'capture-ready').write_text('complete capture; seal retry must reuse these generation bytes\n')
            break
        if ran.returncode != 4:
            raise RuntimeError(ran.stderr.decode())
        time.sleep(1)
    entries, vector = prepare(cap, config_path)
    scopes = load_admitted(cap)[1]
    chosen, scope, vec = assemble(entries, scopes, vector['entries'], generation)
    vector = {'protocol': 'oo.pipeline.native-transaction-registry-vector/v1', 'entries': vec}
    body = {
        'protocol': 'oo.jobs.native-transaction-producer/v1', 'binding': B, 'pin': pin,
        'source_id': 'oo_pg46_finish', 'capture_method': 'c1_cdc', 'source_generation': generation,
        'transaction_scope': scope, 'entries': chosen,
    }
    (root / 'vector.json').write_text(json.dumps(vector, separators=(',', ':')) + '\n')
    with cached.open('x') as handle:
        handle.write(json.dumps(body, separators=(',', ':')) + '\n')
        handle.flush()
        os.fsync(handle.fileno())
    sys.stdout.buffer.write(cached.read_bytes())


if __name__ == '__main__':
    main()
