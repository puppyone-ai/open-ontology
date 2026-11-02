"""Compile an unapproved preview Pack within the existing active role's columns."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

import library_assets
import library_control
import library_selfbuild as lib
from oos_tree import write_source_tree


def read_pinned(reference):
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 67_108_864:
        raise ValueError('preview_input_file')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('preview_input_digest')
    return json.loads(raw)


def build(config, output):
    output = Path(output).resolve()
    if any((p / '.git').exists() for p in (output, *output.parents)):
        raise ValueError('preview_output_must_be_outside_git')
    preview_id = config['preview_id']
    if not isinstance(preview_id, str) or not re.fullmatch(r'[a-z][a-z0-9_-]{0,62}', preview_id):
        raise ValueError('preview_id')
    facts = read_pinned(config['facts'])
    receipts = read_pinned(config['drafts'])['receipts']
    active = read_pinned(config['active_pack'])
    lib.require_complete_facts(facts)
    library_assets.require_draft_coverage(facts, receipts)
    lib.verify_pack_artifact(active)
    manifest = json.loads(next(f['text'] for f in active['files'] if f['path'] == 'manifest.json'))
    allowed = {f['path'][7:-3]: set(re.findall(r'^\s*\|\s*`([^`|\n]+)`', f['text'], re.M))
               for f in active['files'] if f['path'].startswith('tables/') and f['path'].endswith('.md')}
    review = {'project': manifest['project'], 'role': manifest['role'], 'principal': 'preview_reader',
              'review_origin': 'operator_pinned_file', 'columns': {}, 'hidden_columns': [],
              'datasets': {}, 'relationships': [], 'reviewer': 'preview_existing_grant_intersection'}
    for table in facts['tables']:
        for column in table['columns']:
            classification = column['classification_candidate']
            if column['name'] in allowed.get(table['dataset'], set()) and classification['pii'] == 'none':
                kind = classification['semantic_type']
                review['columns'][table['dataset'] + '.' + column['name']] = {
                    'pii': 'none', 'semantic_type': 'quantity' if kind == 'money' else kind,
                    'reviewer': review['reviewer'], 'label_approved': False}
    effective, visibility, audit = library_control.reviewed_documents(
        facts, receipts, review, config.get('knowledge_budgets'), include_candidate_metrics=True)
    versions = {'ontology': preview_id, 'catalog': 'local-' + facts['facts_sha256'][:12],
                'policy': manifest['policy']['version'], 'knowledge': preview_id}
    library_assets.bind_release_versions(effective, visibility, versions)
    effective['ontology']['publish']['include_unreviewed'] = list(dict.fromkeys(
        effective['ontology']['publish']['include_unreviewed'] + ['metrics', 'relationships']))
    generated, _, _ = lib.effective_documents(facts, [r['draft'] for r in receipts])
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    binding = write_source_tree(output / 'ontology', generated, effective)
    audit.update(publication='preview', business_owner_signed=False, ledger_bound=False,
                 active_pack_digest=active['pack_digest'], grants_changed=False)
    for name, document in [('effective-oos.json', effective), ('visibility.json', visibility),
                           ('preview-review.json', audit), ('oos-tree-binding.json', binding)]:
        lib.write(output / name, document)
    result = subprocess.run([
        str(Path(config['builder_binary']).with_name('oos-tree')), str(output / 'ontology'),
        str(output / 'oos-tree-binding.json'), str(output / 'visibility.json'),
        config['pack_ceilings_file'], str(output / 'compiled-tree.json')],
        capture_output=True, text=True, timeout=600)
    if result.returncode:
        raise ValueError('preview_core_compile_refused')
    compiled = json.loads((output / 'compiled-tree.json').read_text())
    if compiled['effective_oos'] != effective:
        raise ValueError('preview_yaml_roundtrip')
    lib.verify_pack_artifact(compiled['artifact'])
    lib.write(output / 'preview-pack.json', compiled['artifact'])
    lib.materialize_pack(compiled['artifact'], output / 'knowledge')
    availability = json.loads(next(f['text'] for f in compiled['artifact']['files']
                                  if f['path'] == 'availability.json'))
    status = {'publication': 'preview', 'grants_changed': False, 'business_readiness_verified': False,
              'yaml_roundtrip_equal': True, 'active_pack_digest': active['pack_digest'],
              'pack_digest': compiled['pack_digest'], 'datasets': len(visibility['datasets']),
              'columns': sum(len(t['columns']) for t in visibility['datasets']),
              'candidate_metrics': len(availability['candidate_metrics']),
              'certified_metrics': len(availability['available']['metrics'])}
    lib.write(output / 'preview-status.json', status)
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        print(lib.canonical(build(json.loads(Path(args.config).read_text()), args.output)))
    except (ValueError, OSError, KeyError, subprocess.TimeoutExpired) as error:
        code = str(error)
        print(lib.canonical({'error': code if re.fullmatch(r'preview_[a-z_]+', code) else type(error).__name__,
                             'publication': 'preview', 'grants_changed': False}))
        raise SystemExit(2) from None


if __name__ == '__main__':
    main()
