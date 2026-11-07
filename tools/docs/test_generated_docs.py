import unittest
import tempfile
from pathlib import Path

from gen_cli_reference import ROOT, commands, write_or_check
from gen_dba_grants import statements
from gen_error_reference import codes, route


class HelpDiscovery(unittest.TestCase):
    def test_aliases_and_nested_commands(self):
        self.assertEqual(commands(
            "  oo init|up|down [--json]\n"
            "Usage: oo token issue|renew --grant-file FILE\n"
            "  oo knowledge cat <path>\n"), {
                ("init",), ("up",), ("down",), ("token",),
                ("token", "issue"), ("token", "renew"),
                ("knowledge",), ("knowledge", "cat")})

    def test_prose_and_flags_are_not_commands(self):
        self.assertEqual(commands("Use oo query.\n  --query sql\n"), set())

    def test_grants_exclude_tests_and_preserve_substitutions(self):
        source = ('format!("GRANT SELECT ON {} TO {user};");\n'
                  '#[cfg(test)]\nmod tests {\n"GRANT ALL"\n}\n')
        self.assertEqual(statements(source), [(1, "GRANT SELECT ON {} TO {user};")])

    def test_error_inventory_and_remedy(self):
        self.assertEqual(codes('"UPSTREAM_TIMEOUT"\\n"OO030"\\n"review_required"'),
                         [("UPSTREAM_TIMEOUT", 1), ("OO030", 1), ("review_required", 1)])
        self.assertEqual(route("OO030"), "database-proof")
        self.assertEqual(route("UPSTREAM_TIMEOUT"), "model")

    def test_rust_quote_char_does_not_hide_later_statements(self):
        self.assertEqual(statements("""let quote = '"';\n"ALTER ROLE {} WITH REPLICATION;";"""),
                         [(2, "ALTER ROLE {} WITH REPLICATION;")])

    def test_parser_prefix_is_not_a_grant(self):
        self.assertEqual(statements('strip_prefix("GRANT ")'), [])

    def test_privilege_loop_is_expanded_from_code(self):
        source = ('for privilege in ["BINLOG MONITOR", "REPLICA MONITOR"] {\n'
                  'format!("GRANT {privilege} ON *.* TO {a};")\n}')
        self.assertEqual(statements(source), [
            (2, "GRANT BINLOG MONITOR ON *.* TO {a};"),
            (2, "GRANT REPLICA MONITOR ON *.* TO {a};")])

    def test_check_rejects_drift_without_rewriting(self):
        (ROOT / "evidence").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "evidence") as directory:
            path = Path(directory) / "reference.md"
            path.write_text("stale")
            with self.assertRaises(SystemExit):
                write_or_check(path, "current", True)
            self.assertEqual(path.read_text(), "stale")


if __name__ == "__main__":
    unittest.main()
