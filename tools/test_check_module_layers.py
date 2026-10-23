import copy
from pathlib import Path
import shutil
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import check_module_layers as layers


class LayerTests(unittest.TestCase):
    def test_current_graph_has_no_concrete_isa_below_core(self):
        graph = layers.audit()["production_dependencies"]
        self.assertNotIn("amd64", graph["ir"])
        self.assertIn("object", graph["toolchain"])

    def test_optional_and_target_conditional_dependencies_are_not_hidden(self):
        manifest = {
            "dependencies": {"a": {"path": "a", "optional": True}},
            "build-dependencies": {"b": {"path": "b"}},
            "dev-dependencies": {"c": {"path": "c"}},
            "target": {"cfg(windows)": {"dependencies": {"d": {"path": "d"}}}},
        }
        self.assertEqual(list(layers.production_dependencies(manifest)),
                         [{"path": "a", "optional": True}, {"path": "b"}, {"path": "d"}])

    def test_wrong_layer_fails_even_for_an_existing_dependency(self):
        rules = copy.deepcopy(layers.CORE)
        rules["backend-api"].remove("ir")
        with patch.object(layers, "CORE", rules):
            with self.assertRaisesRegex(ValueError, "backend-api.*forbidden.*ir"):
                layers.audit()

    def test_concrete_isa_dependency_into_ir_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = tomllib.loads((layers.ROOT / "Cargo.toml").read_text())["workspace"]
            for member in [".", *workspace["members"]]:
                path = layers.ROOT / member / "Cargo.toml"
                relative = path.relative_to(layers.ROOT)
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination)
            manifest = root / "ir/Cargo.toml"
            with manifest.open("a") as stream:
                stream.write('\n[target.\'cfg(windows)\'.dependencies]\n'
                             'amd64 = { path = "../assembler/amd64", optional = true }\n')
            with self.assertRaisesRegex(ValueError, "ir.*forbidden.*amd64"):
                layers.audit(root)


if __name__ == "__main__":
    unittest.main()
