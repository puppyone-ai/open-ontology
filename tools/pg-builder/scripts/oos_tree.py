"""Materialize generated OOS and actual reviewed overlays, never new approvals."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re

_exporter = Path(__file__).resolve().parents[2] / 'selfbuild-control-api/recipes/export_oos.py'
_spec = importlib.util.spec_from_file_location('export_oos', _exporter)
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)
yaml = _module.yaml

COLLECTIONS = {
    'datasets': ('dataset', None),
    'relationships': ('name', 'relationships'),
    'enums': ('enum', None),
    'glossary': ('term', 'terms'),
    'entities': ('entity', None),
    'metrics': ('metric', 'metrics'),
    'examples': ('id', 'examples'),
}


EVIDENCE_PATH = 'evidence/relationships.json'


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                    separators=(',', ':')).encode()).hexdigest()


def documents(layer):
    result = {}
    for category, (key, wrapper) in COLLECTIONS.items():
        items = layer.get(category, [])
        names = [item[key] for item in items]
        if len(names) != len(set(names)):
            raise ValueError('duplicate_oos_identity')
        if wrapper:
            # Long SQL definitions need byte-based splitting, not only item counts.
            chunk, byte_count, part = [], 64, 0
            for item in items:
                size = len(yaml([item], 1).encode()) + 1
                if size + 64 > 1_048_576:
                    raise ValueError('oos_fragment_budget')
                if chunk and (len(chunk) == 128 or byte_count + size > 1_048_576):
                    result[f'{category}/part-{part:04d}.yaml'] = {
                        'schema_version': 'oos/v1', wrapper: chunk}
                    part += 1
                    chunk, byte_count = [], 64
                chunk.append(item)
                byte_count += size
            if chunk:
                result[f'{category}/part-{part:04d}.yaml'] = {'schema_version': 'oos/v1', wrapper: chunk}
        else:
            for item in items:
                name = item[key]
                if not re.fullmatch(r'[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*', name):
                    raise ValueError('unsafe_oos_identity')
                if category == 'datasets':
                    schema, table = name.split('.', 1)
                    path = f'datasets/{schema}/{table}.yaml'
                else:
                    path = f'{category}/{name}.yaml'
                result[path] = item
    return result


def assembled(files, include_empty):
    result = {category: [] for category in COLLECTIONS} if include_empty else {}
    for path, value in sorted(files.items()):
        category = path.split('/')[0]
        wrapper = COLLECTIONS[category][1]
        result.setdefault(category, []).extend(value[wrapper] if wrapper else [value])
    return result


def source_tree(generated, effective):
    """Keep unreviewed generation separate from ledger-approved changes."""
    allowed = set(COLLECTIONS) | {
        'protocol', 'ontology', 'project', 'ontology_version', 'catalog_version', 'policy', 'knowledge',
        'relationship_evidence'}
    if (set(generated) | set(effective)) - allowed:
        raise ValueError('unsupported_oos_field')
    generated_files = documents(generated)
    curated = {}
    for category, (key, _) in COLLECTIONS.items():
        baseline = {item[key]: item for item in generated.get(category, [])}
        approved = effective.get(category, [])
        identities = {item[key] for item in approved}
        # Examples enter OOS only after execution review, not from model claims.
        if (not set(baseline) <= identities
                or category != 'examples' and set(baseline) != identities):
            raise ValueError('review_changed_oos_membership')
        changed = [item for item in approved if item != baseline.get(item[key])]
        if changed:
            # oo-core's overlay treats enum arrays as unkeyed replacements.
            curated[category] = approved if category == 'enums' else changed
    curated_files = documents(curated)
    files = {'ontology.yaml': effective['ontology'],
             'reviews/decisions.yaml': {'schema_version': 'oos/v1', 'decisions': []}}
    files.update({f'generated/{p}': v for p, v in generated_files.items()})
    files.update({f'curated/{p}': v for p, v in curated_files.items()})
    bindings = {key: effective[key] for key in
                ('project', 'ontology_version', 'catalog_version', 'policy', 'knowledge')}
    bindings['generated_sha256'] = digest(assembled(generated_files, True))
    bindings['curated_sha256'] = digest(assembled(curated_files, False))
    bindings['ontology_sha256'] = digest(effective['ontology'])
    if 'relationship_evidence' in effective:
        # Versioned companion, bound by digest; the compiler checks it against the relationships.
        files[EVIDENCE_PATH] = effective['relationship_evidence']
        bindings['relationship_evidence_sha256'] = digest(effective['relationship_evidence'])
    return files, bindings


def write_source_tree(root, generated, effective):
    files, bindings = source_tree(generated, effective)
    encoded = {path: (json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))
                      if path == EVIDENCE_PATH else
                      yaml(json.loads(json.dumps(value, sort_keys=True, ensure_ascii=False)))).encode() + b'\n'
               for path, value in files.items()}
    if any(len(value) > 1_048_576 for value in encoded.values()):
        raise ValueError('oos_fragment_budget')
    root = Path(root)
    if root.is_symlink():
        raise ValueError('oos_symlink')
    if root.exists():
        if any(p.is_symlink() for p in root.rglob('*')):
            raise ValueError('oos_symlink')
        existing = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()}
        companions = {'README.md', 'curated/README.md', 'reviews/README.md',
                      'explorer.html', 'preview.puppyoneapp', 'ontology-preview.puppyoneapp', '.agent',
                      'AGENTS.md', '.cli/oo'}
        existing = {p for p in existing if p not in companions and Path(p).name != '.DS_Store'}
        if existing != set(encoded) or any((root / p).read_bytes() != b for p, b in encoded.items()):
            raise ValueError('oos_tree_drift')
        return bindings
    root.mkdir(parents=True)
    for path, body in encoded.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open('xb') as stream:
            stream.write(body)
    return bindings
