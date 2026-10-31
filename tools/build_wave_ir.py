#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build the internal Wave IR module with the pinned compiler, without editing Wave."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

from wave_bootstrap import (DEFAULT_RUST_TOOLCHAIN, RUST_VERSION, WAVE_REVISION,
                            digest, ensure_bootstrap, run)


SOURCES = (
    'model.wave',
    'layout.wave',
    'lexer.wave',
    'header.wave',
    'type_parser.wave',
    'literal.wave',
    'expression_parser.wave',
    'instruction_parser.wave',
    'module_parser.wave',
    'lower_hir.wave',
    'lower_model.wave',
    'lower_output.wave',
    'lower_expression.wave',
    'lower_storage.wave',
    'lower_statement.wave',
    'lower_driver.wave',
    'builder.wave',
    'builder_namespace.wave',
    'builder_body.wave',
    'verify.wave',
    'verifier_index.wave',
    'verifier_ssa.wave',
    'printer.wave',
    'cfg.wave',
    'shape.wave',
    'source.wave',
    'memory.wave',
    'lower_limits.wave',
    'wide.wave',
    'integer.wave',
    'float_model.wave',
    'float_arithmetic.wave',
    'float_convert.wave',
    'effects.wave',
    'passes.wave',
    'fold_integer.wave',
    'constant.wave',
    'constant_dependencies.wave',
    'runtime_model.wave',
    'runtime_prepare.wave',
    'runtime_execute.wave',
    'runtime_driver.wave',
    'bytecode_model.wave',
    'bytecode_inventory.wave',
    'bytecode_loader.wave',
    'bytecode_verify_types.wave',
    'bytecode_verify_instructions.wave',
    'bytecode_verify_flow.wave',
    'bytecode_verify.wave',
    'bytecode_writer.wave',
    'bytecode_runtime.wave',
    'entry.wave',
)
ABI_SOURCES = (
    'ir/src/wave_layout.rs',
    'ir/src/interpreter/integer.rs',
    'ir/src/float/wave.rs',
    'ir/src/effects_wave.rs',
    'ir/src/pass_wave.rs',
    'ir/src/fold_integer_wave.rs',
    'ir/src/constant_wave.rs',
    'ir/src/interpreter/runtime_wave.rs',
    'ir/src/bytecode.rs',
    'ir/src/verifier/wave_constants.rs',
    'ir/src/source_wave.rs',
    'ir/src/interpreter/memory_wave.rs',
    'ir/src/lower_ast/wave_limits.rs',
    'ir/src/parser/wave_lexer.rs',
    'ir/src/parser/wave_header.rs',
    'ir/src/parser/wave_text.rs',
    'ir/src/parser/wave_type.rs',
    'ir/src/parser/wave_literal.rs',
    'ir/src/parser/wave_expression.rs',
    'ir/src/parser/wave_instruction.rs',
    'ir/src/parser/wave_module.rs',
    'ir/src/lower_ast/wave_hir.rs',
    'ir/src/lower_ast/wave_lower.rs',
    'ir/src/builder/wave.rs',
    'ir/src/builder/namespace.rs',
    'ir/src/builder/body.rs',
    'ir/src/verifier/wave_semantics.rs',
    'ir/src/verifier/wave_index.rs',
    'ir/src/verifier/wave_ssa.rs',
    'ir/src/printer_wave.rs',
    'ir/src/verifier/wave_cfg.rs',
    'ir/src/verifier/wave_shape.rs',
)
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


def source_bytes(root):
    return b"\n".join((root / "ir" / name).read_bytes() for name in SOURCES)


def abi_bytes(root):
    return b"\n".join((root / name).read_bytes() for name in ABI_SOURCES)


def metadata(source, abi, archive, provenance, target, opt_level):
    return (f"schema=1\ntarget={target}\nwave_revision={WAVE_REVISION}\n"
            f"opt_level={opt_level}\nsource_fnv64={fnv64(source)}\n"
            f"abi_fnv64={fnv64(abi)}\narchive_fnv64={fnv64(archive)}\n"
            f"provenance_fnv64={fnv64(provenance)}\n")


def build(args):
    root = Path(__file__).resolve().parents[1]
    output = args.out_dir.resolve()
    source = source_bytes(root)
    abi = abi_bytes(root)
    wave_source = args.wave_source.resolve()
    if output == wave_source or output.is_relative_to(wave_source):
        raise ValueError("Wave IR output must be outside the read-only Wave checkout")
    output.mkdir(parents=True, exist_ok=True)
    bootstrap = ensure_bootstrap(wave_source, args.bootstrap_dir or output / "bootstrap-cache",
                                 rust_toolchain=args.rust_toolchain,
                                 allow_network=args.allow_network)
    wave_target = args.target.replace("riscv64gc-", "riscv64-")
    environment = os.environ.copy()
    environment["WAVE_STD_REVISION"] = WAVE_REVISION
    with tempfile.TemporaryDirectory(prefix=".wave-ir-", dir=output) as temporary:
        temporary = Path(temporary)
        kernel = temporary / "ir.wave"
        kernel.write_bytes(source)
        obj = temporary / "ir.o"
        run([bootstrap.compiler, "-O" + args.opt_level, "-C", "relocation-model=pic",
             "--std-root", bootstrap.std_root, "--target=" + wave_target, "build", kernel,
             "--freestanding", "--emit=obj", "-o", obj, "--target-dir", temporary / "build"],
            cwd=temporary, env=environment)
        name = "whale_wave_ir.lib" if "msvc" in args.target else "libwhale_wave_ir.a"
        archive = temporary / name
        archive_format = ("darwin" if "apple-darwin" in args.target else
                          "coff" if "msvc" in args.target else None)
        archiver = [environment.get("AR", "ar")]
        if archive_format:
            archiver.append("--format=" + archive_format)
        run([*archiver, "crsD", archive, obj])
        provenance = temporary / "provenance.json"
        provenance.write_text(json.dumps({
            "wave_revision": WAVE_REVISION, "target": args.target,
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
        fingerprint = temporary / "wave-ir.meta"
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
    parser.add_argument("--wave-source", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--bootstrap-dir", type=Path)
    parser.add_argument("--rust-toolchain", default=DEFAULT_RUST_TOOLCHAIN)
    parser.add_argument("--target", choices=TARGETS, default=TARGETS[0])
    parser.add_argument("--opt-level", choices=("0", "2"), default="2")
    parser.add_argument("--allow-network", action="store_true")
    args = parser.parse_args()
    print(f"WHALE_WAVE_IR_DIR={build(args)}")


if __name__ == "__main__":
    main()
