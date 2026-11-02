import unittest
from unittest.mock import patch

from renew_control_tls import renew


class RenewalTests(unittest.TestCase):
    def test_invalid_lifetime_never_reads_keys_or_runs_commands(self):
        with patch('renew_control_tls.openssl') as command:
            for days in [0, 366, True, '90']:
                with self.assertRaisesRegex(ValueError, 'certificate_lifetime'):
                    renew('ca', 'key', 'csr', 'ext', '/unused', days)
            command.assert_not_called()

    def test_wrong_ca_key_fails_before_any_certificate_is_written(self):
        with patch('renew_control_tls.openssl', side_effect=[b'public-A', b'public-B']) as command:
            with self.assertRaisesRegex(ValueError, 'ca_key_mismatch'):
                renew('ca', 'key', 'csr', 'ext', '/unused')
            self.assertEqual(command.call_count, 2)


if __name__ == '__main__':
    unittest.main()
