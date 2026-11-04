#!/usr/bin/env python3
"""Single-architecture, content-addressed lite bundles. Never pull or build."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile

import remote

HERE = Path(__file__).resolve().parent
PROTOCOL = 'oo.lite-bundle/v1'
DIGEST = re.compile(r'sha256:[0-9a-f]{64}')
BINARIES = ('oo', 'oo-builder', 'oo-fixture-migrate', 'oo-load', 'oo-pipeline', 'oo-mysql-doctor', 'pg-builder',
            'selfbuild-budget', 'release-coordinator', 'install-retained-target',
            'job-worker', 'oo-gateway', 'issue-token')
FILES = ('compose.yaml', 'postgres.sh', 'scheduler.py', 'bundle.py', 'remote.py', 'oo')
MIGRATIONS = (
    'crates/oo-jobs/migrations/0000_bootstrap_roles.sql',
    'crates/oo-jobs/migrations/0001_jobs_control_expand.sql',
    'crates/oo-jobs/migrations/0002_event_project_binding_expand.sql',
    'crates/oo-jobs/migrations/0003_job_lease_operations_expand.sql',
    'tools/gateway-token-store/migrations/oo_token_control_v1.sql',
    'tools/gateway-token-store/migrations/oo_token_control_v2.sql',
    'tools/release-coordinator/migrations/oo_publish_target_v1.sql',
)
PAYLOAD = (set(FILES) | {'bin/' + name for name in BINARIES}
           | {'runtime.tar', 'postgres.tar'} | {'support/' + name for name in MIGRATIONS})
SERVICES = ('gateway', 'worker', 'builder', 'scheduler', 'model-egress', 'operator-bootstrap')


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(condition, code):
    if not condition:
        raise ValueError(code)


def immutable(ref):
    return bool(DIGEST.fullmatch(ref.rsplit('@', 1)[-1]))


def single_image_index(path, expected):
    """Exclude separately exported referrers; registry attestations stay in CI."""
    temporary = path.with_suffix('.tmp')
    try:
        with tarfile.open(path, 'r') as source:
            if 'index.json' not in source.getnames():
                return
            index = json.load(source.extractfile('index.json'))
            rows = index['manifests']
            if len(rows) == 1:
                return
            selected = [row for row in rows if row['digest'] == expected['id']]
            require(len(selected) == 1, 'BUNDLE_ARCHIVE_MANIFEST')
            require(all(row in selected or row.get('annotations', {}).get(
                'io.containerd.manifest.subject') == expected['id'] for row in rows),
                'BUNDLE_ARCHIVE_MANIFEST')
            index['manifests'] = selected
            raw = json.dumps(index).encode()
            with tarfile.open(temporary, 'w') as target:
                for member in source:
                    if member.name == 'index.json':
                        member.size = len(raw)
                        target.addfile(member, io.BytesIO(raw))
                    else:
                        target.addfile(member, source.extractfile(member) if member.isfile() else None)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def archive_identity(path, expected):
    """Reject tagged/multi-image archives before they can alter daemon state."""
    with tarfile.open(path, 'r') as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        require(len(names) == len(set(names)), 'BUNDLE_ARCHIVE_DUPLICATE')
        for member in members:
            require(not Path(member.name).is_absolute() and '..' not in Path(member.name).parts
                    and (member.isfile() or member.isdir()), 'BUNDLE_ARCHIVE_PATH')
        if 'index.json' in names:
            index = json.load(archive.extractfile('index.json'))
            descriptors = index.get('manifests', [])
            require(len(descriptors) == 1, 'BUNDLE_ARCHIVE_MANIFEST')
            for descriptor in descriptors:
                require(all(key.startswith('containerd.io/distribution.source.')
                            for key in descriptor.get('annotations', {})), 'BUNDLE_ARCHIVE_TAGS')
                require(descriptor['digest'] in (expected['id'], expected.get('source_id')),
                        'BUNDLE_ARCHIVE_ID')
        manifest = archive.extractfile('manifest.json')
        require(manifest is not None, 'BUNDLE_ARCHIVE_MANIFEST')
        rows = json.load(manifest)
        require(len(rows) == 1 and not rows[0].get('RepoTags'), 'BUNDLE_ARCHIVE_TAGS')
        config = archive.extractfile(rows[0]['Config'])
        require(config is not None, 'BUNDLE_ARCHIVE_CONFIG')
        raw = config.read(8 * 1024 * 1024 + 1)
        require(len(raw) <= 8 * 1024 * 1024, 'BUNDLE_ARCHIVE_CONFIG_SIZE')
        require('sha256:' + hashlib.sha256(raw).hexdigest() == expected.get('config_id', expected['id']),
                'BUNDLE_ARCHIVE_ID')
        data = json.loads(raw)
        require((data['os'], data['architecture'], data['rootfs']['diff_ids']) ==
                (expected['os'], expected['arch'], expected['layers']), 'BUNDLE_ARCHIVE_PLATFORM')


def create(output, platform, runtime, postgres):
    require(platform in ('linux/amd64', 'linux/arm64'), 'BUNDLE_PLATFORM')
    require(immutable(runtime) and immutable(postgres), 'BUNDLE_IMMUTABLE_REFERENCE')
    require(not output.exists(), 'BUNDLE_OUTPUT_EXISTS')
    images = {}
    for role, ref in (('runtime', runtime), ('postgres', postgres)):
        info = remote.image_info(ref, platform)
        require(info and info['os'] + '/' + info['arch'] == platform, 'BUNDLE_IMAGE_PLATFORM')
        images[role] = dict(info, source=ref, source_id=remote.image_info(ref)['id'],
                            archive=role + '.tar')
    output.mkdir(mode=0o700)
    (output / 'bin').mkdir()
    for name in FILES:
        shutil.copyfile(HERE / name, output / name)
    for name in MIGRATIONS:
        dest = output / 'support' / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(HERE.parents[1] / name, dest)
    os.chmod(output / 'oo', 0o755)
    cid = None
    try:
        # This container is never started; copy binaries from the exact saved image.
        cid = subprocess.check_output(['docker', 'create', '--platform', platform,
            '--pull', 'never', '--label', 'oo.lane=m', images['runtime']['source_id']],
            text=True, stderr=subprocess.PIPE).strip()
        for name in BINARIES:
            subprocess.run(['docker', 'cp', cid + ':/usr/local/bin/' + name,
                            str(output / 'bin' / name)], check=True, capture_output=True)
            os.chmod(output / 'bin' / name, 0o755)
    finally:
        if cid:
            subprocess.run(['docker', 'rm', cid], check=True, capture_output=True)
    for info in images.values():
        path = output / info['archive']
        subprocess.run(['docker', 'save', '--platform', platform, '--output', str(path), info['source_id']],
                       check=True, capture_output=True)
        single_image_index(path, info)
        # Containerd's image ID can be an OCI index; classic dockerd uses config IDs.
        with tarfile.open(path, 'r') as archive:
            rows = json.load(archive.extractfile('manifest.json'))
            require(len(rows) == 1, 'BUNDLE_ARCHIVE_MANIFEST')
            config = archive.extractfile(rows[0]['Config']).read()
            info['config_id'] = 'sha256:' + hashlib.sha256(config).hexdigest()
        archive_identity(path, info)
    files = {name: sha256(output / name) for name in sorted(PAYLOAD)}
    manifest = {'protocol': PROTOCOL, 'platform': platform, 'images': images,
                'runtime_services': list(SERVICES), 'files': files,
                'signing': 'unsigned; authenticate manifest out of band or with cosign'}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    (output / 'SHA256SUMS').write_text(''.join(f'{digest}  {name}\n' for name, digest in files.items()))
    return {'platform': platform, 'manifest_sha256': sha256(output / 'manifest.json')}


def verify(root, expected):
    require(re.fullmatch('[0-9a-f]{64}', expected or ''), 'BUNDLE_TRUST_REQUIRED')
    require(not (root / 'manifest.json').is_symlink(), 'BUNDLE_SYMLINK')
    require(sha256(root / 'manifest.json') == expected, 'BUNDLE_MANIFEST_CHECKSUM')
    manifest = json.loads((root / 'manifest.json').read_text())
    require(manifest['protocol'] == PROTOCOL, 'BUNDLE_PROTOCOL')
    require(manifest['platform'] in ('linux/amd64', 'linux/arm64'), 'BUNDLE_PLATFORM')
    require(set(manifest['files']) == PAYLOAD, 'BUNDLE_FILE_SET')
    require(set(manifest['images']) == {'runtime', 'postgres'}, 'BUNDLE_IMAGE_SET')
    require(manifest['runtime_services'] == list(SERVICES), 'BUNDLE_SERVICE_SET')
    for name, digest in manifest['files'].items():
        path = root / name
        require(not any(p.is_symlink() for p in [path, *path.parents]), 'BUNDLE_SYMLINK')
        require(path.is_file() and sha256(path) == digest, 'BUNDLE_FILE_CHECKSUM')
    for role, info in manifest['images'].items():
        require(info['archive'] == role + '.tar' and DIGEST.fullmatch(info['id'])
                and DIGEST.fullmatch(info['config_id'])
                and info['os'] + '/' + info['arch'] == manifest['platform'], 'BUNDLE_IMAGE_ID')
        archive_identity(root / info['archive'], info)
    return manifest


def install(root, expected, emulated=False):
    manifest = verify(root, expected)
    daemon = subprocess.check_output(['docker', 'version', '--format',
        '{{.Server.Os}}/{{.Server.Arch}}'], text=True).strip()
    # Explicit client-side emulation only; never silently run a foreign CLI.
    client = {'aarch64': 'arm64', 'x86_64': 'amd64'}.get(platform.machine(), platform.machine())
    require(daemon == manifest['platform'] or (
        emulated and platform.system() == 'Linux' and client == 'amd64'
        and manifest['platform'] == 'linux/amd64' and daemon == 'linux/arm64'),
        'BUNDLE_TARGET_PLATFORM')
    for info in manifest['images'].values():
        current = remote.image_info(info['id'])
        if not current and info.get('config_id'):
            current = remote.image_info(info['config_id'])
        if not current or any(current[key] != info[key] for key in ('os', 'arch', 'layers')):
            with (root / info['archive']).open('rb') as stream:
                current, _ = remote.load_verified(stream, info)
        info['id'] = current['id']
    return manifest


def launch(argv):
    parser = argparse.ArgumentParser(description='Offline lite CLI launcher')
    parser.add_argument('action', choices=['up', 'status', 'down'])
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--manifest-sha256', required=True)
    parser.add_argument('--emulated-client', action='store_true',
                        help='explicit Linux amd64 client on an arm64 Docker daemon')
    args, rest = parser.parse_known_args(argv)
    require(not any(arg == '--target' or arg.startswith('--target=') for arg in rest),
            'BUNDLE_LOCAL_ONLY')
    root = args.bundle.resolve()
    manifest = install(root, args.manifest_sha256, args.emulated_client) if args.action == 'up' else verify(
        root, args.manifest_sha256)
    env = dict(os.environ, OO_LITE_RUNTIME_IMAGE=manifest['images']['runtime']['id'],
               OO_LITE_POSTGRES_IMAGE=manifest['images']['postgres']['id'], OO_LITE_LANE='m',
               DOCKER_DEFAULT_PLATFORM=manifest['platform'])
    # Preserve CLI health semantics: foundations_only remains a nonzero exit.
    return subprocess.run([str(root / 'bin/oo'), args.action, '--target', 'local', *rest],
                          env=env).returncode


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('create')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--platform', choices=['linux/amd64', 'linux/arm64'], required=True)
    p.add_argument('--runtime', required=True)
    p.add_argument('--postgres', required=True)
    for command in ('verify', 'load'):
        p = sub.add_parser(command)
        p.add_argument('--bundle', type=Path, required=True)
        p.add_argument('--manifest-sha256', required=True)
    args = parser.parse_args(argv)
    if args.command == 'create':
        report = create(args.output, args.platform, args.runtime, args.postgres)
    else:
        report = (install if args.command == 'load' else verify)(
            args.bundle.resolve(), args.manifest_sha256)
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, KeyError, subprocess.SubprocessError, tarfile.TarError, remote.Refused) as error:
        code = str(error) if re.fullmatch(r'(BUNDLE|REMOTE)_[A-Z_]+', str(error)) else 'BUNDLE_FAILED'
        print(code + '; no installation success claimed', file=sys.stderr)
        raise SystemExit(1)
