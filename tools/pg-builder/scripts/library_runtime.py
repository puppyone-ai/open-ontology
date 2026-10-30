"""Prepare an operator-local Gateway bundle from a completed Library package.

This does not grant database access, issue tokens, activate a release, or claim
authentication by the self-build HTTP service.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path

import library_selfbuild as lib


def bundle(producer, effective, visibility, review, binding, delivery):
    lib.verify_pack_artifact(producer['artifact'])
    if (producer.get('protocol') != 'oo.selfbuild.library-pack/v1'
            or producer.get('business_owner_signed') is not False
            or producer.get('published') is not False
            or producer.get('effective_sha256') != lib.digest(effective)
            or producer.get('visibility_sha256') != lib.digest(visibility)
            or producer.get('review_sha256') != lib.digest(review)
            or producer.get('pack_digest') != producer['artifact']['pack_digest']
            or delivery.get('stage') != 'packaged'
            or delivery.get('pack_digest') != producer['pack_digest']
            or review.get('review_origin') != 'operator_pinned_file'
            or review.get('project') != effective['project']
            or (review.get('role'), review.get('principal')) !=
               (visibility['role'], visibility['principal'])):
        raise ValueError('runtime_package_binding')
    bindings = binding['bindings']
    names = [b['dataset'] for b in bindings]
    if (len(names) != len(set(names))
            or not {t['dataset'] for t in visibility['datasets']} <= set(names)
            or binding['publication']['receipt_sha256'] != lib.digest(binding['publication']['receipt'])):
        raise ValueError('runtime_source_binding')
    return {
        'protocol': 'oo.gateway.library-reviewed/v1',
        'envelope': {
            'contract': 'oo.operator.library-pack/v1', 'operation': 'prepareLocalBuildPack',
            'management_identity': 'operator_local', 'producer_authenticated': False,
            'published': False, 'active': False, 'compiled_pack': 'not_published',
            'data_grants': 'none', 'producer': producer, 'output_sha256': lib.digest(producer),
            'review': review,
        },
        'effective': effective, 'visibility': visibility,
        'publication': binding['publication'], 'bindings': bindings,
        'login': binding['login'], 'ceilings': binding['ceilings'],
    }


def prepare(package, binding_reference, output):
    package, output = Path(package), Path(output).resolve()
    if any((p / '.git').exists() for p in (output, *output.parents)):
        raise ValueError('runtime_output_must_be_private')
    raw = Path(binding_reference['path']).read_bytes()
    if hashlib.sha256(raw).hexdigest() != binding_reference['sha256']:
        raise ValueError('runtime_binding_digest')
    read = lambda name: json.loads((package / name).read_text())
    document = bundle(read('pack.json'), read('effective-oos.json'), read('visibility.json'),
                      read('effective-review-input.json'), json.loads(raw), read('delivery.json'))
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    lib.write(output / 'library.json', document)
    lib.write(output / 'pack.json', document['envelope']['producer']['artifact'])
    status = {'protocol': 'oo.operator.library-bundle/v1', 'role': document['visibility']['role'],
              'pack_digest': document['envelope']['producer']['pack_digest'],
              'published': False, 'grants_changed': False, 'business_accepted': False,
              'library_sha256': hashlib.sha256((output / 'library.json').read_bytes()).hexdigest()}
    lib.write(output / 'status.json', status)
    return status


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True)
    parser.add_argument('--binding', required=True, help='JSON path/sha256 reference to native bindings')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    print(lib.canonical(prepare(args.package, json.loads(Path(args.binding).read_text()), args.output)))
