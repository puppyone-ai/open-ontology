import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import lifecycle


class Tests(unittest.TestCase):
    def test_grant_is_only_the_read_proof_helper(self):
        self.assertEqual(lifecycle.retained_login_grant('oo_reader'),
            'GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO "oo_reader";\n')
        for login in (None, '', 'public', 'role;RESET ROLE', 'a"b', 'a'*64):
            with self.assertRaises(ValueError):
                lifecycle.retained_login_grant(login)

    def test_up_provisions_configured_data_login_on_agent_db(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            config=root/'work/operator/gateway/config.json'
            config.parent.mkdir(parents=True)
            lifecycle.write(config,json.dumps({'data_user':'oo_reader'}).encode())
            with patch.object(lifecycle.subprocess,'run',return_value=Mock(returncode=0)) as run:
                # PATH without a standalone docker-compose; the resolver must not
                # depend on what the test host has installed.
                lifecycle.provision_retained_login(root,['compose','-p','owned'],{'PATH':directory})
            self.assertEqual(run.call_args.args[0],
                ['docker','compose','-p','owned','exec','-T','agent-db','psql',
                 '-X','-q','-v','ON_ERROR_STOP=1','-U','postgres'])
            self.assertEqual(run.call_args.kwargs['input'],
                lifecycle.retained_login_grant('oo_reader').encode())
            with patch.object(lifecycle.subprocess,'run',return_value=Mock(returncode=1)):
                with self.assertRaisesRegex(ValueError,'LITE_GATEWAY_RETAINED_GRANT'):
                    lifecycle.provision_retained_login(root,[],{})

    def test_operator_only_does_not_get_data_grants(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(lifecycle.subprocess,'run') as run:
                lifecycle.provision_retained_login(root,[],{})
                config=root/'work/operator/gateway/config.json'
                config.parent.mkdir(parents=True)
                lifecycle.write(config,b'{}')
                lifecycle.provision_retained_login(root,[],{})
            run.assert_not_called()


if __name__=='__main__':
    unittest.main()
