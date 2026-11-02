"""Resumable local Library build: observation, semantic drafting, YAML and role Pack."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import library_assets as assets
import library_control as control
import library_metrics as metrics
import library_selfbuild as lib


def pinned_review(config, facts):
    reference = config.get('column_review_input')
    if not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}:
        raise ValueError('column_review_required')
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 32_000_000:
        raise ValueError('column_review_file')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('column_review_digest')
    document = json.loads(raw)
    if (set(document) != {'protocol', 'facts_sha256', 'project', 'review'}
            or document['protocol'] != 'oo.library.column-review/v1'
            or document['facts_sha256'] != facts['facts_sha256']
            or document['project'] != config['project']
            or document['review'].get('project') != config['project']
            or document['review'].get('metrics') or document['review'].get('semantic_assets')
            or document['review'].get('metric_amendments')):
        raise ValueError('column_review_binding')
    return dict(document['review'], review_origin='operator_pinned_file')


def stage(request):
    output = Path(request['work_dir'])
    receipt = output / (request['stage'] + '-receipt.json')
    if receipt.exists():
        value = json.loads(receipt.read_text())
        artifact = output / value['artifact']
        if (artifact.is_symlink() or not artifact.is_file()
                or hashlib.sha256(artifact.read_bytes()).hexdigest() != value['artifact_sha256']):
            raise ValueError('stage_artifact_drift')
        if request['stage'] == 'compile':
            # A sealed artifact does not extend a read grant's lifetime.
            return control.run(request)
        return value
    return control.run(request)


def verify_data_only_pack(facts, config, output, result, effective):
    origin = lib.data_only_origin(config)
    uses = lib.data_only_drafts(facts, config)
    if json.loads((output / 'drafts.json').read_text()) != {'receipts': uses, 'failures': []}:
        raise ValueError('data_only_use_drift')
    semantic_digests = {use['semantic_input_digest'] for use in uses}
    if len(semantic_digests) != 1:
        raise ValueError('data_only_semantic_binding')
    lib.verify_pack_artifact(origin['pack']['artifact'])
    if (result['pack_digest'] != origin['pack']['pack_digest']
            or result['artifact'] != origin['pack']['artifact']
            or lib.digest(effective) != origin['pack']['effective_sha256']
            or lib.digest(json.loads((output / 'visibility.json').read_text()))
            != origin['pack']['visibility_sha256']):
        raise ValueError('data_only_pack_drift')
    control.save(output / 'data-only-reuse.json', {
        'protocol': 'oo.library.data-only-pack-use/v1',
        'origin': config['data_only_origin'],
        'origin_facts_sha256': origin['facts']['facts_sha256'],
        'current_facts_sha256': facts['facts_sha256'],
        'semantic_input_digest': next(iter(semantic_digests)),
        'pack_digest': result['pack_digest'], 'new_s5_calls': 0,
        'binding_vector': facts['binding_vector'],
        'review_sha256': lib.digest(json.loads((output / 'effective-review-input.json').read_text())),
        'published': False, 'runtime_execution_verified': False,
    })


def run(config_path, output, through='package', resume=False):
    config_path, output = Path(config_path).resolve(), Path(output).resolve()
    config = json.loads(config_path.read_text())
    if through not in {'observation', 'draft', 'package'}:
        raise ValueError('build_phase')
    if config.get('fixture_review') or config.get('allow_fixture_rebase'):
        raise ValueError('full_build_cannot_use_fixture_approval')
    output.mkdir(parents=True, exist_ok=resume)
    output.chmod(0o700)
    # Approval inputs can arrive after drafting, but observation/provider inputs cannot drift.
    mutable = {'column_review_input', 'metric_review_input', 'semantic_review_input',
               'required_metrics', 'required_assets', 'include_candidate_metrics', 'knowledge_budgets',
               'metric_amendment_input', 'release_versions'}
    control.save(output / 'build-input.json', {
        'protocol': 'oo.library.full-build/v1',
        'config_sha256': lib.digest({k: v for k, v in config.items() if k not in mutable}),
        'draft_contract': assets.DRAFT_CONTRACT})
    manifest_path = Path(config['library_manifest'])
    manifest = json.loads(manifest_path.read_text())
    control.save(output / 'manifest.json', manifest)
    request = {'work_dir': str(output), 'config': str(config_path),
               'manifest': str(output / 'manifest.json'), 'datasets': lib.inventory_names(manifest),
               'max_tables': config.get('max_tables', 4096), 'llm': {}}
    for name in ('S0', 'S1', 'S2', 'S3', 'S4'):
        stage(dict(request, stage=name))
    facts = json.loads((output / 'facts.json').read_text())
    lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'))
    db = lib.ReadOnlyPostgres(config['psql_command'], config.get('statement_timeout_ms', 30000))
    lib.validate_retained_tables(db, json.loads((output / 's0-s2.json').read_text()), config.get('ddl_directory'),
                                oracle_input=config.get('oracle_observation_input'))
    if through == 'observation':
        return {'stage': through, 'tables': len(facts['tables']), 'published': False}
    batches = lib.configured_batches(facts, config)
    preflight = []
    for batch in batches:
        payload, body = lib.draft_request(batch, config)
        preflight.append({'batch': batch['index'], 'tables': len(batch['tables']),
                          'request_sha256': hashlib.sha256(body).hexdigest(),
                          'request_bytes': len(body), 'max_completion_tokens': payload['max_tokens']})
    control.save(output / 'draft-preflight.json', preflight)
    for name in ('S5', 'S6', 'S7', 'S8'):
        stage(dict(request, stage=name))
    drafts = json.loads((output / 'drafts.json').read_text())
    assets.require_draft_coverage(facts, drafts['receipts'])
    if through == 'draft':
        return {'stage': through, 'batches': len(drafts['receipts']), 'published': False}
    profile = config.get('delivery_profile', 'business')
    required_assets = assets.requirements(config)
    if profile not in {'business', 'catalog'}:
        raise ValueError('delivery_profile_invalid')
    if profile == 'business' and set(required_assets) != {
            'datasets', 'relationships', 'enums', 'entities', 'metrics', 'glossary', 'examples'}:
        raise ValueError('business_asset_requirements_incomplete')
    if profile == 'business' and not config.get('release_versions'):
        raise ValueError('business_release_versions_required')
    review = pinned_review(config, facts)
    control.save(output / 'package-input.json', {k: config[k] for k in sorted(mutable) if k in config})
    control.save(output / 'ledger-review.json', review)
    stage(dict(request, stage='compile'))
    compiled = output / 'oos-tree-result.json'
    if not compiled.exists():
        command = [str(Path(config['builder_binary']).with_name('oos-tree')), str(output / 'ontology'),
                   str(output / 'oos-tree-binding.json'), str(output / 'visibility.json'),
                   config['pack_ceilings_file'], str(compiled)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=600)
        if result.returncode:
            raise RuntimeError('oos_tree_compile_refused')
    result = json.loads(compiled.read_text())
    control.save(output / 'oos-tree-result-binding.json', {
        'sha256': hashlib.sha256(compiled.read_bytes()).hexdigest()})
    effective = json.loads((output / 'effective-oos.json').read_text())
    if result['effective_oos'] != effective:
        raise ValueError('oos_roundtrip_drift')
    lib.verify_pack_artifact(result['artifact'])
    if result['pack_digest'] != result['artifact']['pack_digest']:
        raise ValueError('pack_result_digest')
    if config.get('data_only_origin') is not None:
        verify_data_only_pack(facts, config, output, result, effective)
    metrics.require_pack_metrics(result['artifact'], metrics.requirements(config))
    assets.require_pack_assets(result['artifact'], required_assets)
    if not (output / 'knowledge').exists():
        lib.materialize_pack(result['artifact'], output / 'knowledge')
    exported = {f.relative_to(output / 'knowledge').as_posix(): f.read_text()
                for f in (output / 'knowledge').rglob('*') if f.is_file()}
    if exported != {f['path']: f['text'] for f in result['artifact']['files']}:
        raise ValueError('knowledge_export_drift')
    control.save(output / 'pack.json', {
        'protocol': 'oo.selfbuild.library-pack/v1', 'pack_digest': result['pack_digest'],
        'artifact': result['artifact'], 'effective_sha256': lib.digest(effective),
        'visibility_sha256': lib.digest(json.loads((output / 'visibility.json').read_text())),
        'review_sha256': lib.digest(json.loads((output / 'effective-review-input.json').read_text())),
        'business_owner_signed': False, 'published': False})
    report = {'protocol': 'oo.library.build-delivery/v1', 'stage': 'packaged', 'delivery_profile': profile,
              'facts_sha256': facts['facts_sha256'], 'pack_digest': result['pack_digest'],
              'yaml_files': result['yaml_files'], 'knowledge_files': len(exported),
              'collections': {k: len(effective[k]) for k in
                              ('datasets', 'relationships', 'enums', 'entities', 'glossary', 'metrics', 'examples')},
              'available': json.loads(exported['availability.json'])['available'],
              'published': False, 'runtime_execution_verified': False, 'business_accepted': False}
    control.save(output / 'delivery.json', report)
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--through', choices=['observation', 'draft', 'package'], default='package')
    parser.add_argument('--resume', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    try:
        result = run(args.config, args.output, args.through, args.resume)
        print(lib.canonical(result))
    except (ValueError, RuntimeError, OSError, KeyError, subprocess.TimeoutExpired) as error:
        code = str(error)
        if not code.replace('_', '').isalnum() or len(code) > 100:
            code = type(error).__name__
        print(lib.canonical({'stage': 'failed', 'reason': code, 'published': False}))
        raise SystemExit(2) from None


if __name__ == '__main__':
    main()
