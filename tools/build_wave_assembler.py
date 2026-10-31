#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build the explicit shared or ISA Wave assembler module with the pinned compiler, without editing Wave."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

from wave_bootstrap import (DEFAULT_RUST_TOOLCHAIN, RUST_VERSION, WAVE_REVISION,
                            digest, ensure_bootstrap, run)


MODULES = {
    'common': {
        'sources': (
            'assembler/model.wave',
            'assembler/unicode.wave',
            'assembler/numeric.wave',
            'assembler/output.wave',
            'assembler/lexer.wave',
            'assembler/fixed_model.wave',
            'assembler/fixed_text.wave',
            'assembler/fixed_numbers.wave',
            'assembler/fixed_operands.wave',
            'assembler/fixed_namespace.wave',
            'assembler/fixed_relative.wave',
            'assembler/fixed_scalar.wave',
            'assembler/fixed_source.wave',
            'assembler/fixed_driver.wave',
        ),
        'abi': (
            'assembler/src/tokens.rs',
            'assembler/src/tokens_wave.rs',
            'assembler/src/fixed.rs',
            'assembler/src/fixed_wave.rs',
            'assembler/src/lib.rs',
        ),
        'name': 'assembler',
    },
    'amd64': {
        'sources': (
            'assembler/model.wave',
            'assembler/declarations.wave',
            'assembler/ast_model.wave',
            'assembler/amd64/parser_model.wave',
            'assembler/amd64/registers.wave',
            'assembler/amd64/expressions.wave',
            'assembler/amd64/operands.wave',
            'assembler/amd64/directives.wave',
            'assembler/amd64/parser.wave',
            'assembler/amd64/encoder_model.wave',
            'assembler/amd64/namespace.wave',
            'assembler/amd64/layout.wave',
            'assembler/amd64/address.wave',
            'assembler/amd64/emit.wave',
            'assembler/amd64/integer.wave',
            'assembler/amd64/sse.wave',
            'assembler/amd64/branch.wave',
            'assembler/amd64/directives_encode.wave',
            'assembler/amd64/encoder.wave',
        ),
        'abi': (
            'assembler/amd64/src/ast_model.rs',
            'assembler/amd64/src/parser_wave.rs',
            'assembler/amd64/src/encoder_wave.rs',
            'assembler/amd64/src/encoder_namespace_errors.rs',
            'assembler/amd64/src/tables.rs',
            'assembler/amd64/src/encoder.rs',
            'assembler/amd64/src/encoding/address.rs',
            'assembler/amd64/src/encoding/modrm.rs',
            'assembler/amd64/src/encoding/rex.rs',
            'assembler/amd64/src/encoding/sib.rs',
            'assembler/amd64/src/lib.rs',
        ),
        'name': 'amd64',
    },
    'aarch64': {
        'sources': (
            'assembler/model.wave',
            'assembler/declarations.wave',
            'assembler/fixed_model.wave',
            'assembler/fixed_declarations.wave',
            'assembler/aarch64/registers.wave',
            'assembler/aarch64/parser.wave',
            'assembler/aarch64/operands.wave',
            'assembler/aarch64/encoding.wave',
            'assembler/aarch64/encoder.wave',
            'assembler/aarch64/entry.wave',
        ),
        'abi': (
            'assembler/aarch64/src/wave.rs',
            'assembler/aarch64/src/lib.rs',
            'assembler/aarch64/src/encoder.rs',
            'assembler/aarch64/src/parser.rs',
        ),
        'name': 'aarch64',
    },
    'riscv64': {
        'sources': (
            'assembler/model.wave',
            'assembler/declarations.wave',
            'assembler/fixed_model.wave',
            'assembler/fixed_declarations.wave',
            'assembler/riscv64/registers.wave',
            'assembler/riscv64/operands.wave',
            'assembler/riscv64/encoding.wave',
            'assembler/riscv64/encoder.wave',
            'assembler/riscv64/parser.wave',
            'assembler/riscv64/entry.wave',
        ),
        'abi': (
            'assembler/riscv64/src/wave.rs',
            'assembler/riscv64/src/lib.rs',
            'assembler/riscv64/src/encoder.rs',
            'assembler/riscv64/src/parser.rs',
        ),
        'name': 'riscv64',
    },
    'loongarch64': {
        'sources': (
            'assembler/model.wave',
            'assembler/declarations.wave',
            'assembler/fixed_model.wave',
            'assembler/fixed_declarations.wave',
            'assembler/loongarch64/model.wave',
            'assembler/loongarch64/registers.wave',
            'assembler/loongarch64/tables.wave',
            'assembler/loongarch64/operands.wave',
            'assembler/loongarch64/formats.wave',
            'assembler/loongarch64/branch.wave',
            'assembler/loongarch64/encoder.wave',
            'assembler/loongarch64/entry.wave',
        ),
        'abi': (
            'assembler/loongarch64/src/wave.rs',
            'assembler/loongarch64/src/lib.rs',
            'assembler/loongarch64/src/encoder.rs',
            'assembler/loongarch64/src/parser.rs',
        ),
        'name': 'loongarch64',
    },
}
TARGETS = (
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
    'riscv64gc-unknown-linux-gnu',
    'x86_64-pc-windows-msvc',
    'aarch64-pc-windows-msvc',
    'x86_64-apple-darwin',
    'aarch64-apple-darwin',
)


def fnv64(data):
    value = 0xcbf29ce484222325
    for octet in data:
        value = ((value ^ octet) * 0x100000001b3) & 0xffffffffffffffff
    return f"{value:016x}"


def source_bytes(root, module):
    return b"\n".join((root / name).read_bytes() for name in MODULES[module]["sources"])


def abi_bytes(root, module):
    return b"\n".join((root / name).read_bytes() for name in MODULES[module]["abi"])


def metadata(source, abi, archive, provenance, target, opt_level):
    return (f"schema=1\ntarget={target}\nwave_revision={WAVE_REVISION}\n"
            f"opt_level={opt_level}\nsource_fnv64={fnv64(source)}\n"
            f"abi_fnv64={fnv64(abi)}\narchive_fnv64={fnv64(archive)}\n"
            f"provenance_fnv64={fnv64(provenance)}\n")


def build(args):
    root = Path(__file__).resolve().parents[1]
    output = args.out_dir.resolve()
    config = MODULES[args.module]
    name = config["name"]
    source = source_bytes(root, args.module)
    abi = abi_bytes(root, args.module)
    wave_source = args.wave_source.resolve()
    if output == wave_source or output.is_relative_to(wave_source):
        raise ValueError("Wave assembler output must be outside the read-only Wave checkout")
    output.mkdir(parents=True, exist_ok=True)
    bootstrap = ensure_bootstrap(wave_source, args.bootstrap_dir or output / "bootstrap-cache",
                                 rust_toolchain=args.rust_toolchain,
                                 allow_network=args.allow_network)
    wave_target = args.target.replace("riscv64gc-", "riscv64-")
    environment = os.environ.copy()
    environment["WAVE_STD_REVISION"] = WAVE_REVISION
    with tempfile.TemporaryDirectory(prefix=".wave-assembler-", dir=output) as temporary:
        temporary = Path(temporary)
        kernel = temporary / "assembler.wave"
        kernel.write_bytes(source)
        obj = temporary / "assembler.o"
        run([bootstrap.compiler, "-O" + args.opt_level, "-C", "relocation-model=pic",
             "--std-root", bootstrap.std_root, "--target=" + wave_target, "build", kernel,
             "--freestanding", "--emit=obj", "-o", obj, "--target-dir", temporary / "build"],
            cwd=temporary, env=environment)
        archive_name = (f"whale_wave_{name}.lib" if "msvc" in args.target
                        else f"libwhale_wave_{name}.a")
        archive = temporary / archive_name
        archive_format = ("darwin" if "apple-darwin" in args.target else
                          "coff" if "msvc" in args.target else None)
        archiver = [environment.get("AR", "ar")]
        if archive_format:
            archiver.append("--format=" + archive_format)
        run([*archiver, "crsD", archive, obj])
        provenance = temporary / "provenance.json"
        provenance.write_text(json.dumps({
            "wave_revision": WAVE_REVISION, "target": args.target, "module": args.module,
            "opt_level": args.opt_level,
            "source_sha256": hashlib.sha256(source).hexdigest(),
            "abi_sha256": hashlib.sha256(abi).hexdigest(),
            "archive_sha256": digest(archive),
            "compiler_sha256": digest(bootstrap.compiler),
            "std_sha256": bootstrap.metadata["std_sha256"],
            "bootstrap_rust_version": RUST_VERSION,
            "required_llvm_major": 21,
            "bootstrap_provenance": str(bootstrap.provenance),
        }, indent=2) + "\n")
        fingerprint = temporary / f"wave-{name}.meta"
        fingerprint.write_text(metadata(source, abi, archive.read_bytes(),
                                        provenance.read_bytes(), args.target, args.opt_level))
        staged = (archive, provenance, fingerprint)
        previous = {}
        for path in staged:
            destination = output / path.name
            if destination.exists():
                backup = temporary / (path.name + ".previous")
                os.link(destination, backup)
                previous[destination] = backup
        published = []
        try:
            for path in staged:
                destination = output / path.name
                os.replace(path, destination)
                published.append(destination)
        except OSError:
            for destination in reversed(published):
                backup = previous.get(destination)
                if backup is None:
                    destination.unlink()
                else:
                    os.replace(backup, destination)
            raise
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--module", choices=tuple(MODULES), default="common")
    parser.add_argument("--wave-source", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--bootstrap-dir", type=Path)
    parser.add_argument("--rust-toolchain", default=DEFAULT_RUST_TOOLCHAIN)
    parser.add_argument("--target", choices=TARGETS, default=TARGETS[0])
    parser.add_argument("--opt-level", choices=("0", "2"), default="2")
    parser.add_argument("--allow-network", action="store_true")
    args = parser.parse_args()
    name = MODULES[args.module]["name"].upper()
    print(f"WHALE_WAVE_{name}_DIR={build(args)}")


if __name__ == "__main__":
    main()
