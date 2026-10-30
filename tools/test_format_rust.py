import importlib.util
from pathlib import Path
import unittest
import io
import contextlib
import tempfile
import subprocess
from unittest.mock import patch


MODULE_SPEC = importlib.util.spec_from_file_location(
    "format_rust",
    Path(__file__).with_name("format-rust.py"),
)
FORMAT_RUST = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(FORMAT_RUST)


class ItemSpacingTests(unittest.TestCase):
    def test_attributes_and_comments_remain_with_the_next_function(self):
        source = "fn first() {}\n// next function\n#[inline]\nfn second() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "fn first() {}\n\n// next function\n#[inline]\nfn second() {}\n",
        )

    def test_spaces_between_rust_declarations(self):
        source = "enum First { A }\nstruct Second;\nfn third() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "enum First { A }\n\nstruct Second;\n\nfn third() {}\n",
        )

    def test_spaces_between_nested_impl_methods(self):
        source = "impl Example {\nfn first() {}\nfn second() {}\n}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "impl Example {\nfn first() {}\n\nfn second() {}\n}\n",
        )

    def test_ignores_braces_in_raw_strings(self):
        source = 'fn first() { let text = r##"{{{"##; }\nfn second() {}\n'

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            'fn first() { let text = r##"{{{"##; }\n\nfn second() {}\n',
        )

    def test_label_before_multiline_raw_string_preserves_literal_bytes(self):
        source = "fn main(){'l:loop{print!(\"{}\",r\"'\nfn first() {}\nfn second() {}\n'\");break 'l;}}\n"
        self.assertEqual(FORMAT_RUST.add_item_spacing(source), source)
        self.assertEqual(FORMAT_RUST.add_item_spacing(FORMAT_RUST.add_item_spacing(source)), source)

    def test_char_and_lifetime_before_raw_string_keep_item_spacing(self):
        source = "fn first<'a>(value: &'a str) { let c = 'x'; let s = r\"text\"; }\nfn second() {}\n"
        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "fn first<'a>(value: &'a str) { let c = 'x'; let s = r\"text\"; }\n\nfn second() {}\n",
        )

    def test_spaces_between_wave_items(self):
        source = "struct State {}\nfun first() {}\nexport(c) fun second() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "struct State {}\n\nfun first() {}\n\nexport(c) fun second() {}\n",
        )

    def test_check_mode_reports_tracked_rust_and_wave_files_without_rewriting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixtures = {"fixture.rs": "fn a() {}\nfn b() {}\n", "fixture.wave": "fun a() {}\nfun b() {}\n"}
            for name, content in fixtures.items():
                (root / name).write_text(content)
            inventory = subprocess.CompletedProcess([], 0, b"fixture.rs\0fixture.wave\0")
            with (patch.object(FORMAT_RUST, "ROOT", root),
                  patch.object(FORMAT_RUST, "run_rustfmt") as rustfmt,
                  patch.object(FORMAT_RUST.subprocess, "run", return_value=inventory),
                  patch.object(FORMAT_RUST.sys, "argv", ["format-rust.py", "--check"]),
                  contextlib.redirect_stdout(io.StringIO()) as output,
                  contextlib.redirect_stderr(io.StringIO())):
                self.assertEqual(FORMAT_RUST.main(), 1)
                self.assertIn("fixture.rs", output.getvalue())
                self.assertIn("fixture.wave", output.getvalue())
                for name, content in fixtures.items():
                    self.assertEqual((root / name).read_text(), content)
                    (root / name).write_text(FORMAT_RUST.add_item_spacing(content))
                self.assertEqual(FORMAT_RUST.main(), 0)
                rustfmt.assert_called_with(True)

    def test_does_not_treat_rust_variables_named_fun_as_items(self):
        source = "fn test() {\nlet fun = value;\nfun.blocks.push(value);\n}\n"

        self.assertEqual(FORMAT_RUST.add_item_spacing(source), source)


if __name__ == "__main__":
    unittest.main()
