"""Real CLI/compiler D28 handoff; no model, network, or publication."""
import json
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[4]


class AdjudicatedReview(unittest.TestCase):
    def test_confirmation_applies_evidence_and_preserves_exact_input_bytes(self):
        envelope = json.loads((ROOT / "deploy/lite/demo/confirmed-analyst.json").read_text())
        expected = json.loads((ROOT / "deploy/lite/demo/pack-analyst.json").read_text())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def write(name, value):
                path = root / name
                path.write_text(value if isinstance(value, str) else json.dumps(value))
                path.chmod(0o600)
                return path
            source = write("input.json", envelope["adjudication_input"])
            adj = write("adjudication.json", envelope["adjudication"])
            confirm = write("confirm.json", envelope["confirmation"])
            for flags, name in [
                (["--adjudication", str(adj), "--confirm", str(confirm)], "explicit"),
                (["--review-file", str(confirm)], "build-compatible"),
            ]:
                out = root / (name + ".json")
                result = subprocess.run(
                    [ROOT / "target/debug/oo", "review", "--input", source,
                     "--out", out, "--json", *flags], capture_output=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                reviewed = json.loads(out.read_text())
                self.assertEqual(reviewed["review_decision"], envelope["confirmation"])
                self.assertEqual(reviewed["command"]["adjudication_input"], envelope["adjudication_input"])
                self.assertEqual(reviewed["compiled"]["producer"]["artifact"], expected)
                self.assertNotIn("phone_synthetic", json.dumps(reviewed["compiled"]["producer"]["artifact"]))
            # Evidence can hide an otherwise role-visible column; confirmation is not accept-all.
            doc = envelope["adjudication"]
            item = next(i for i in doc["items"] if i["subject"].get("column") == "demo.calendar.month_no")
            item["decision"] = "hidden"
            preimage = {"decisions": {i["id"]: i["decision"] for i in doc["items"]}, "graph": doc["graph"]}
            doc["summary_sha256"] = hashlib.sha256(json.dumps(
                preimage, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
            write("adjudication.json", doc)
            changed_confirm = dict(envelope["confirmation"], summary_sha256=doc["summary_sha256"])
            write("confirm.json", changed_confirm)
            out = root / "hidden.json"
            hidden = subprocess.run(
                [ROOT / "target/debug/oo", "review", "--input", source, "--out", out,
                 "--review-file", confirm, "--json"], capture_output=True)
            self.assertEqual(hidden.returncode, 0, hidden.stdout + hidden.stderr)
            artifact = json.loads(out.read_text())["compiled"]["producer"]["artifact"]
            table = next(f["text"] for f in artifact["files"] if f["path"] == "tables/demo.calendar.md")
            self.assertNotIn("`month_no`", table)
            # A changed byte must not be silently rebound to the adjudication.
            write("input.json", envelope["adjudication_input"] + " ")
            refused = subprocess.run(
                [ROOT / "target/debug/oo", "review", "--input", source,
                 "--out", root / "refused.json", "--review-file", confirm, "--json"],
                capture_output=True)
            self.assertNotEqual(refused.returncode, 0)
            self.assertFalse((root / "refused.json").exists())


if __name__ == "__main__":
    unittest.main()
