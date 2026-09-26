import tempfile
import unittest
from pathlib import Path

from worker.job_runner import unique_path


class UniquePathTest(unittest.TestCase):
    def test_free_name_is_used_as_is(self):
        with tempfile.TemporaryDirectory() as folder:
            target = Path(folder) / "fl2v_00010-audio.mp4"
            self.assertEqual(unique_path(target), target)

    def test_existing_clip_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            first = root / "fl2v_00010-audio.mp4"
            first.write_bytes(b"clip")
            (root / "fl2v_00010-audio-2.mp4").write_bytes(b"clip")
            resolved = unique_path(first)
            self.assertEqual(resolved.name, "fl2v_00010-audio-3.mp4")
            self.assertEqual(resolved.parent, root)


if __name__ == "__main__":
    unittest.main()
