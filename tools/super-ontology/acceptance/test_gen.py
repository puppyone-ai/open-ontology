import shutil, subprocess, sys, tempfile, unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import gen  # noqa: E402


@unittest.skipUnless(all(shutil.which(t) for t in ("say", "ffmpeg", "ffprobe", "swiftc")), "needs macOS say, ffmpeg and swiftc")
class Gen(unittest.TestCase):
    def test_build_and_gold_are_consistent(self):
        with tempfile.TemporaryDirectory() as t:
            gen.FILES.clear(); gen.TEXT.clear()
            gen.build(t)
        rel, unl, qs = gen.gold(), gen.gold_unlinked(), gen.gold_questions()
        gen.check_gold(rel, unl, qs)
        self.assertGreaterEqual(len(gen.FILES), 40)
        self.assertEqual(sum(1 for q in qs if not q["answerable"]), 4)
        self.assertEqual(len(unl), 7)
        self.assertLess(sum(len(v) for v in gen.FILES.values()), 30_000_000)
        self.assertEqual(gen.FILES["misc/empty-notes.txt"], b"")


if __name__ == "__main__":
    unittest.main()
