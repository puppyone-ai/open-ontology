"""No Docker needed: exercise bundle trust, archive and loader boundaries."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import bundle
import remote


class Bundle(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name).resolve()
        self.config = json.dumps({'os': 'linux', 'architecture': 'arm64',
                                 'rootfs': {'diff_ids': []}}).encode()
        self.info = {'id': 'sha256:' + hashlib.sha256(self.config).hexdigest(),
                     'os': 'linux', 'arch': 'arm64', 'layers': []}
        for name in bundle.PAYLOAD:
            path = self.root / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(b'fixture')
        for role in ('runtime', 'postgres'):
            self.archive(self.root / (role + '.tar'))
        self.manifest = {'protocol': bundle.PROTOCOL, 'platform': 'linux/arm64',
                         'runtime_services': list(bundle.SERVICES),
                         'images': {role: dict(self.info, config_id=self.info['id'], archive=role + '.tar') for role in
                                    ('runtime', 'postgres')}}
        self.seal()

    def tearDown(self):
        self.tmp.cleanup()

    def archive(self, path, tags=None, config=None):
        with tarfile.open(path, 'w') as out:
            for name, raw in [('manifest.json', json.dumps([{
                'Config': 'config.json', 'RepoTags': tags, 'Layers': []}]).encode()),
                              ('config.json', self.config if config is None else config)]:
                item = tarfile.TarInfo(name)
                item.size = len(raw)
                out.addfile(item, io.BytesIO(raw))

    def seal(self):
        self.manifest['files'] = {name: bundle.sha256(self.root / name) for name in bundle.PAYLOAD}
        path = self.root / 'manifest.json'
        path.write_text(json.dumps(self.manifest))
        self.trust = bundle.sha256(path)

    def test_roundtrip_requires_external_manifest_identity(self):
        self.assertEqual(bundle.verify(self.root, self.trust), self.manifest)
        for expected in ('', '0' * 64):
            with self.assertRaises(ValueError):
                bundle.verify(self.root, expected)

    def test_all_payloads_checked_before_load(self):
        for name in ('runtime.tar', 'postgres.tar', 'bin/oo', 'compose.yaml'):
            path = self.root / name
            original = path.read_bytes()
            path.write_bytes(b'tampered')
            with patch.object(remote, 'load_verified') as loader:
                with self.assertRaises(ValueError):
                    bundle.install(self.root, self.trust)
                loader.assert_not_called()
            path.write_bytes(original)

    def test_manifest_cannot_add_escape_paths_or_remove_files(self):
        for name in ('../escape', '/absolute', 'bin/oo'):
            self.manifest['files'][name] = '0' * 64
            if name == 'bin/oo':
                del self.manifest['files'][name]
            path = self.root / 'manifest.json'
            path.write_text(json.dumps(self.manifest))
            with self.assertRaises(ValueError):
                bundle.verify(self.root, bundle.sha256(path))
            self.seal()

    def test_symlinks_rejected_even_when_content_matches(self):
        path = self.root / 'bin/oo'
        path.unlink()
        path.symlink_to('../compose.yaml')
        with self.assertRaises(ValueError):
            bundle.verify(self.root, self.trust)

    def test_tagged_archive_rejected_before_docker_load(self):
        self.archive(self.root / 'runtime.tar', ['existing-user-image:latest'])
        self.seal()
        with self.assertRaisesRegex(ValueError, 'ARCHIVE_TAGS'):
            bundle.verify(self.root, self.trust)

    def test_oci_name_annotation_cannot_bypass_tag_guard(self):
        raw = json.dumps({'manifests': [{'annotations': {
            'org.opencontainers.image.ref.name': 'existing:latest'}}]}).encode()
        with tarfile.open(self.root / 'runtime.tar', 'a') as archive:
            member = tarfile.TarInfo('index.json')
            member.size = len(raw)
            archive.addfile(member, io.BytesIO(raw))
        self.seal()
        with self.assertRaisesRegex(ValueError, 'ARCHIVE_TAGS'):
            bundle.verify(self.root, self.trust)

    def test_only_associated_referrers_are_removed_from_export_index(self):
        for subject, allowed in [(self.info['id'], True), ('sha256:' + '0' * 64, False)]:
            path = self.root / 'runtime.tar'
            self.archive(path)
            raw = json.dumps({'manifests': [{'digest': self.info['id']},
                {'digest': 'sha256:' + '1' * 64, 'annotations': {
                    'io.containerd.manifest.subject': subject}}]}).encode()
            with tarfile.open(path, 'a') as archive:
                member = tarfile.TarInfo('index.json')
                member.size = len(raw)
                archive.addfile(member, io.BytesIO(raw))
            if allowed:
                bundle.single_image_index(path, self.info)
                bundle.archive_identity(path, self.info)
            else:
                with self.assertRaisesRegex(ValueError, 'ARCHIVE_MANIFEST'):
                    bundle.single_image_index(path, self.info)

    def test_config_identity_is_not_just_layer_identity(self):
        config = json.loads(self.config)
        config['config'] = {'Entrypoint': ['wrong']}
        self.archive(self.root / 'runtime.tar', config=json.dumps(config).encode())
        self.seal()
        with self.assertRaisesRegex(ValueError, 'ARCHIVE_ID'):
            bundle.verify(self.root, self.trust)

    def test_wrong_target_architecture_never_loads(self):
        with patch.object(subprocess, 'check_output', return_value='linux/amd64\n'), \
                patch.object(remote, 'load_verified') as loader:
            with self.assertRaisesRegex(ValueError, 'TARGET_PLATFORM'):
                bundle.install(self.root, self.trust)
            loader.assert_not_called()

    def test_installer_reuses_remote_loader_and_is_idempotent(self):
        with patch.object(subprocess, 'check_output', return_value='linux/arm64\n'), \
                patch.object(remote, 'image_info', return_value=None), \
                patch.object(remote, 'load_verified', return_value=(self.info, self.info['id'])) as loader:
            bundle.install(self.root, self.trust)
            self.assertEqual(loader.call_count, 2)
        with patch.object(subprocess, 'check_output', return_value='linux/arm64\n'), \
                patch.object(remote, 'image_info', return_value=self.info), \
                patch.object(remote, 'load_verified') as loader:
            bundle.install(self.root, self.trust)
            loader.assert_not_called()

    def test_remote_loader_verifies_config_id_and_architecture(self):
        result = subprocess.CompletedProcess([], 0, b'Loaded image ID: sha256:fixture\n', b'')
        with patch.object(remote, 'run', return_value=result), \
                patch.object(remote, 'image_info', return_value=self.info):
            self.assertEqual(remote.load_verified(io.BytesIO(), self.info)[0], self.info)
            for key, value in [('id', 'sha256:' + '0' * 64), ('arch', 'amd64')]:
                with self.assertRaises(remote.Refused):
                    remote.load_verified(io.BytesIO(), dict(self.info,
                        config_id='sha256:' + '1' * 64, **{key: value}))

    def test_legacy_ssh_id_is_store_dependent(self):
        result = subprocess.CompletedProcess([], 0, b'Loaded image ID: sha256:fixture\n', b'')
        with patch.object(remote, 'run', return_value=result), \
                patch.object(remote, 'image_info', return_value=self.info):
            info, _ = remote.load_verified(io.BytesIO(), dict(self.info, id='sha256:' + '0' * 64))
            self.assertEqual(info, self.info)

    def test_launcher_preserves_partial_cli_exit_and_pins_database(self):
        with patch.object(bundle, 'install', return_value=self.manifest), \
                patch.object(subprocess, 'run', return_value=subprocess.CompletedProcess([], 1)) as run:
            self.assertEqual(bundle.launch(['up', '--bundle', str(self.root),
                '--manifest-sha256', self.trust, '--config', 'stack.json', '--json']), 1)
            args, kwargs = run.call_args
            self.assertEqual(args[0], [str(self.root / 'bin/oo'), 'up', '--target', 'local',
                                      '--config', 'stack.json', '--json'])
            self.assertEqual(kwargs['env']['OO_LITE_POSTGRES_IMAGE'], self.info['id'])
            self.assertEqual(kwargs['env']['OO_LITE_LANE'], 'm')

    def test_build_definition_has_both_platforms_and_attestations(self):
        root = Path(__file__).resolve().parents[2]
        dockerfile = (root / 'deploy/lite/Dockerfile').read_text()
        for line in dockerfile.splitlines():
            if line.startswith('FROM '):
                self.assertRegex(line, r'@sha256:[0-9a-f]{64}')
        bake = (root / 'deploy/lite/docker-bake.hcl').read_text()
        for value in ('linux/amd64', 'linux/arm64', 'type=provenance,mode=max',
                      'type=sbom,generator=', '@sha256:'):
            self.assertIn(value, bake)
        compose = (root / 'deploy/lite/compose.yaml').read_text()
        self.assertIn('pull_policy: ${OO_LITE_PULL_POLICY:-missing}', compose)
        self.assertIn('pull_policy: never', compose)
        for service in bundle.SERVICES:
            self.assertIn('  ' + service + ':', compose)


if __name__ == '__main__':
    unittest.main()
