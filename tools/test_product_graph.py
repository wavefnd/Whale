"""Regression tests for selected Cargo product graph validation."""

import unittest
from unittest.mock import patch

from check_product_graph import check_product, package_names


class ProductGraphTests(unittest.TestCase):
    def test_package_names_rejects_malformed_output(self):
        for output in ("", "whale v0.1.0\ninvalid", "amd64 v0.1.0"):
            with self.subTest(output=output), self.assertRaises(ValueError):
                package_names(output)

    def test_selected_crates_must_match(self):
        result = type("Result", (), {"returncode": 0, "stdout": "whale v0.1.0\namd64 v0.1.0\n", "stderr": ""})()
        with patch("check_product_graph.subprocess.run", return_value=result):
            self.assertEqual(check_product(".", {"amd64"}), {"amd64"})
            with self.assertRaisesRegex(ValueError, "product contains"):
                check_product(".", set())

    def test_cargo_failure_is_reported(self):
        result = type("Result", (), {"returncode": 7, "stdout": "", "stderr": "offline unavailable"})()
        with patch("check_product_graph.subprocess.run", return_value=result):
            with self.assertRaisesRegex(ValueError, "offline unavailable"):
                check_product(".", set())


if __name__ == "__main__":
    unittest.main()
