#!/usr/bin/python3
"""Bind an existing PG C1 page to its independent admission seal for worker replay.

The page and authoritative seal are immutable inputs. The worker supplies the
real binding/pin and later attaches its own lease; this program never writes a
source database, target database, receipt, lease, or watermark.
"""

import argparse
import hashlib
import json
import os
import pathlib
import sys


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    return json.loads(pathlib.Path(path).read_bytes())


def emit(table, root, operator_root):
    context = json.load(sys.stdin)
    binding = context['binding']
    pin = context['pin']
    if context['protocol'] != 'oo.jobs.native-producer/v1' or context['source_id'] != 'oo_dl01w1_46':
        raise ValueError('worker identity')
    if context['method'] != 'C1' or context['cycle'] != 'pull' or not isinstance(pin, int) or pin < 1:
        raise ValueError('worker method/pin')
    prior = context['acknowledged_source_generation']
    if prior is not None and (not isinstance(prior, int) or prior < 1 or prior >= pin):
        raise ValueError('invalid prior acknowledgment')
    # A cancelled/failed schedule pin is not an applied source generation.
    generation = (prior or 0) + 1

    captured = root / 'native-admit' / table / f'g{generation}.request.json'
    sealed = root / 'sealed-current' / table / str(generation)
    original = read(captured)
    registry_bytes = (sealed / 'registry.json').read_bytes()
    registry = json.loads(registry_bytes)
    decision = read(sealed / 'decision.json')
    if not decision['admitted'] or decision['blocks']:
        raise ValueError('admission refused')
    entries = registry['bindings']
    if len(entries) != 1:
        raise ValueError('registry cardinality')
    evidence = entries[0]
    if (original['logical_schema'] + '.' + original['logical_table'] != table
            or original['source_generation'] != generation
            or original['source_id'] != evidence['source_id']
            or original['snapshot'] != evidence['snapshot']
            or original['input_digest'] != evidence['input_digest']):
        raise ValueError('captured page identity')
    arrow = root / 'native-admit' / table / f'g{generation}.arrow'
    if sha(arrow.read_bytes()) != evidence['input_digest']:
        raise ValueError('captured Arrow changed')
    by_name = {field['name']: field for field in evidence['fields']}
    for column in original['columns']:
        field = by_name.pop(column['name'])
        for name in ('source_oid', 'source_typmod', 'source_format'):
            column[name] = field[name]
    if by_name:
        raise ValueError('source fields missing')

    for name in ('schema_digest', 'type_digest', 'config_digest'):
        original[name] = evidence[name]
    original['audit_digest'] = evidence['capture_receipt_sha256']
    original['admission_path'] = evidence['admission_receipt_path']
    original['admission_expect'] = {'declared_receipt_sha256': evidence['admission_receipt_sha256']}
    original['run_id'] = evidence['run_id']
    # The Loader's default file cap is 1 MiB; this source snapshot is bounded
    # by the operator's 128 MiB budget and a 16 MiB single-page producer cap.
    original['max_arrow_bytes'] = 16 * 1024 * 1024
    original['target']['ca_file'] = '/work/ca-renewed.pem'

    output = operator_root / binding / str(pin) / 'registry.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        if output.read_bytes() != registry_bytes:
            raise ValueError('operator registry changed')
    else:
        temporary = output.with_suffix('.tmp')
        with open(temporary, 'xb') as stream:
            stream.write(registry_bytes)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output)
    original['registry_path'] = str(output)
    sys.stdout.write(json.dumps(original, separators=(',', ':')) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--table', required=True)
    parser.add_argument('--root', type=pathlib.Path, required=True)
    parser.add_argument('--operator-root', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.table not in ('public.hubs', 'public.shipments', 'public.tracking_events',
                          'customer_east.delivery_notes'):
        raise ValueError('unapproved fixture table')
    emit(args.table, args.root, args.operator_root)


if __name__ == '__main__':
    try:
        main()
    except (OSError, KeyError, ValueError, IndexError) as error:
        print(f'pg-native-adapter: {type(error).__name__}: {error}', file=sys.stderr)
        raise SystemExit(2)
