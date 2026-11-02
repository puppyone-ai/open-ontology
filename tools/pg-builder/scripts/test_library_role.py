import unittest
from types import SimpleNamespace
from unittest.mock import patch

from library_role import role_sql, run


class LibraryRoleTests(unittest.TestCase):
    def test_role_is_readonly_identity_not_table_access(self):
        sql = role_sql('oo_r_business_admin', 'oo_library_reader')
        for text in ['NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE',
                     'NOINHERIT NOREPLICATION NOBYPASSRLS',
                     'ADMIN FALSE, INHERIT FALSE, SET TRUE', 'unsafe_existing_role',
                     'unexpected_role_membership']:
            self.assertIn(text, sql)
        self.assertNotIn('GRANT SELECT', sql)
        self.assertNotIn('ALTER ROLE', sql)

    def test_invalid_or_privileged_bindings_never_execute(self):
        for role, reader in [('admin', 'oo_reader'), ('oo_r_x;DROP', 'oo_reader'),
                             ('oo_r_admin', 'oo_target_owner'),
                             ('oo_r_admin', 'oo_target_admin'),
                             ('oo_r_admin', 'oo_r_admin')]:
            with self.assertRaises(ValueError):
                role_sql(role, reader)

    @patch('library_role.subprocess.run', return_value=SimpleNamespace(returncode=1))
    def test_failed_provision_never_claims_success(self, _call):
        with self.assertRaisesRegex(ValueError, 'role_provision_failed'):
            run({'role': 'oo_r_admin', 'reader': 'oo_library_reader', 'psql_command': ['psql']})


if __name__ == '__main__':
    unittest.main()
