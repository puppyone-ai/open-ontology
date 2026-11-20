import json, sys, unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import score, score_ask  # noqa: E402


class AskScoring(unittest.TestCase):
    def test_numbers_must_match_and_words_mostly(self):
        self.assertTrue(score_ask.correct("CNY 2,480,000", "The contract is worth CNY 2480000."))
        self.assertFalse(score_ask.correct("CNY 2,480,000", "The contract is worth CNY 2,840,000."))
        self.assertTrue(score_ask.correct("Hengding Optoelectronics; Tianyou Jiang", "Lenses come from Hengding Optoelectronics, owned internally by Tianyou Jiang."))
        self.assertFalse(score_ask.correct("Hengding Optoelectronics; Tianyou Jiang", "Hengding Optoelectronics"))
        self.assertTrue(score_ask.correct("顾明远，产品副总裁", "他在中文材料里是顾明远，职位是产品副总裁。"))
        self.assertTrue(score_ask.correct("2027-03-18", "Launch is on 2027-03-18 [S1]."))
        self.assertFalse(score_ask.correct("2027-03-18", "Launch is on 2027-03-04."))

    def test_dates_in_words_count_as_the_iso_date(self):
        self.assertTrue(score_ask.correct("2027-02-02", "Before 2 February 2027."))
        self.assertTrue(score_ask.correct("2027-02-02", "By February 2, 2027"))
        self.assertTrue(score_ask.correct("2027-03-18", "发布日期是2027年3月18日"))
        self.assertFalse(score_ask.correct("2027-02-02", "Before 3 February 2027."))

    def test_every_part_of_a_multi_part_answer_is_needed(self):
        self.assertFalse(score_ask.correct("Hengding Optoelectronics; Tianyou Jiang", "Hengding Optoelectronics"))

    def test_summary_counts_honesty_and_citations(self):
        qs = [{"id": "q1", "kind": "single", "answer": "9 hours", "answerable": True, "files": ["specs/np7-product-spec.docx"]},
              {"id": "q2", "kind": "unanswerable", "answer": None, "answerable": False, "files": []},
              {"id": "q3", "kind": "unanswerable", "answer": None, "answerable": False, "files": []}]
        res = [{"id": "q1", "status": "answered", "answer": "9 hours", "sources": [{"path": "project/specs/np7-product-spec.docx"}], "ms": 1},
               {"id": "q2", "status": "unanswerable", "sources": [], "ms": 1}, {"id": "q3", "status": "answered", "answer": "42", "sources": [], "ms": 1}]
        import tempfile, subprocess
        with tempfile.TemporaryDirectory() as t:
            (Path(t) / "q.jsonl").write_text("\n".join(json.dumps(q) for q in qs))
            (Path(t) / "r.json").write_text(json.dumps(res))
            out = subprocess.run([sys.executable, "-B", str(HERE / "score_ask.py"), "--ask-results", f"{t}/r.json", "--questions", f"{t}/q.jsonl"], capture_output=True, text=True, check=True).stdout
        s = json.loads(out)
        self.assertEqual((s["correct"], s["cited"], s["honest"], s["made_up"]), (1, 1, 1, 1))


class RelationScoring(unittest.TestCase):
    def world(self):
        return [{"key": "p1", "type": "Person", "name": "顾明远", "names": {"顾明远", "mingyuan gu"}, "files": {"a.docx", "b.pdf", "c.xlsx"}, "blob": "顾明远 mingyuan gu", "blob_by_file": {}},
                {"key": "p2", "type": "Person", "name": "Ruobing Bai", "names": {"ruobing bai"}, "files": {"a.docx"}, "blob": "ruobing bai", "blob_by_file": {}}]

    def test_found_needs_two_files_and_sixty_percent_in_one_entity(self):
        gold = [{"id": "r1", "kind": "same_person", "label": "x", "names": ["顾明远", "Mingyuan Gu"], "files": ["a.docx", "b.pdf", "c.xlsx", "d.md"]},
                {"id": "r2", "kind": "same_person", "label": "y", "names": ["Ruobing Bai"], "files": ["a.docx", "b.pdf"]},
                {"id": "r3", "kind": "figure", "label": "z", "names": ["6,850,000"], "files": ["a.docx", "b.pdf"]}]
        texts = {"a.docx": "顾明远 and Ruobing Bai", "b.pdf": "mingyuan gu", "c.xlsx": "mingyuan gu"}
        summary, rows = score.score_relations(self.world(), gold, texts)
        self.assertEqual([r["found"] for r in rows], [True, False, False])
        self.assertEqual(summary["found"], 1)
        self.assertEqual(summary["shown_links"], 3)
        self.assertEqual(summary["supported_links"], 3)

    def test_unsupported_link_lowers_precision(self):
        world = self.world()
        gold = [{"id": "r1", "kind": "same_person", "label": "x", "names": ["Mingyuan Gu"], "files": ["a.docx", "b.pdf"]}]
        summary, _ = score.score_relations(world, gold, {"a.docx": "nothing here", "b.pdf": "mingyuan gu", "c.xlsx": "顾明远"})
        self.assertEqual((summary["shown_links"], summary["supported_links"]), (3, 2))


class UnlinkedScoring(unittest.TestCase):
    def test_reasons_map_to_the_gold_vocabulary(self):
        gold = [{"file": "misc/a.key", "kind": "unsupported_format"}, {"file": "misc/b.pdf", "kind": "corrupt_file"}, {"file": "misc/e.txt", "kind": "empty_file"},
                {"file": "misc/r.docx", "kind": "unrelated_content"}]
        run = {"files": {"project/misc/a.key": {"state": "unsupported", "detail": {"size": 9}}, "project/misc/b.pdf": {"state": "parse_failed", "detail": {"size": 9}},
                         "project/misc/e.txt": {"state": "published", "detail": {"size": 0}}, "project/misc/r.docx": {"state": "published", "detail": {"size": 9}}}}
        ui = {"misc/a.key": "unsupported", "misc/b.pdf": "parse_failed", "misc/e.txt": "no_entities", "misc/r.docx": "no_entities"}
        summary, rows = score.score_unlinked(gold, run, [], ui)
        self.assertEqual(summary["correct"], 4)
        self.assertEqual({r["file"]: r["mapped"] for r in rows}["misc/r.docx"], "unrelated_content")


if __name__ == "__main__":
    unittest.main()
