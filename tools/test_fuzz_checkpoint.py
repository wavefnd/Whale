import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fuzz_checkpoint import Checkpoint


class CheckpointTests(unittest.TestCase):
    def test_original_and_literal_replay_are_ready_before_native_call(self):
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Checkpoint(directory, 'case', ['python', 'a file.py', '--library', '$x.so'])
            checkpoint.before(b'\x00\xff', seed=7, case=3)
            record = json.loads(checkpoint.manifest.read_text())
            self.assertEqual(checkpoint.input.read_bytes(), b'\x00\xff')
            self.assertEqual(record['replay'][:4], ['python', 'a file.py', '--library', '$x.so'])
            self.assertEqual(record['case'], 3)
            checkpoint.finish(cases=4)
            self.assertTrue((Path(directory) / 'case.completed.json').is_file())

    def test_rejected_checkpoint_preserves_previous_original(self):
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Checkpoint(directory, 'case', [], max_bytes=2)
            checkpoint.before(b'ok')
            with self.assertRaises(ValueError):
                checkpoint.before(b'too long')
            self.assertEqual(checkpoint.input.read_bytes(), b'ok')

    def test_atomic_publish_failure_preserves_original(self):
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Checkpoint(directory, 'case', [])
            checkpoint.before(b'old')
            with patch('fuzz_checkpoint.os.replace', side_effect=OSError('failure')):
                with self.assertRaises(OSError):
                    checkpoint.before(b'new')
            self.assertEqual(checkpoint.input.read_bytes(), b'old')
            self.assertEqual(set(Path(directory).iterdir()), {checkpoint.input, checkpoint.manifest})

    def test_manifest_failure_keeps_previous_pair_consistent(self):
        import os
        with tempfile.TemporaryDirectory() as directory:
            checkpoint = Checkpoint(directory, 'case', [])
            checkpoint.before(b'old')
            previous = checkpoint.input
            replace = os.replace
            def publish(source, destination):
                if Path(destination) == checkpoint.manifest:
                    raise OSError('manifest failure')
                replace(source, destination)
            with patch('fuzz_checkpoint.os.replace', side_effect=publish):
                with self.assertRaises(OSError):
                    checkpoint.before(b'new')
            record = json.loads(checkpoint.manifest.read_text())
            self.assertEqual(Path(record['input']), previous)
            self.assertEqual(previous.read_bytes(), b'old')
            self.assertEqual(len(list(Path(directory).iterdir())), 2)

    def test_new_campaign_removes_old_completion(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Checkpoint(directory, 'case', [])
            first.before(b'old')
            first.finish(cases=1)
            second = Checkpoint(directory, 'case', [])
            second.before(b'new')
            self.assertFalse((Path(directory) / 'case.completed.json').exists())
            self.assertEqual(len(list(Path(directory).iterdir())), 2)


if __name__ == '__main__':
    unittest.main()
