"""Check local production dependency direction without compiling a product.

Dev-dependencies are deliberately excluded: an independent test oracle may use
an ISA. Optional and target-conditional production dependencies are still checked.
"""

import argparse
import json
from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]
CORE = {
    "diagnostic": set(),
    "target-spec": set(),
    "assembler": {"target-spec", "diagnostic"},
    "ir": {"target-spec", "diagnostic"},
    "backend-api": {"ir", "target-spec", "diagnostic"},
    "object": {"target-spec", "diagnostic"},
    "linker": {"object", "diagnostic"},
    "reverse": {"object"},
    "toolchain": {"assembler", "backend-api", "ir", "target-spec", "diagnostic", "object"},
}


def production_dependencies(manifest):
    for section in ("dependencies", "build-dependencies"):
        yield from manifest.get(section, {}).values()
    for conditional in manifest.get("target", {}).values():
        yield from production_dependencies(conditional)


def audit(root=ROOT):
    root = Path(root).resolve()
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    members = set(workspace["workspace"]["members"]) | {"."}
    packages = {}
    for member in sorted(members):
        location = (root / member).resolve()
        manifest = tomllib.loads((location / "Cargo.toml").read_text())
        name = manifest["package"]["name"]
        if name in packages:
            raise ValueError(f"duplicate package {name}")
        packages[name] = (location, manifest)
    names = {location: name for name, (location, _) in packages.items()}
    edges = {}
    for name, (location, manifest) in sorted(packages.items()):
        dependencies = set()
        for dep in production_dependencies(manifest):
            if isinstance(dep, dict) and "path" in dep:
                path = (location / dep["path"]).resolve()
                if path not in names:
                    raise ValueError(f"{name}: unclassified local dependency {path}")
                dependencies.add(names[path])
        edges[name] = sorted(dependencies)
        if name in CORE:
            forbidden = dependencies - CORE[name]
            if forbidden:
                raise ValueError(f"{name}: forbidden production dependency {sorted(forbidden)}")
    missing = CORE.keys() - packages.keys()
    if missing:
        raise ValueError(f"missing core packages: {sorted(missing)}")
    return {"schema": 1, "production_dependencies": edges}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        print(json.dumps(audit(args.root), sort_keys=True, indent=2))
    except (ValueError, OSError) as error:
        parser.exit(1, f"module layers: {error}\n")


if __name__ == "__main__":
    main()
