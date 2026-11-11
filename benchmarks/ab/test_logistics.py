import collections
import json
from pathlib import Path
import tempfile
import unittest

import logistics
import run as ab


class LogisticsTests(unittest.TestCase):
    def test_frozen_bank_matches_questions_and_has_semantic_coverage(self):
        bank = json.loads(Path(__file__).with_name("fixtures").joinpath(
            "logistics-bank.json").read_text())
        questions = bank["questions"]
        self.assertEqual(len(questions), 40)
        self.assertEqual(collections.Counter(q["category"] for q in questions),
                         dict.fromkeys(("code_decoding", "legacy_names",
                                        "undeclared_joins", "calendar", "metric_definitions"), 8))
        self.assertEqual([{k: v for k, v in q.items() if k != "gold"} for q in questions],
                         logistics.questions())
        self.assertEqual({q["track"] for q in bank["security_questions"]},
                         {"permission", "injection"})
        self.assertTrue(all(q["gold"]["completion"] == "complete" for q in questions))
        self.assertEqual(len(bank["source_csv_sha256"]), 10)
        self.assertEqual(bank["seed"], 42)

    def test_logistics_alias_never_silently_runs_demo_subset(self):
        with self.assertRaisesRegex(ab.Invalid, "logistics_requires_published_manifest"):
            ab.main(["ab", "--arms", "c0,c3", "--dataset", "oo-logistics",
                     "--output", "evidence/not-created"])

    def test_binding_requires_visible_role_columns_and_preserves_frozen_gold(self):
        bank_path = Path(__file__).with_name("fixtures") / "logistics-bank.json"
        bank = ab.load(bank_path)
        columns = collections.defaultdict(set)
        for q in bank["questions"]:
            for table, names in q["required_columns"].items():
                columns[table].update(names)
        pack = dict(files=[dict(path="README.md", text="role: `oo_r_owner`")])
        pack["files"] += [dict(path="tables/" + table + ".md",
                               text="\n".join("| `" + name + "` | text |"
                                              for name in sorted(names)))
                          for table, names in columns.items()]
        with tempfile.TemporaryDirectory(dir=ab.ROOT / "evidence") as temporary:
            root = Path(temporary)
            ab.write(root / "pack.json", pack)
            ab.write(root / "receipt.json", dict(
                publication_binding=dict(publication_id="pub1_" + "a" * 64,
                                         data_vector_sha256="b" * 64),
                knowledge_version="test", policy_version="test"))
            logistics.bind(bank_path, root / "pack.json", root / "receipt.json", root / "bound")
            bound = ab.load(root / "bound/input.json")
            self.assertEqual(bound["questions"], bank["questions"] + bank["security_questions"])
            self.assertEqual(bound["inputs"]["logistics_bank"]["sha256"],
                             ab.digest(bank_path.read_bytes()))
            pack["files"] = pack["files"][:1]
            ab.write(root / "hidden.json", pack)
            with self.assertRaisesRegex(ab.Invalid, "accuracy_not_answerable"):
                logistics.bind(bank_path, root / "hidden.json", root / "receipt.json",
                               root / "rejected")
            self.assertFalse((root / "rejected").exists())


if __name__ == "__main__":
    unittest.main()
