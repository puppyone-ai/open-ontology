"""D15 Builder cycle over an oo-load retained generation (PostgreSQL, MySQL, Oracle).

    python3 retained_build.py --config build.json --output NEW_DIR

The retained cut is the data source: no source connection. An optional
source_catalog pin supplies declared metadata in retained_catalog.PROTOCOL.
With a pinned
`origin` (previous facts plus the owner-reviewed registry), an unchanged
data-only digest reuses the reviewed registry with zero model calls (AT-BLD-02)
and hands it to the publisher for the new generation. Otherwise the bounded
community runner drafts metadata through model-egress and writes the
`oo review` input; publication waits for the owner's self-review.
"""
import argparse
import hashlib
import json
import os
import re
import stat
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

import library_selfbuild as lib
import library_semantics as semantics
import retained_observation
import adjudication

PROTOCOL = 'oo.builder.retained-build/v1'
KEYS = {'protocol', 'psql_command', 'library_id', 'generation', 'source_engine', 'project'}
OPTIONAL = {'origin', 'model', 'statement_timeout_ms', 'max_relationship_checks', 'min_enum_containment',
            'source_catalog', 'relationship_cache'}


def save(path, value):
    path = Path(path)
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as stream:
        stream.write(lib.canonical(value))
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pinned(reference, limit=64_000_000):
    if not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}:
        raise ValueError('retained_origin_pin')
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > limit:
        raise ValueError('retained_origin_file')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('retained_origin_digest')
    return raw


def facts_for(db, config, previous=None, stale_tables=()):
    catalog = json.loads(pinned(config['source_catalog'])) if 'source_catalog' in config else None
    tables, provenance = retained_observation.observe(
        db, config['library_id'], config['generation'], config['source_engine'], catalog,
        previous=previous, stale_tables=stale_tables)
    facts = lib.build_facts(tables, lib.database_containment(db, config.get('max_relationship_checks', 65536)),
                            lib.database_enums(db, config.get('min_enum_containment', .5)))
    lib.require_complete_relationship_observations(facts)
    return facts, provenance


def reviewed_shape(reviewed, facts):
    """Early refusal; the publisher re-binds every column/type to the cut itself."""
    if reviewed.get('protocol') != 'oo.owner-reviewed-registry/v1' or reviewed.get('status') != 'reviewed':
        raise ValueError('retained_origin_review_required')
    decision = reviewed.get('review_decision', {})
    if decision.get('protocol') == 'oo.publish-confirm/v1':
        command = reviewed.get('command', {})
        if (decision != command.get('confirmation') or reviewed.get('published') is not False
                or reviewed.get('authentication') != 'not_claimed'):
            raise ValueError('retained_origin_review_required')
        # Replay the compiler gate; a protocol tag alone is not confirmation.
        if lib.selfbuild_budget.bridge('reviewed', command=command) != reviewed.get('compiled'):
            raise ValueError('retained_origin_compiled_mismatch')
    elif decision.get('mode') != 'self':
        raise ValueError('retained_origin_review_required')
    effective = {d['dataset']: {c['name']: c.get('data_type') for c in d['columns']}
                 for d in reviewed['compiled']['effective']['datasets']}
    current = {t['dataset']: {c['name']: c['data_type'] for c in t['columns']} for t in facts['tables']}
    if not effective or any(not columns or name not in current or any(current[name].get(column) != kind
                                                                     for column, kind in columns.items())
           for name, columns in effective.items()):
        raise ValueError('retained_origin_shape_drift')


def relationship_quality_pending(db, reviewed, facts, scope='library'):
    """Adopted relationships of a confirmed review must still measure the same."""
    document = reviewed.get('command', {}).get('adjudication')
    if not isinstance(document, dict):
        return []
    drift = adjudication.relationship_quality_drift(db, document, facts['tables'])
    return [{'table': scope, 'scope': 'library',
             'changes': ['relationship_quality:' + r for r in drift]}] if drift else []


def inventory(facts):
    return [dict(dataset=t['dataset'], primary_key=t['primary_key'], comment=t.get('table_comment') or None,
                 columns=[dict(name=c['name'], data_type=c['data_type'], nullable=c['nullable'],
                               comment=c.get('comment') or None)
                          for c in t['columns']], foreign_keys=t.get('foreign_keys', []))
            for t in facts['tables']]


def review_input(tables, draft, project):
    """oo review input. Most restrictive classification proposal; the owner decides."""
    versions = dict(project=project, ontology_version='draft-v1', catalog_version='draft-v1',
                    policy=dict(id='owner', version='draft-v1'), knowledge=dict(id='owner', version='draft-v1'))
    effective = dict(versions, protocol='oo.builder.effective-oos/v1',
        ontology=dict(spec_version='oos/v1', name=project, locale='en-US',
            include_schemas=sorted({t['dataset'].split('.')[0] for t in tables}),
            default_tier_preference=['file'], publish=dict(include_unreviewed=[]),
            classification=dict(auto_accept='none'), lifecycle=dict(deprecation_min_releases=2),
            compile=dict(per_role=True, budgets=dict(readme_tokens=2000, table_file_tokens=3000),
                         sample_values=dict(default='masked', max_per_column=0))),
        datasets=[dict(schema_version='oos/v1', dataset=t['dataset'], tier='file', label=t['dataset'],
            primary_key=t['primary_key'],
            columns=[dict(name=c['name'], data_type=c['data_type'], pii='restricted', semantic_type='free_text')
                     for c in t['columns']]) for t in tables])
    for key in ('relationships', 'enums', 'glossary', 'entities', 'metrics', 'examples'):
        effective[key] = []
    visibility = dict(versions, protocol='oo.builder.role-visibility/v1', principal='owner',
        role='oo_r_owner', issued_for_compiler='oo-builder/role-pack/v1',
        datasets=[dict(dataset=t['dataset'], row_security='none', global_stats='denied',
                       columns=[c['name'] for c in t['columns']]) for t in tables],
        limits=dict(max_file_bytes=262144, max_total_bytes=1048576, max_entries=128))
    return dict(protocol='oo.builder.reviewed-community/v1', draft=draft,
                source_draft_sha256=lib.digest(draft), effective=effective, visibility=visibility,
                orphan_threshold=.2, ceilings={})


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def draft(facts, config, output):
    import selfbuild_budget as builder
    model = config.get('model')
    if not isinstance(model, dict) or set(model) != {'stack', 'egress', 'call_limit'}:
        raise ValueError('retained_model_config')
    if type(model['call_limit']) is not int or not 1 <= model['call_limit'] <= 4096:
        raise ValueError('retained_model_budget')
    stack = json.loads(Path(model['stack']).read_text())
    port = free_port()
    # Credentials stay in owner-private secret_ref files read only by model-egress.
    egress = subprocess.Popen([sys.executable, '-I', model['egress'], '--config', model['stack'],
                               '--meter', str(output / 'usage.jsonl'), '--port', str(port),
                               '--timeout', '300'],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(200):
            if egress.poll() is not None:
                raise ValueError('retained_egress_start')
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    break
            except OSError:
                time.sleep(.05)
        else:
            raise ValueError('retained_egress_start')
        settings = dict(model=stack['model']['model'], endpoint=f'http://127.0.0.1:{port}/v1',
                        transport='model-egress', stream=False, request_timeout_seconds=300)
        lock, calls = threading.Lock(), [0]

        def transport(request):
            with lock:
                if calls[0] >= model['call_limit']:
                    raise ValueError('retained_model_call_budget')
                calls[0] += 1
                # Reserve before dispatch: an interrupted request still consumes the limit.
                (output / 'model-call-count.json').write_text(lib.canonical({'calls': calls[0]}))
            return builder.model_call(settings, request)

        tables = inventory(facts)
        runner = builder.Runner(settings, output / 'community', transport)
        result = runner.run(tables, [])
        if result['unresolved'] or result.get('published') is not False:
            raise ValueError('retained_draft_incomplete')
        return result, calls[0]
    finally:
        egress.terminate()
        try:
            egress.wait(timeout=5)
        except subprocess.TimeoutExpired:
            egress.kill()
            egress.wait()


def restore_receipts(output):
    """Resume only model receipts; the Runner rechecks input hashes and semantics."""
    candidates = []
    for path in output.parent.glob(output.name + '-failed-*'):
        suffix = path.name.removeprefix(output.name + '-failed-')
        if suffix.isdigit() and 1 <= int(suffix) < 1000:
            candidates.append((int(suffix), path))
    if not candidates:
        return
    previous = max(candidates)[1]
    source = previous / 'community' / 'receipts'
    if any(p.is_symlink() for p in (previous, previous / 'community', source)):
        raise ValueError('retained_cache_symlink')
    if not source.exists():
        return
    files = sorted(source.iterdir())
    if len(files) > 4096:
        raise ValueError('retained_cache_budget')
    target = output / 'community' / 'receipts'
    target.mkdir(parents=True, mode=0o700)
    total = 0
    for path in files:
        if not re.fullmatch(r'[0-9a-f]{64}\.json', path.name):
            raise ValueError('retained_cache_name')
        with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 8 * 1024 * 1024:
                raise ValueError('retained_cache_file')
            raw = stream.read(8 * 1024 * 1024 + 1)
        total += len(raw)
        if len(raw) > 8 * 1024 * 1024 or total > 64 * 1024 * 1024:
            raise ValueError('retained_cache_budget')
        with os.fdopen(os.open(target / path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as stream:
            stream.write(raw)


def run(config_path, output):
    config = json.loads(Path(config_path).read_text())
    if (config.get('protocol') != PROTOCOL or not KEYS <= set(config) or set(config) - KEYS - OPTIONAL
            or config['source_engine'] not in retained_observation.ENGINES):
        raise ValueError('retained_build_config')
    output = Path(output)
    output.mkdir(mode=0o700)
    restore_receipts(output)
    db = lib.ReadOnlyPostgres(config['psql_command'], config.get('statement_timeout_ms', 30000))
    facts, provenance = facts_for(db, config)
    current = semantics.data_only_digest(facts)
    save(output / 'facts.json', facts)
    report = dict(protocol=PROTOCOL, library_id=config['library_id'], generation=config['generation'],
                  source_engine=config['source_engine'], receipt_sha256=provenance['receipt_sha256'],
                  facts_sha256=facts['facts_sha256'], data_only_digest=current,
                  table_data_only_digests=semantics.table_data_only_digests(facts), published=False)
    origin = config.get('origin')
    if origin is not None:
        if not isinstance(origin, dict) or set(origin) != {'facts', 'reviewed_registry'}:
            raise ValueError('retained_origin_pin')
        old = json.loads(pinned(origin['facts']))
        raw = pinned(origin['reviewed_registry'])
        quality = (relationship_quality_pending(db, json.loads(raw), facts, 'library:' + config['library_id'])
                   if semantics.data_only_digest(old) == current else [])
        if semantics.data_only_digest(old) == current and not quality:
            reviewed_shape(json.loads(raw), facts)
            reuse = dict(protocol='oo.builder.retained-data-only/v1', origin_facts_sha256=old['facts_sha256'],
                         current_facts_sha256=facts['facts_sha256'], data_only_digest=current,
                         reviewed_registry_sha256=origin['reviewed_registry']['sha256'],
                         library_id=config['library_id'], generation=config['generation'],
                         receipt_sha256=provenance['receipt_sha256'], new_model_calls=0,
                         handoff='release-coordinator publish-retained', published=False)
            save(output / 'data-only-reuse.json', reuse)
            return dict(report, stage='data_only', model_calls=0,
                        reviewed_registry=origin['reviewed_registry'])
        report['semantic_change'] = True
        report['review_pending'] = quality + semantics.table_review_pending(old, facts)
    result, calls = draft(facts, config, output)
    save(output / 'draft.json', result)
    command = review_input(inventory(facts), result, config['project'])
    command['adjudication_replica'] = provenance
    if 'review_pending' in report:
        command['review_pending'] = report['review_pending']
    if 'source_catalog' in config:
        # Evidence is visible and pinned by the owner's input digest, not approval.
        command['source_inventory'] = inventory(facts)
        command['source_relationships'] = facts['relationship_candidates']
        command['excluded_foreign_keys'] = [
            dict(dataset=t['dataset'], **fk)
            for t in facts['tables'] for fk in t.get('excluded_foreign_keys', [])]
    adjudication.discover_candidates(command, facts['tables'])
    input_hash = save(output / 'review-input.json', command)
    cache = config.get('relationship_cache')
    if cache is not None and (not isinstance(cache, str) or not cache):
        raise ValueError('retained_relationship_cache')
    decisions = adjudication.collect(db, command, facts['tables'], input_hash,
                                     budget=config.get('max_relationship_checks', 65536),
                                     cache_path=Path(cache) if cache else output / 'adjudication-measurements.json')
    _, current_provenance = facts_for(db, config)
    if current_provenance != provenance:
        raise ValueError('adjudication_retained_cut_changed')
    save(output / 'adjudication.json', decisions)
    return dict(report, stage='review_required', model_calls=calls, review_input='review-input.json',
                adjudication='adjudication.json', summary_sha256=decisions['summary_sha256'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        print(lib.canonical(run(args.config, args.output)))
    except (ValueError, RuntimeError, OSError, KeyError, TypeError, subprocess.TimeoutExpired) as error:
        code = str(error)
        if not code.replace('_', '').replace(':', '').isalnum() or len(code) > 100:
            code = type(error).__name__
        print(lib.canonical({'stage': 'failed', 'reason': code, 'published': False}))
        raise SystemExit(2) from None


if __name__ == '__main__':
    main()
