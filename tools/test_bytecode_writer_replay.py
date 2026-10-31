# SPDX-License-Identifier: MPL-2.0
"""Raw writer cases survive failure without dropping malformed native fields."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock

import check_wave_bytecode_writer as writer
from fuzz_checkpoint import Checkpoint


class WriterReplayChecks(unittest.TestCase):
    def test_original_preserves_derived_fields_before_a_failed_native_call(self):
        with tempfile.TemporaryDirectory(prefix="writer replay ") as directory:
            checkpoint = Checkpoint(
                directory, "writer", ["python", "writer.py", "--library", "ir.so"]
            )
            native = writer.Native.__new__(writer.Native)
            native.checkpoint, native.phase, native.case = checkpoint, "mutation/7", 7
            tables = writer.fixture()
            fields = list(tables[4][0])
            fields[9] = 0xDEADBEEF
            fields[19] = writer.MISSING
            tables[4][0] = tuple(fields)
            packet, keep = native.packet(tables)
            with self.assertRaisesRegex(RuntimeError, "native failure"):
                native.remember("write", packet, writer.Limits(), 1024, 4096, -1)
                raise RuntimeError("native failure")
            original = checkpoint.input.read_bytes()
            saved = json.loads(original)
            self.assertEqual(saved["tables"][4][0][9], 0xDEADBEEF)
            self.assertEqual(saved["tables"][4][0][19], writer.MISSING)
            self.assertEqual(saved["tables"][10], list(tables[10]))
            manifest = json.loads(checkpoint.manifest.read_bytes())
            self.assertEqual(manifest["sha256"], hashlib.sha256(original).hexdigest())
            self.assertEqual(manifest["phase"], "mutation/7")
            self.assertEqual(manifest["replay"][-2:], ["--replay", str(checkpoint.input)])
            self.assertTrue(keep)

    def test_invalid_replay_capacities_and_unsigned_fields_fail_before_allocation(self):
        native = writer.Native.__new__(writer.Native)
        packet, keep = native.packet(writer.fixture())
        original = {
            "schema": 1,
            "operation": "write",
            "tables": writer.snapshot(packet),
            "limits": {name: getattr(writer.Limits(), name) for name, _ in writer.Limits._fields_},
            "capacity": 1024,
            "scratch_words": 4096,
            "expected_status": 0,
            "query": False,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "original.json"
            for change in ("capacity", "scratch", "unsigned"):
                record = json.loads(json.dumps(original))
                if change == "capacity":
                    record["capacity"] = 1 << 64
                elif change == "scratch":
                    record["scratch_words"] = 1 << 64
                else:
                    record["tables"][4][0][13] = -1
                path.write_text(json.dumps(record))
                never_entered = Mock()
                with self.subTest(change=change), self.assertRaises(ValueError):
                    writer.replay(never_entered, path)
                never_entered.packet.assert_not_called()
        self.assertTrue(keep)


if __name__ == "__main__":
    unittest.main()
