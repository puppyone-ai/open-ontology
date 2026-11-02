#!/usr/bin/env python3
"""Library inventory, layout and bounded initial import using the source scheduler."""
import argparse
import copy
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

import source_layout
import source_scheduler

PROTOCOL = 'oo.library.manifest/v1'
RESULT_PROTOCOL = 'oo.library.import-result/v1'


def text(value):
    if not isinstance(value, str) or not value.strip() or '\0' in value:
        raise ValueError('identity')
    return value


def digest(manifest):
    return hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(',', ':'),
                                     ensure_ascii=False).encode()).hexdigest()


def plan(library_id, tables, split='prefix', table_cap=16, concurrency=2,
         stagger_seconds=10):
    text(library_id)
    if split not in ('prefix', 'domain', 'explicit', 'size'):
        raise ValueError('split')
    if type(table_cap) is not int or not 1 <= table_cap <= 4096:
        raise ValueError('table_cap')
    candidates, exclusions, inventory, seen = [], [], [], set()
    for original in tables:
        table = copy.deepcopy(original)
        identity = (text(table['schema']), text(table['table']))
        if identity in seen:
            raise ValueError('duplicate_table')
        seen.add(identity)
        inventory.append(dict(schema=identity[0], table=identity[1]))
        if 'exclude_reason' in table:
            exclusions.append(dict(schema=identity[0], table=identity[1],
                                   reason=text(table['exclude_reason']),
                                   upgrade_condition=table.get('upgrade_condition', 'Explicit inventory review required.')))
            continue
        if split == 'explicit' and not table.get('domain'):
            raise ValueError('explicit_group')
        if split == 'prefix':
            table['domain'] = table.get('prefix') or identity[1].split('_')[0]
        elif split == 'domain':
            table['domain'] = table.get('domain') or table.get('prefix') or identity[1].split('_')[0]
        candidates.append(table)
    if split == 'size':
        bins = {}
        for table in sorted(candidates, key=lambda t: (-source_layout.count(t.get('exact_rows'), 'exact_rows'),
                                                       t['schema'], t['table'])):
            schema = table['schema']
            slots = bins.setdefault(schema, [])
            if not slots:
                slots.extend(dict(rows=0, count=0) for _ in range(
                    (sum(t['schema'] == schema for t in candidates) + table_cap - 1) // table_cap))
            index = min((i for i, slot in enumerate(slots) if slot['count'] < table_cap),
                        key=lambda i: (slots[i]['rows'], i))
            slots[index]['count'] += 1
            slots[index]['rows'] += table['exact_rows']
            table['domain'] = f'size_{index:06d}'
    layout = source_layout.plan(candidates, table_cap, table_cap, library_id=library_id)
    result = dict(protocol=PROTOCOL, library_id=library_id,
                  inventory=sorted(inventory, key=lambda t: (t['schema'], t['table'])),
                  sources=layout['sources'], manifests=layout['manifests'],
                  exclusions=sorted(exclusions + layout['exclusions'], key=lambda t: (t['schema'], t['table'])),
                  layout=dict(split=split, table_cap=table_cap),
                  scheduling=dict(concurrency=concurrency, stagger_seconds=stagger_seconds))
    validate(result)
    return result


def validate(manifest):
    if (not isinstance(manifest, dict) or set(manifest) != {
            'protocol', 'library_id', 'inventory', 'sources', 'manifests', 'exclusions', 'layout', 'scheduling'}
            or manifest['protocol'] != PROTOCOL):
        raise ValueError('manifest_protocol')
    text(manifest['library_id'])
    scheduling = manifest['scheduling']
    if (set(scheduling) != {'concurrency', 'stagger_seconds'}
            or type(scheduling['concurrency']) is not int or not 1 <= scheduling['concurrency'] <= 32):
        raise ValueError('concurrency')
    source_scheduler.number(scheduling['stagger_seconds'])
    layout = manifest['layout']
    if (set(layout) != {'split', 'table_cap'} or layout['split'] not in ('prefix', 'domain', 'explicit', 'size')
            or type(layout['table_cap']) is not int or not 1 <= layout['table_cap'] <= 4096):
        raise ValueError('layout')
    expected, covered, keys, sources = set(), set(), set(), set()
    for table in manifest['inventory']:
        identity = (text(table['schema']), text(table['table']))
        if identity in expected:
            raise ValueError('duplicate_inventory')
        expected.add(identity)
    for source in manifest['sources']:
        key, source_id = text(source['key']), text(source['source_id'])
        if key in keys or source_id in sources:
            raise ValueError('duplicate_source')
        keys.add(key)
        sources.add(source_id)
        shard = manifest['manifests'].get(key)
        if (not shard or shard['source_id'] != source_id or shard['shard'] != source['shard']
                or not 1 <= len(source['tables']) <= layout['table_cap']
                or [(t['schema'], t['table']) for t in shard['tables']] != [
                    (source['schema'], t) for t in source['tables']]):
            raise ValueError('shard_manifest')
        for name in source['tables']:
            identity = (text(source['schema']), text(name))
            if identity in covered:
                raise ValueError('duplicate_ownership')
            covered.add(identity)
    if set(manifest['manifests']) != keys:
        raise ValueError('shard_manifest')
    for exclusion in manifest['exclusions']:
        text(exclusion['reason'])
        text(exclusion['upgrade_condition'])
        identity = (text(exclusion['schema']), text(exclusion['table']))
        if identity in covered:
            raise ValueError('duplicate_ownership')
        covered.add(identity)
    if expected != covered:
        raise ValueError('inventory_coverage')


def identities(manifest):
    return {(s['key'], t) for s in manifest['sources'] for t in s['tables']}


def reconcile(manifest, reports):
    allowed, results = identities(manifest), {}
    for report in reports:
        if (report.get('protocol') != RESULT_PROTOCOL or report.get('library_id') != manifest['library_id']
                or report.get('manifest_sha256') != digest(manifest)):
            raise ValueError('result_identity')
        seen = set()
        for entry in report['results']:
            identity = (entry['source'], entry['table'])
            if identity not in allowed or identity in seen:
                raise ValueError('result_coverage')
            seen.add(identity)
            text(entry['reason'])
            results[identity] = entry
    return results


def status(manifest, reports):
    validate(manifest)
    results = reconcile(manifest, reports)
    imported = sum(r['reason'] == 'ok' for r in results.values())
    failed = len(results) - imported
    pending = len(identities(manifest)) - len(results)
    return dict(protocol='oo.library.status/v1', library_id=manifest['library_id'],
                expected=len(manifest['inventory']), imported=imported,
                excluded=len(manifest['exclusions']), failed=failed, pending=pending,
                complete=failed == 0 and pending == 0, publication='unverified')


def import_library(manifest, binding, out, previous=None):
    validate(manifest)
    if set(binding) - {'sources', 'cost_guard', 'tunnel', 'provision_timeout', 'table_timeout'}:
        raise ValueError('binding_fields')
    expected_sources = {s['key'] for s in manifest['sources']}
    actual_sources, actual = set(), set()
    for source in binding['sources']:
        if source['key'] in actual_sources:
            raise ValueError('binding_coverage')
        actual_sources.add(source['key'])
        source_scheduler.argv_check(source['provision'])
        for table in source['tables']:
            source_scheduler.argv_check(table['argv'])
            identity = (source['key'], table['table'])
            if identity in actual:
                raise ValueError('binding_coverage')
            actual.add(identity)
    if actual_sources != expected_sources or actual != identities(manifest):
        raise ValueError('binding_coverage')
    prior = reconcile(manifest, [] if previous is None else [previous])
    retry = None if previous is None else dict(failed=[dict(source=s, table=t) for s, t in sorted(
        identities(manifest)) if (s, t) not in prior or prior[(s, t)]['reason'] != 'ok'])
    spec = dict(binding, **manifest['scheduling'])
    checkpoint = Path(out)/'summary.json'
    if Path(out).exists():
        raise FileExistsError(out)
    attempt = None
    try:
        attempt = source_scheduler.run(spec, out, retry)
    finally:
        if attempt is None and checkpoint.exists():
            attempt = json.loads(checkpoint.read_text())
            reported = {(entry['source'], entry['table']) for entry in attempt['results']}
            wanted = identities(manifest) if retry is None else {
                (entry['source'], entry['table']) for entry in retry['failed']}
            attempt['results'].extend(dict(source=s, table=t, reason='runner_aborted')
                                      for s, t in sorted(wanted - reported))
            attempt['failed'] = [entry for entry in attempt['results'] if entry['reason'] != 'ok']
        if attempt is not None:
            for entry in attempt['results']:
                prior[(entry['source'], entry['table'])] = entry
            result = dict(protocol=RESULT_PROTOCOL, library_id=manifest['library_id'],
                          manifest_sha256=digest(manifest), attempt=attempt,
                          results=[prior[k] for k in sorted(prior)], published=False)
            result['coverage'] = status(manifest, [result])
            path = Path(out)/'library-result.json'
            temporary = path.with_suffix('.tmp')
            temporary.write_text(json.dumps(result, indent=2) + '\n')
            temporary.replace(path)
    return result


def membership(manifest):
    if manifest.get('protocol') == PROTOCOL:
        validate(manifest)
        return dict(protocol='oo.library.membership/v1', library_id=manifest['library_id'],
                    sources=[dict(source_id=s['source_id'], schema=s['schema'], tables=s['tables'])
                             for s in manifest['sources']])
    if (set(manifest) != {'protocol', 'library_id', 'sources'}
            or manifest['protocol'] != 'oo.library.membership/v1'):
        raise ValueError('membership_protocol')
    text(manifest['library_id'])
    seen_sources, seen_tables = set(), set()
    if not manifest['sources']:
        raise ValueError('membership_empty')
    for source in manifest['sources']:
        if set(source) != {'source_id', 'schema', 'tables'}:
            raise ValueError('membership_source')
        identity = text(source['source_id'])
        if identity in seen_sources or not source['tables']:
            raise ValueError('duplicate_source')
        seen_sources.add(identity)
        for name in source['tables']:
            table = (text(source['schema']), text(name))
            if table in seen_tables:
                raise ValueError('duplicate_ownership')
            seen_tables.add(table)
    return manifest


def pinned_binary(binding, key):
    binary = binding[key]
    if set(binary) != {'path', 'sha256'}:
        raise ValueError('binary_pin')
    path = Path(binary['path'])
    if not path.is_absolute() or hashlib.sha256(path.read_bytes()).hexdigest() != binary['sha256']:
        raise ValueError('binary_digest')
    return str(path)


def publish_library(manifest, binding, out, prepare_only=False, recover=False):
    members = membership(manifest)
    if set(binding) != {'pipeline', 'coordinator', 'target', 'release_config', 'activation'}:
        raise ValueError('publication_binding')
    pipeline = pinned_binary(binding, 'pipeline')
    coordinator = pinned_binary(binding, 'coordinator')
    activation = copy.deepcopy(binding['activation'])
    required = {'project_id', 'candidate_id', 'source_hash', 'expected_version',
                'schema_name', 'relation_name', 'role_name'}
    if set(activation) != required:
        raise ValueError('activation_fields')
    out = Path(out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    request = dict(protocol='oo.pipeline.native-library-prepare/v1',
                   target=binding['target'], manifest=members)
    prepared_process = subprocess.run([pipeline, 'native-library-prepare'], input=json.dumps(request),
                                      text=True, capture_output=True, timeout=600)
    for suffix, value in [('stdout', prepared_process.stdout), ('stderr', getattr(prepared_process, 'stderr', '')),
                          ('exit', str(prepared_process.returncode))]:
        path = out / ('prepare.' + suffix)
        path.write_text(value)
        path.chmod(0o600)
    if not prepared_process.stdout.strip():
        raise ValueError('library_prepare_failed; private command evidence retained')
    prepared = json.loads(prepared_process.stdout)
    (out/'precheck.json').write_text(json.dumps(prepared, indent=2) + '\n')
    if prepared_process.returncode or prepared.get('precheck') != 'rolled_back':
        raise ValueError('library_precheck_failed')
    publication = prepared['native_mapping']['publication']
    if (publication['receipt']['library']['manifest'] != members
            or publication['receipt_sha256'] != digest(publication['receipt'])):
        raise ValueError('prepared_identity')
    activation.update(protocol='oo.release-coordinator/maintain-native-recover/v1' if recover else 'oo.release-coordinator/maintain-native-activate/v1',
                      native_mapping=prepared['native_mapping'])
    (out/'activation.json').write_text(json.dumps(activation, indent=2) + '\n')
    if prepare_only:
        return dict(protocol='oo.library.publication-result/v1', library_id=members['library_id'],
                    published=False, prepared=True, current_state=prepared.get('current_state', 'unverified'),
                    receipt_sha256=publication['receipt_sha256'])
    result_process = subprocess.run([coordinator, 'maintain-recover' if recover else 'maintain-activate', '--config', binding['release_config']],
                                   input=json.dumps(activation), text=True, capture_output=True, timeout=1800)
    for suffix, value in [('stdout', result_process.stdout), ('stderr', getattr(result_process, 'stderr', '')),
                          ('exit', str(result_process.returncode))]:
        path = out / ('release.' + suffix)
        path.write_text(value)
        path.chmod(0o600)
    if not result_process.stdout.strip():
        raise ValueError('library_release_failed; private command evidence retained')
    result = json.loads(result_process.stdout)
    (out/'release-receipt.json').write_text(json.dumps(result, indent=2) + '\n')
    published = (result_process.returncode == 0 and result.get('knowledge_published') is True
                 and result.get('native_publication', {}).get('receipt_sha256') == publication['receipt_sha256'])
    return dict(protocol='oo.library.publication-result/v1', library_id=members['library_id'],
                published=published, receipt_sha256=publication['receipt_sha256'], release=result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    planning = commands.add_parser('plan')
    planning.add_argument('--library-id', required=True)
    planning.add_argument('--input', required=True, type=Path)
    planning.add_argument('--out', required=True, type=Path)
    planning.add_argument('--split', choices=['prefix', 'domain', 'explicit', 'size'], default='prefix')
    planning.add_argument('--table-cap', type=int, default=16)
    planning.add_argument('--concurrency', type=int, default=2)
    planning.add_argument('--stagger-seconds', type=float, default=10)
    for name in ('adopt', 'publish'):
        publishing = commands.add_parser(name)
        publishing.add_argument('--manifest', required=True, type=Path)
        publishing.add_argument('--binding', required=True, type=Path)
        publishing.add_argument('--out', required=True, type=Path)
        publishing.add_argument('--prepare-only', action='store_true')
        publishing.add_argument('--recover', action='store_true')
    importing = commands.add_parser('import')
    importing.add_argument('--binding', required=True, type=Path)
    importing.add_argument('--out', required=True, type=Path)
    importing.add_argument('--retry-failed', type=Path)
    checking = commands.add_parser('status')
    checking.add_argument('--result', action='append', type=Path, default=[])
    checking.add_argument('--binding', type=Path)
    for command in (importing, checking):
        command.add_argument('--manifest', required=True, type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        if args.command == 'plan':
            result = plan(args.library_id, json.loads(args.input.read_text())['tables'],
                          args.split, args.table_cap, args.concurrency, args.stagger_seconds)
            args.out.mkdir(mode=0o700, parents=True, exist_ok=False)
            (args.out/'library.json').write_text(json.dumps(result, indent=2) + '\n')
            for key, shard in result['manifests'].items():
                (args.out/(key + '-manifest.json')).write_text(json.dumps(shard, indent=2) + '\n')
            print(json.dumps(dict(library_id=result['library_id'], shards=len(result['sources']),
                                  expected=len(result['inventory']))))
            return 0
        manifest = json.loads(args.manifest.read_text())
        if args.command in ('adopt', 'publish'):
            result = publish_library(manifest, json.loads(args.binding.read_text()), args.out, args.prepare_only, args.recover)
            print(json.dumps(result))
            return 0 if result['published'] or args.prepare_only else 1
        if args.command == 'status':
            if args.binding:
                if args.result:
                    raise ValueError('status_mode')
                with tempfile.TemporaryDirectory(prefix='oo-library-status-') as temporary:
                    verified = publish_library(manifest, json.loads(args.binding.read_text()), Path(temporary)/'precheck', True)
                members = membership(manifest)
                imported = sum(len(s['tables']) for s in members['sources'])
                excluded = len(manifest.get('exclusions', []))
                result = dict(protocol='oo.library.status/v1', library_id=members['library_id'],
                              expected=imported + excluded, imported=imported, excluded=excluded,
                              failed=0, pending=0, complete=True, generation_vector_verified=True,
                              publication=verified['current_state'], receipt_sha256=verified['receipt_sha256'])
                print(json.dumps(result))
                return 0
            result = status(manifest, [json.loads(p.read_text()) for p in args.result])
            print(json.dumps(result))
            return 0 if result['complete'] else 1
        result = import_library(manifest, json.loads(args.binding.read_text()), args.out,
                                json.loads(args.retry_failed.read_text()) if args.retry_failed else None)
        print(json.dumps(result['coverage']))
        return 75 if result['attempt']['paused'] else (0 if result['coverage']['complete'] else 1)
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        parser.exit(2, f'library: {error}\n')


if __name__ == '__main__':
    raise SystemExit(main())
