#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Exercise real Cargo selection and guards using actual pinned Wave archives."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def check(command, environment, accepted, reason=None):
    result = subprocess.run(command, env=environment, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=180)
    if (result.returncode == 0) != accepted or (reason and reason not in result.stdout):
        raise AssertionError(f"unexpected guard result: {' '.join(command)}\n{result.stdout[-12000:]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ir-dir', type=Path, required=True)
    parser.add_argument('--assembler-dir', type=Path, required=True)
    parser.add_argument('--target-dir', type=Path, required=True)
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    base = os.environ.copy()
    for name in tuple(base):
        if name.startswith('WHALE_WAVE_') or name == 'WHALE_ASMOBJ_DIR':
            base.pop(name)
    arguments.target_dir.mkdir(parents=True, exist_ok=True)
    base['CARGO_TARGET_DIR'] = str(arguments.target_dir.resolve())
    base['CARGO_INCREMENTAL'] = '0'
    base.pop('CARGO_FEATURE_RUST_REFERENCE', None)
    cargo = shutil.which('cargo')
    if not cargo:
        raise RuntimeError('Cargo is required for actual module selection checks')
    checks = 0
    for package, directory, variable, library, metadata in [
        ('ir', arguments.ir_dir, 'WHALE_WAVE_IR_DIR', 'libwhale_wave_ir.a', 'wave-ir.meta'),
        ('assembler', arguments.assembler_dir, 'WHALE_WAVE_ASSEMBLER_DIR',
         'libwhale_wave_assembler.a', 'wave-assembler.meta'),
    ]:
        command = [cargo, 'check', '--manifest-path', str(root / 'Cargo.toml'), '-p', package,
                   '--no-default-features', '--locked', '--offline', '--jobs', '2']
        check(command, base, False, 'requires its Wave module')
        checks += 1
        reference = [cargo, 'test', '--manifest-path', str(root / 'Cargo.toml'), '-p', package,
                     '--no-default-features', '--features', 'rust-reference', '--lib',
                     'selected_implementation_has_its_own_compiled_identity',
                     '--locked', '--offline', '--jobs', '2']
        check(reference, base, True)
        checks += 1
        native = {**base, variable: str(directory.resolve())}
        check(command, native, True)
        checks += 1
        # The copies retain real compiler/archive bytes; only the advertised
        # failure is changed. Never mutate the user's valid artifact directory.
        with tempfile.TemporaryDirectory(prefix='guards-', dir=arguments.target_dir) as temporary:
            copied = Path(temporary).resolve()
            for filename in [library, metadata, 'provenance.json']:
                shutil.copyfile(directory / filename, copied / filename)
            environment = {**base, variable: str(copied)}
            archive = copied / library
            original = archive.read_bytes()
            archive.write_bytes(bytes([original[0] ^ 1]) + original[1:])
            check(command, environment, False, 'corrupt')
            checks += 1
            archive.write_bytes(original)
            text = (copied / metadata).read_text()
            lines = [line if not line.startswith('source_fnv64=')
                     else 'source_fnv64=0000000000000000' for line in text.splitlines()]
            (copied / metadata).write_text('\n'.join(lines) + '\n')
            check(command, environment, False, 'stale')
            checks += 1
            (copied / metadata).write_text(text)
            archive.unlink()
            check(command, environment, False, 'missing')
            checks += 1
    print(f'{checks} actual Cargo module-selection and artifact-guard checks passed')


if __name__ == '__main__':
    main()
