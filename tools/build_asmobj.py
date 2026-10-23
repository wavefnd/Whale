#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Compile the complete asmobj Wave module; read the pinned compiler checkout only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
from wave_bootstrap import DEFAULT_RUST_TOOLCHAIN, WAVE_REVISION, ensure_bootstrap, run, digest

SOURCES = ('model.wave', 'sections.wave', 'symbols.wave', 'relocations.wave', 'entry.wave')
TARGETS = ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
           'riscv64gc-unknown-linux-gnu', 'x86_64-pc-windows-msvc',
           'aarch64-pc-windows-msvc', 'x86_64-apple-darwin', 'aarch64-apple-darwin')

def fnv64(data):
    value = 0xcbf29ce484222325
    for octet in data:
        value = ((value ^ octet) * 0x100000001b3) & 0xffffffffffffffff
    return f'{value:016x}'

def source_bytes(root):
    return b'\n'.join((root / 'assembler/asmobj' / name).read_bytes() for name in SOURCES)

def metadata(source, archive, target):
    return (f'schema=1\ntarget={target}\nsource_fnv64={fnv64(source)}\n'
            f'archive_fnv64={fnv64(archive)}\n')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wave-source', type=Path, required=True)
    parser.add_argument('--out-dir', type=Path, required=True)
    parser.add_argument('--bootstrap-dir', type=Path)
    parser.add_argument('--rust-toolchain', default=DEFAULT_RUST_TOOLCHAIN)
    parser.add_argument('--target', choices=TARGETS, default=TARGETS[0])
    parser.add_argument('--opt-level', choices=('0', '2'), default='2')
    parser.add_argument('--allow-network', action='store_true')
    args = parser.parse_args()
    output = args.out_dir.resolve()
    if output == args.wave_source.resolve() or output.is_relative_to(args.wave_source.resolve()):
        parser.error('asmobj output must be outside the read-only Wave checkout')
    output.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[1]
    source = source_bytes(root)
    bootstrap = ensure_bootstrap(args.wave_source, args.bootstrap_dir or output / 'bootstrap-cache',
                                 rust_toolchain=args.rust_toolchain, allow_network=args.allow_network)
    # Only code generation is cross-target; this never executes a foreign object.
    wave_target = args.target.replace('riscv64gc-', 'riscv64-')
    environment = os.environ.copy()
    environment['WAVE_STD_REVISION'] = WAVE_REVISION
    with tempfile.TemporaryDirectory(prefix='.asmobj-', dir=output) as temporary:
        temporary = Path(temporary)
        kernel = temporary / 'asmobj.wave'
        kernel.write_bytes(source)
        obj = temporary / 'asmobj.o'
        run([bootstrap.compiler, '-O' + args.opt_level, '-C', 'relocation-model=pic',
             '--std-root', bootstrap.std_root, '--target=' + wave_target, 'build', kernel,
             '--freestanding', '--emit=obj', '-o', obj, '--target-dir', temporary / 'build'],
            cwd=temporary, env=environment)
        name = 'whale_asmobj.lib' if 'msvc' in args.target else 'libwhale_asmobj.a'
        archive = temporary / name
        archive_format = ('darwin' if 'apple-darwin' in args.target else
                          'coff' if 'msvc' in args.target else None)
        archiver = [environment.get('AR', 'ar')]
        if archive_format:
            # Cross-format symbol indexes must match the consuming linker.
            # LLVM ar supports these explicit formats; a host GNU ar must fail
            # rather than publish an archive it cannot index correctly.
            archiver.append('--format=' + archive_format)
        run([*archiver, 'crsD', archive, obj])
        provenance = temporary / 'provenance.json'
        provenance.write_text(json.dumps({'wave_revision': WAVE_REVISION, 'target': args.target,
            'opt_level': args.opt_level, 'source_sha256': hashlib.sha256(source).hexdigest(),
            'archive_sha256': digest(archive), 'compiler_sha256': digest(bootstrap.compiler),
            'std_sha256': bootstrap.metadata['std_sha256'],
            'bootstrap_provenance': str(bootstrap.provenance)}, indent=2) + '\n')
        fingerprint = temporary / 'asmobj.meta'
        fingerprint.write_text(metadata(source, archive.read_bytes(), args.target))
        for path in (archive, provenance, fingerprint):
            os.replace(path, output / path.name)
    print(f'WHALE_ASMOBJ_DIR={output}')

if __name__ == '__main__':
    main()
