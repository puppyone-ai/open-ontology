"""No Docker needed: exercise bundle trust, archive and loader boundaries."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
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
            path.parent.mkdir(parents=True, exist_ok=True)
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
        for name in ('runtime.tar', 'postgres.tar', 'bin/oo', 'compose.yaml',
                     'bin/oo-fixture-migrate', 'bin/install-retained-target',
                     'support/' + bundle.MIGRATIONS[0]):
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

    def test_single_descriptor_export_strips_registry_annotations(self):
        path = self.root / 'runtime.tar'
        with tarfile.open(path, 'a') as archive:
            raw = json.dumps({'manifests': [{
                'digest': self.info['id'], 'annotations': {
                    'org.opencontainers.image.ref.name': 'existing-user-image:latest',
                    'org.opencontainers.image.version': '17.11-bookworm',
                }}]}).encode()
            member = tarfile.TarInfo('index.json')
            member.size = len(raw)
            archive.addfile(member, io.BytesIO(raw))
        with self.assertRaisesRegex(ValueError, 'ARCHIVE_TAGS'):
            bundle.archive_identity(path, self.info)
        bundle.single_image_index(path, self.info)
        bundle.archive_identity(path, self.info)

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

    def test_emulation_requires_explicit_matching_linux_client(self):
        self.manifest['platform'] = 'linux/amd64'
        for info in self.manifest['images'].values():
            info['arch'] = 'amd64'
        with patch.object(bundle, 'verify', return_value=self.manifest), \
                patch.object(subprocess, 'check_output', return_value='linux/arm64\n'), \
                patch.object(bundle.platform, 'system', return_value='Linux'), \
                patch.object(bundle.platform, 'machine', return_value='x86_64'), \
                patch.object(remote, 'image_info', return_value=self.manifest['images']['runtime']):
            with self.assertRaisesRegex(ValueError, 'TARGET_PLATFORM'):
                bundle.install(self.root, self.trust)
            bundle.install(self.root, self.trust, emulated=True)
            with patch.object(bundle.platform, 'machine', return_value='aarch64'):
                with self.assertRaisesRegex(ValueError, 'TARGET_PLATFORM'):
                    bundle.install(self.root, self.trust, emulated=True)
            with patch.object(bundle.platform, 'system', return_value='Darwin'):
                with self.assertRaisesRegex(ValueError, 'TARGET_PLATFORM'):
                    bundle.install(self.root, self.trust, emulated=True)

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
        self.assertIn('/build/target/debug/oo-builder', dockerfile)
        self.assertIn('--test native_library_publication --no-run', dockerfile)
        self.assertIn('/out/oo-fixture-migrate', dockerfile)
        self.assertIn('oo-fixture-migrate', bundle.BINARIES)

    def test_staged_target_has_no_checkout_or_host_binaries(self):
        import test_bundle_dind
        harness = self.root / 'harness'
        test_bundle_dind.stage_harness(harness, self.root, self.root / 'out', 'fixture')
        self.assertFalse(list(harness.rglob('Cargo.toml')))
        self.assertFalse(list(harness.rglob('*.rs')))
        self.assertFalse((harness / '.git').exists())
        self.assertEqual((harness / 'target/debug').readlink(), self.root / 'bin')
        probe = subprocess.run(
            [sys.executable, '-I', '-c',
             'import sys; sys.path.insert(0, sys.argv[1]); import t1',
             str(harness / 'tools/oo-test')], capture_output=True, text=True)
        self.assertEqual(probe.returncode, 0, probe.stderr)
        for name in bundle.MIGRATIONS:
            self.assertEqual((harness / name).readlink(), self.root / 'support' / name)

    def test_timeout_cleanup_refuses_unowned_siblings(self):
        import test_bundle_dind
        project = 'oo-a-lite-' + 'a' * 16
        owner = self.root / 'foundation/.oo/lite/owner.json'
        owner.parent.mkdir(parents=True)
        owner.write_text(json.dumps({'project': project}))
        labels = {'oo.lane': 'a', 'com.docker.compose.project': project}
        with patch.object(subprocess, 'check_output', side_effect=['container\n', json.dumps(labels)]), \
                patch.object(subprocess, 'run') as run:
            with self.assertRaises(AssertionError):
                test_bundle_dind.cleanup_siblings(self.root, 'docker')
            run.assert_not_called()

    def test_fixture_forward_preserves_bytes_in_both_directions(self):
        import socket
        import socketserver
        import threading
        import test_bundle_dind
        class Echo(socketserver.BaseRequestHandler):
            def handle(self):
                self.request.sendall(b'ready')
                self.request.sendall(self.request.recv(16))
        with socketserver.TCPServer(('127.0.0.1', 0), Echo) as upstream:
            worker = threading.Thread(target=upstream.handle_request)
            worker.start()
            server, proxy = test_bundle_dind.forward(upstream.server_address)
            try:
                with socket.create_connection(server.server_address, timeout=5) as client:
                    self.assertEqual(client.recv(5), b'ready')
                    client.sendall(b'\x00\xffopaque')
                    self.assertEqual(client.recv(16), b'\x00\xffopaque')
            finally:
                server.shutdown()
                server.server_close()
                proxy.join(timeout=5)
                worker.join(timeout=5)


if __name__ == '__main__':
    unittest.main()
