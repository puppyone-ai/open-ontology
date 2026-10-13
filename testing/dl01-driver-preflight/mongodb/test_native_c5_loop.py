"""Safety checks for Mongo C5 scope changes versus physical deletion."""
import importlib.util
import pathlib
import unittest


MODULE = pathlib.Path(__file__).resolve().parents[3] / "tools/mongodb-preview/scripts/native-c5-loop.py"
spec = importlib.util.spec_from_file_location("native_c5_loop", MODULE)
loop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loop)


class ReconcileTests(unittest.TestCase):
    def test_refused_existing_key_is_held_across_cycles(self):
        previous = {"a": {"units": 1}, "b": {"units": 2}}
        approved = {"b": {"units": 3}}
        refused = [{"key": "a", "fields": ["hiddenNote"]}]
        diff, newly_quarantined = loop.reconcile(previous, approved, refused)
        self.assertEqual(diff, {"inserts": [], "updates": ["b"], "deletes": []})
        held = loop.held_scope_keys(set(), newly_quarantined, refused)
        self.assertEqual(held, {"a"})
        second_diff, second_new = loop.reconcile(approved, approved, refused)
        self.assertEqual(second_diff["deletes"], [])
        self.assertEqual(loop.held_scope_keys(held, second_new, refused), {"a"})
        self.assertEqual(loop.held_scope_keys(held, [], []), set())

    def test_actual_absence_is_delete_and_duplicate_refusal_fails(self):
        diff, held = loop.reconcile({"a": {"units": 1}}, {}, [])
        self.assertEqual(diff["deletes"], ["a"])
        self.assertEqual(held, [])
        with self.assertRaisesRegex(RuntimeError, "ambiguous"):
            loop.reconcile({"a": {}}, {}, [{"key": "a"}, {"key": "a"}])


if __name__ == "__main__":
    unittest.main()
