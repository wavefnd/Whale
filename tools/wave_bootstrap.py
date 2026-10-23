#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build and reuse the pinned Wave compiler and std without editing its checkout.

The default is offline. The cache publishes one immutable compiler/std/provenance
directory, then atomically replaces a small pointer file. No module registry or
SDK is involved; ordinary internal Wave build rules consume the returned paths.
"""

import argparse
from dataclasses import dataclass
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import uuid


WAVE_REVISION = "e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b"
WAVE_VERSION = "0.0.1-alpha-dev"
RUST_VERSION = "1.89.0"
DEFAULT_RUST_TOOLCHAIN = "1.89.0"
CACHE_SCHEMA = 1
POINTER_NAME = "bootstrap.json"
PROVENANCE_NAME = "provenance.json"


class BootstrapError(ValueError):
    pass


@dataclass(frozen=True)
class Bootstrap:
    compiler: Path
    std_root: Path
    provenance: Path
    metadata: dict
    reused: bool

    def paths(self):
        return {"compiler": str(self.compiler), "std_root": str(self.std_root),
                "provenance": str(self.provenance), "wave_revision": WAVE_REVISION,
                "reused": self.reused}


def run(arguments, **kwargs):
    subprocess.run(list(map(str, arguments)), check=True, **kwargs)


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def _std_digest(root):
    """Hash sorted relative names and payloads; never follow cache symlinks."""
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        raise BootstrapError("cached Wave std must be a regular directory")
    result = hashlib.sha256()
    count = total = 0
    for directory, directories, files in os.walk(root, followlinks=False):
        directories.sort()
        files.sort()
        for name in directories:
            if (Path(directory) / name).is_symlink():
                raise BootstrapError("cached Wave std contains a symlink")
        for name in files:
            path = Path(directory) / name
            metadata = path.lstat()
            if not stat.S_ISREG(metadata.st_mode):
                raise BootstrapError("cached Wave std contains a special file")
            count += 1
            total += metadata.st_size
            if count > 8192 or total > 64 * 1024 * 1024:
                raise BootstrapError("cached Wave std exceeds the bootstrap validation limit")
            relative = path.relative_to(root).as_posix().encode("utf-8")
            result.update(len(relative).to_bytes(4, "little"))
            result.update(relative)
            result.update(metadata.st_size.to_bytes(8, "little"))
            result.update(bytes.fromhex(digest(path)))
    if count == 0:
        raise BootstrapError("cached Wave std is empty")
    return result.hexdigest()


def _read_json(path, maximum):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > maximum:
        raise BootstrapError("invalid or oversized bootstrap metadata")
    try:
        with path.open("rb") as source:
            data = source.read(maximum + 1)
        if len(data) > maximum:
            raise BootstrapError("oversized bootstrap metadata")
        return json.loads(data)
    except (OSError, ValueError, UnicodeError, RecursionError) as error:
        raise BootstrapError("invalid bootstrap metadata") from error


def _compiler_version(compiler):
    environment = os.environ.copy()
    environment.update(NO_COLOR="1", CLICOLOR="0", CLICOLOR_FORCE="0")
    version = subprocess.check_output([str(compiler), "--version"], text=True, env=environment).strip()
    if not version.startswith("wavec " + WAVE_VERSION + " ("):
        raise BootstrapError(f"expected pinned Wave compiler version {WAVE_VERSION}")
    return version


def _host():
    return platform.system().lower() + "-" + platform.machine().lower()


def load_bootstrap(out_dir, *, rustc=None, std_tree=None):
    """Validate a published cache before returning executable/compiler paths."""
    output = Path(out_dir).resolve()
    pointer = _read_json(output / POINTER_NAME, 4096)
    if (not isinstance(pointer, dict) or set(pointer) != {"schema", "generation"}
            or type(pointer["schema"]) is not int or pointer["schema"] != CACHE_SCHEMA
            or not isinstance(pointer["generation"], str)
            or not re.fullmatch(r"[a-f0-9]{32}", pointer["generation"])):
        raise BootstrapError("invalid bootstrap cache pointer")
    generation = output / "generations" / pointer["generation"]
    if (output / "generations").is_symlink() or generation.is_symlink():
        raise BootstrapError("bootstrap generation must not be a symlink")
    provenance = generation / PROVENANCE_NAME
    metadata = _read_json(provenance, 16384)
    required = {"schema", "wave_revision", "wave_version", "std_revision", "std_tree",
                "std_sha256", "rustc", "rust_toolchain", "host", "compiler_version",
                "compiler_sha256", "source_archive_sha256"}
    if not isinstance(metadata, dict) or set(metadata) != required:
        raise BootstrapError("invalid bootstrap provenance schema")
    if (type(metadata["schema"]) is not int or metadata["schema"] != CACHE_SCHEMA
            or metadata["wave_revision"] != WAVE_REVISION
            or metadata["std_revision"] != WAVE_REVISION
            or metadata["wave_version"] != WAVE_VERSION or metadata["host"] != _host()
            or not isinstance(metadata["rustc"], str)
            or not metadata["rustc"].startswith("rustc " + RUST_VERSION + " ")
            or (rustc is not None and metadata["rustc"] != rustc)
            or (std_tree is not None and metadata["std_tree"] != std_tree)):
        raise BootstrapError("stale Wave compiler, Rust compiler or std revision in cache")
    for field in ("std_sha256", "compiler_sha256", "source_archive_sha256"):
        if not isinstance(metadata[field], str) or not re.fullmatch(r"[a-f0-9]{64}", metadata[field]):
            raise BootstrapError("invalid bootstrap provenance digest")
    if (not isinstance(metadata["std_tree"], str)
            or not re.fullmatch(r"[a-f0-9]{40}", metadata["std_tree"])
            or not isinstance(metadata["rust_toolchain"], str)
            or not metadata["rust_toolchain"]):
        raise BootstrapError("invalid bootstrap compiler/std identity")
    compiler = generation / ("wavec.exe" if os.name == "nt" else "wavec")
    if compiler.is_symlink() or not compiler.is_file() or digest(compiler) != metadata["compiler_sha256"]:
        raise BootstrapError("corrupt cached Wave compiler")
    std_root = generation / "std"
    if _std_digest(std_root) != metadata["std_sha256"]:
        raise BootstrapError("corrupt cached Wave std")
    # Hash and std checks precede execution of any cached compiler.
    if _compiler_version(compiler) != metadata["compiler_version"]:
        raise BootstrapError("cached Wave compiler version changed")
    return Bootstrap(compiler, std_root, provenance, metadata, True)


def ensure_bootstrap(wave_source, out_dir, *, rust_toolchain=DEFAULT_RUST_TOOLCHAIN,
                     allow_network=False):
    """Reuse a valid cache or build from git archive of the immutable pin."""
    repository = Path(wave_source).resolve()
    output = Path(out_dir).resolve()
    if output == repository or output.is_relative_to(repository):
        raise BootstrapError("bootstrap output must be outside the read-only Wave checkout")
    revision = subprocess.check_output(
        ["git", "-C", str(repository), "rev-parse", WAVE_REVISION + "^{commit}"], text=True).strip()
    if revision != WAVE_REVISION:
        raise BootstrapError("Wave repository does not contain the pinned immutable commit")
    std_tree = subprocess.check_output(
        ["git", "-C", str(repository), "rev-parse", WAVE_REVISION + ":std"], text=True).strip()
    if not re.fullmatch(r"[a-f0-9]{40}", std_tree):
        raise BootstrapError("invalid pinned Wave std tree")
    # `rustup which` never installs a missing toolchain. Do not let an implicit
    # rustup download precede Cargo's explicit offline policy.
    rustc_path = subprocess.check_output(
        ["rustup", "which", "--toolchain", rust_toolchain, "rustc"], text=True).strip()
    rustc = subprocess.check_output([rustc_path, "--version"], text=True).strip()
    if not rustc.startswith("rustc " + RUST_VERSION + " "):
        raise BootstrapError("the pinned Wave compiler requires installed Rust 1.89.0")
    output.mkdir(parents=True, exist_ok=True)
    if any((output / name).is_symlink() for name in ("build", "generations")):
        raise BootstrapError("bootstrap build/cache directories must not be symlinks")
    if (output / POINTER_NAME).exists():
        try:
            return load_bootstrap(output, rustc=rustc, std_tree=std_tree)
        except (BootstrapError, OSError, subprocess.SubprocessError):
            # An invalid cache is rebuilt from the pin; its current generation
            # remains untouched until the replacement is completely validated.
            pass
    environment = os.environ.copy()
    # Stage 0 runs on this build machine; final Whale host selections and
    # optional Whale archives must not leak into the compiler bootstrap.
    environment.pop("CARGO_BUILD_TARGET", None)
    environment.pop("WHALE_WAVE_ELF_DIR", None)
    environment.pop("WHALE_ASMOBJ_DIR", None)
    environment.pop("WHALE_WAVE_REVERSE_DIR", None)
    environment.update(WAVE_STD_REVISION=WAVE_REVISION, CARGO_INCREMENTAL="0", RUSTUP_AUTO_INSTALL="0")
    if not allow_network:
        environment["CARGO_NET_OFFLINE"] = "true"
    archive = subprocess.check_output(
        ["git", "-C", str(repository), "archive", "--format=tar", WAVE_REVISION])
    with tempfile.TemporaryDirectory(prefix=".wave-bootstrap-", dir=output) as temporary:
        temporary = Path(temporary)
        source = temporary / "source"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
            tree.extractall(source, filter="data")
        command = ["cargo", "+" + rust_toolchain, "build", "--locked", "--jobs", "2",
                   "--manifest-path", source / "Cargo.toml", "--target-dir", output / "build", "--bin", "wavec"]
        if not allow_network:
            command.append("--offline")
        run(command, cwd=source, env=environment)
        built = output / "build/debug" / ("wavec.exe" if os.name == "nt" else "wavec")
        candidate = temporary / "candidate"
        candidate.mkdir()
        compiler = candidate / built.name
        shutil.copy2(built, compiler)
        shutil.copytree(source / "std", candidate / "std")
        compiler_version = _compiler_version(compiler)
        metadata = {
            "schema": CACHE_SCHEMA, "wave_revision": WAVE_REVISION,
            "wave_version": WAVE_VERSION, "std_revision": WAVE_REVISION,
            "std_tree": std_tree, "std_sha256": _std_digest(candidate / "std"),
            "rustc": rustc, "rust_toolchain": rust_toolchain, "host": _host(),
            "compiler_version": compiler_version, "compiler_sha256": digest(compiler),
            "source_archive_sha256": hashlib.sha256(archive).hexdigest(),
        }
        (candidate / PROVENANCE_NAME).write_text(json.dumps(metadata, indent=2) + "\n")
        generation_id = uuid.uuid4().hex
        generations = output / "generations"
        generations.mkdir(exist_ok=True)
        generation = generations / generation_id
        pointer = temporary / POINTER_NAME
        pointer.write_text(json.dumps({"schema": CACHE_SCHEMA, "generation": generation_id}) + "\n")
        os.replace(candidate, generation)
        try:
            os.replace(pointer, output / POINTER_NAME)
        except OSError:
            shutil.rmtree(generation)
            raise
    return Bootstrap(generation / built.name, generation / "std",
                     generation / PROVENANCE_NAME, metadata, False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wave-source", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--rust-toolchain", default=DEFAULT_RUST_TOOLCHAIN)
    parser.add_argument("--allow-network", action="store_true",
                        help="explicitly permit Cargo dependency downloads; Rust must already be installed")
    args = parser.parse_args()
    try:
        result = ensure_bootstrap(args.wave_source, args.out_dir,
                                  rust_toolchain=args.rust_toolchain, allow_network=args.allow_network)
    except (BootstrapError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f"Wave bootstrap failed: {error}\n")
    print(json.dumps(result.paths(), sort_keys=True))


if __name__ == "__main__":
    main()
