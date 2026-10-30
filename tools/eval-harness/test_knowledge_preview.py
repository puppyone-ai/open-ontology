import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import knowledge_preview as preview


def pack(columns=('id',), role='reader'):
    body = '# Items\n\n| name | type |\n| --- | --- |\n' + ''.join(f'| `{c}` | integer |\n' for c in columns)
    files = [{'path': 'tables/app.items.md', 'text': body, 'bytes': len(body.encode()),
              'sha256': preview.digest(body.encode())}]
    manifest = {'project': 'p', 'role': role, 'policy': {'version': '1'},
                'knowledge': {'version': 'preview'}, 'files': [{k: f[k] for k in ('path', 'bytes', 'sha256')} for f in files]}
    text = preview.canonical(manifest)
    files.append({'path': 'manifest.json', 'text': text.decode(), 'bytes': len(text),
                  'sha256': preview.digest(text)})
    return {'protocol': 'oo.builder.role-pack/v1', 'files': files, 'manifest_sha256': preview.digest(text),
            'entry_count': len(files), 'total_bytes': sum(f['bytes'] for f in files),
            'pack_digest': preview.digest(preview.canonical(sorted(
                [{k: f[k] for k in ('path', 'bytes', 'sha256')} for f in files], key=lambda f: f['path'])))}


class PreviewTest(unittest.TestCase):
    def test_knowledge_requires_current_auth_manifest_seal_and_subset_scope(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            def store(name, value):
                path = root / name
                path.write_bytes(preview.canonical(value))
                return path, preview.digest(path.read_bytes())
            old, new = pack(('id', 'amount')), pack()
            old_path, old_hash = store('active.json', old)
            new_path, new_hash = store('preview.json', new)
            cfg = {'cli_command': ['fixture-cli'], 'pack_file': str(new_path), 'pack_sha256': new_hash,
                   'active_pack_file': str(old_path), 'active_pack_sha256': old_hash,
                   'active_manifest_sha256': old['manifest_sha256']}
            auth = {'text': old['files'][-1]['text'], 'sha256': old['manifest_sha256'],
                    'knowledge_version': '1', 'policy_version': '1'}
            calls = []
            def invoke(command, **kwargs):
                calls.append(command)
                return subprocess.CompletedProcess(command, 0, preview.canonical(auth), b'')
            argv = ['knowledge', 'cat', 'tables/app.items.md', '--json', '--token-file', '/private/token']
            result = preview.execute(cfg, argv, invoke)
            value = json.loads(result.stdout)
            self.assertEqual(value['publication'], 'preview')
            self.assertFalse(value['knowledge_preview']['grants_changed'])
            self.assertEqual(calls[0][1:4], ['knowledge', 'cat', 'manifest.json'])
            bad = copy.deepcopy(auth)
            bad['text'] += 'changed'
            with self.assertRaisesRegex(ValueError, 'ACTIVE_PACK_CHANGED'):
                preview.execute(cfg, argv, lambda *a, **k: subprocess.CompletedProcess([], 0, preview.canonical(bad), b''))
            denied = subprocess.CompletedProcess([], 4, b'{"code":"UNAUTHENTICATED"}', b'')
            self.assertIs(preview.execute(cfg, argv, lambda *a, **k: denied), denied)
            wider, wider_hash = store('wider.json', pack(('id', 'secret')))
            with self.assertRaisesRegex(ValueError, 'EXPAND_SCOPE'):
                preview.execute(dict(cfg, pack_file=str(wider), pack_sha256=wider_hash), argv, invoke)
            other, other_hash = store('other.json', pack(role='other'))
            with self.assertRaisesRegex(ValueError, 'ROLE_BINDING'):
                preview.execute(dict(cfg, pack_file=str(other), pack_sha256=other_hash), argv, invoke)

    def test_query_is_forwarded_unchanged_without_replacing_live_receipts(self):
        args = ['query', 'SELECT COUNT(*) FROM app.items', '--json']
        result = subprocess.CompletedProcess([], 0, b'{"query_id":"q_actual","rows":[[2]]}', b'')
        def invoke(command, **kwargs):
            self.assertEqual(command, ['real-cli', *args])
            return result
        self.assertIs(preview.execute({'cli_command': ['real-cli']}, args, invoke), result)

    def test_path_traversal_and_changed_bytes_are_rejected(self):
        self.assertFalse(preview.safe_path('../outside'))
        self.assertFalse(preview.safe_path('/absolute'))
        self.assertFalse(preview.safe_path('tables//double'))
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'pack.json'
            body = pack()
            body['files'][0]['text'] += 'tampered'
            path.write_bytes(preview.canonical(body))
            with self.assertRaisesRegex(ValueError, 'SEAL'):
                preview.load_pack(path, preview.digest(path.read_bytes()))

    def test_search_ranks_exact_paths_and_reports_complete_pagination(self):
        files = {f'tables/t{i}.md': {'text': '# T\nitem', 'sha256': str(i)} for i in range(25)}
        files['tables/item.md'] = {'text': '# item\nprimary', 'sha256': 'primary'}
        first = preview.search(files, 'item', {'--limit': '20'})
        self.assertEqual(first['total'], 26)
        self.assertEqual(first['hits'][0]['path'], 'tables/item.md')
        self.assertEqual(first['next_offset'], 20)
        self.assertEqual(len(preview.search(files, 'item', {'--offset': '20'})['hits']), 6)


if __name__ == '__main__':
    unittest.main()
