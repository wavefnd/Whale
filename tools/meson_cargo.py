#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Internal Meson bridge for existing Rust components; never used at configure.

Cargo's target triple selects where the Whale executable runs. The independent
asm-* features select which machine-code outputs that executable can produce.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


class BuildError(ValueError):
    pass


ASSEMBLERS = ("amd64", "aarch64", "riscv64", "loongarch64")
FEATURES = {"asm-" + arch for arch in ASSEMBLERS} | {"socket-cli"}
COMPONENTS = ("diagnostic", "assembler", "backend-api", "toolchain",
              "reverse", "target-spec", "object", "linker", "ir")
TRIPLE = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]+\Z")


def validate_features(features):
    features = sorted(set(features))
    unknown = set(features) - FEATURES
    if unknown:
        raise BuildError("unknown product feature: " + min(unknown))
    return features


def cargo_environment(target_dir, asmobj_directory, profile, optimization, debug_info):
    environment = os.environ.copy()
    environment.pop("WHALE_ASMOBJ_DIR", None)
    environment.pop("CARGO_BUILD_TARGET", None)
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    environment["CARGO_INCREMENTAL"] = "0"
    prefix = "CARGO_PROFILE_" + profile.upper() + "_"
    environment[prefix + "OPT_LEVEL"] = optimization
    environment[prefix + "DEBUG"] = debug_info
    if asmobj_directory:
        environment["WHALE_ASMOBJ_DIR"] = str(asmobj_directory)
    return environment


def native_target(cargo, source, environment):
    result = subprocess.run([str(cargo), "-vV"], cwd=source, env=environment, check=True,
                            capture_output=True, text=True)
    targets = [line.removeprefix("host: ") for line in result.stdout.splitlines() if line.startswith("host: ")]
    if len(targets) != 1 or not TRIPLE.fullmatch(targets[0]):
        raise BuildError("Cargo did not report one native host triple")
    return targets[0]


def cargo_command(cargo, source, target_dir, target, profile, features, offline, jobs):
    command = [str(cargo), "build", "--locked", "--manifest-path", str(source / "Cargo.toml"),
               "--target-dir", str(target_dir), "--target", target, "--profile", profile,
               "--jobs", str(jobs), "-p", "whale", "--bin", "whale", "--no-default-features"]
    if offline:
        command.append("--offline")
    if features:
        command.extend(["--features", ",".join(features)])
    return command


def validate_host(rustc, target, system, cpu_family, source, environment):
    result = subprocess.run([str(environment.get("RUSTC", rustc)), "--print", "cfg", "--target", target],
                            cwd=source, env=environment, check=True, capture_output=True, text=True)
    values = {}
    for line in result.stdout.splitlines():
        if line.startswith(('target_arch="', 'target_os="')) and line.endswith('"'):
            name, value = line.split("=", 1)
            values[name] = value[1:-1]
    expected_os = {"darwin": "macos"}.get(system, system)
    expected_arch = {"ppc": "powerpc", "ppc64": "powerpc64"}.get(cpu_family, cpu_family)
    if values != {"target_os": expected_os, "target_arch": expected_arch}:
        raise BuildError(f"Rust host target {target!r} disagrees with Meson host {system}/{cpu_family}; "
                         "use the matching Meson cross file and cargo-host-target")


def make_escape(path):
    return str(path).replace("\\", "\\\\").replace("$", "$$").replace(" ", "\\ ").replace("#", "\\#").replace(":", "\\:")


def cargo_dependencies(path):
    """Read the first Make dependency rule, including escaped path spaces."""
    text = Path(path).read_text(encoding="utf-8").replace("\\\n", "")
    line = text.splitlines()[0] if text else ""
    start = None
    for index, character in enumerate(line):
        if character == ":" and (index + 1 == len(line) or line[index + 1].isspace()):
            backslashes = 0
            previous = index - 1
            while previous >= 0 and line[previous] == "\\":
                backslashes += 1
                previous -= 1
            if backslashes % 2 == 0:
                start = index + 1
                break
    if start is None:
        raise BuildError("Cargo produced a malformed binary dependency file")
    result = []
    token = []
    index = start
    while index < len(line):
        character = line[index]
        if character == "\\" and index + 1 < len(line) and line[index + 1] in " \\#:":
            index += 1
            token.append(line[index])
        elif character == "$" and index + 1 < len(line) and line[index + 1] == "$":
            index += 1
            token.append("$")
        elif character.isspace():
            if token:
                result.append("".join(token))
                token = []
        else:
            token.append(character)
        index += 1
    if token:
        result.append("".join(token))
    return result


def source_dependencies(source, features):
    """Explicit source-tree components, with directories tracking new files."""
    dependencies = {source, source / "Cargo.toml", source / "Cargo.lock", Path(__file__).resolve()}
    components = [*COMPONENTS, *("assembler/" + arch for arch in ASSEMBLERS if "asm-" + arch in features)]
    for component in ["", *components]:
        root = source / component
        for name in ["Cargo.toml", "build.rs"]:
            if (root / name).is_file():
                dependencies.add(root / name)
        tree = root / "src"
        if tree.is_dir():
            for directory, _, files in os.walk(tree, followlinks=False):
                directory = Path(directory)
                dependencies.add(directory)
                dependencies.update(directory / name for name in files)
    for path in [source / ".cargo", source / "rust-toolchain", source / "rust-toolchain.toml"]:
        if path.is_file():
            dependencies.add(path)
        elif path.is_dir():
            dependencies.add(path)
            dependencies.update(path.iterdir())
    dependencies.add(source / "assembler/asmobj")
    dependencies.update((source / "assembler/asmobj").glob("*.wave"))
    return dependencies


def _temporary_copy(source, destination):
    with tempfile.NamedTemporaryFile(prefix=".whale-", dir=destination.parent, delete=False) as temporary:
        path = Path(temporary.name)
    try:
        # Preserve executability, but publish a fresh mtime even when Cargo
        # reused its binary after a build-rule/directory dependency changed.
        shutil.copyfile(source, path)
        shutil.copymode(source, path)
        return path
    except BaseException:
        path.unlink(missing_ok=True)
        raise


def build(arguments):
    source = arguments.source_root.resolve()
    target_dir = arguments.target_dir.resolve()
    # Atomic replacement replaces the output entry itself, including an old
    # symlink, rather than following it into an unrelated source file.
    output = arguments.output.parent.resolve() / arguments.output.name
    depfile = arguments.depfile.parent.resolve() / arguments.depfile.name
    asmobj_directory = arguments.asmobj_dir.resolve() if arguments.asmobj_dir else None
    features = validate_features(arguments.feature)
    if target_dir in [source, source / "target"]:
        raise BuildError("Meson requires its own private Cargo target directory")
    environment = cargo_environment(target_dir, asmobj_directory, arguments.profile,
                                    arguments.opt_level, arguments.debug_info)
    target = arguments.host_target or native_target(arguments.cargo, source, environment)
    if not TRIPLE.fullmatch(target):
        raise BuildError("cargo host target must be a Rust target triple")
    validate_host(arguments.rustc, target, arguments.host_system, arguments.host_cpu_family, source, environment)
    subprocess.run(cargo_command(arguments.cargo, source, target_dir, target, arguments.profile,
                                 features, arguments.offline, arguments.jobs), cwd=source,
                   env=environment, check=True)
    directory = target_dir / target / ("debug" if arguments.profile == "dev" else "release")
    binary = directory / ("whale.exe" if arguments.host_system == "windows" else "whale")
    if not binary.is_file():
        raise BuildError("Cargo did not produce the requested Whale host executable")
    dependencies = source_dependencies(source, features)
    dependencies.update(Path(path) if Path(path).is_absolute() else source / path
                        for path in cargo_dependencies(directory / "whale.d"))
    if asmobj_directory:
        archive = "whale_asmobj.lib" if "msvc" in target else "libwhale_asmobj.a"
        dependencies.update(asmobj_directory / name for name in [archive, "asmobj.meta"])
    contents = make_escape(output) + ": " + " ".join(make_escape(path) for path in sorted(dependencies)) + "\n"
    candidate = None
    candidate_depfile = None
    try:
        candidate = _temporary_copy(binary, output)
        with tempfile.NamedTemporaryFile(prefix=".whale-deps-", dir=depfile.parent, delete=False) as temporary:
            candidate_depfile = Path(temporary.name)
            temporary.write(contents.encode("utf-8"))
        # Prepare both before publishing. Dependency output failure cannot
        # replace the last-good executable.
        os.replace(candidate_depfile, depfile)
        os.replace(candidate, output)
    finally:
        if candidate is not None:
            candidate.unlink(missing_ok=True)
        if candidate_depfile is not None:
            candidate_depfile.unlink(missing_ok=True)


def smoke(arguments):
    features = set(arguments.feature)
    binary = arguments.binary.resolve(strict=True)
    launcher = [*arguments.runner_arg, str(binary)]
    with tempfile.TemporaryDirectory(prefix="whale-meson-smoke-") as temporary:
        directory = Path(temporary)

        def invoke(values):
            return subprocess.run([*launcher, *map(str, values)], cwd=directory, capture_output=True, timeout=20)

        version = invoke(["--version"])
        if version.returncode or not version.stdout.startswith(b"whale "):
            raise BuildError("Whale host version smoke failed")
        probe = invoke(["asm", "--arch", "meson-unregistered-probe", "missing.s", "-o", "probe.o"])
        expected = ", ".join(sorted(arch for arch in ASSEMBLERS if "asm-" + arch in features)).encode()
        if probe.returncode == 0 or b"unregistered assembler" not in probe.stderr or b"available: " + expected + b"\n" not in probe.stderr:
            raise BuildError("Whale assembler selection differs from the Meson product")
        if (directory / "probe.o").exists():
            raise BuildError("selection probe unexpectedly published output")
        ir = directory / "answer.wir"
        ir.write_text('module {\n format_version 8\n semantics_version 1\n target "x86_64-whale-linux"\n'
                      ' datalayout { ptr=64, endian=little }\n declare @f0 "answer": whale () -> i32, linkage internal\n'
                      ' fn @f0 "answer"() -> i32, entry %b0 {\n %b0 "entry":\n %v0: i32 = const i32 42\n'
                      ' ret i32 %v0\n }\n}\n', encoding="utf-8")
        result = invoke(["ir", "run", ir, "--function", "@f0"])
        if result.returncode or result.stdout != b"i32 42\n":
            raise BuildError("Whale IR smoke failed")


def bootstrap(arguments):
    from wave_bootstrap import BootstrapError, ensure_bootstrap
    try:
        result = ensure_bootstrap(arguments.wave_source, arguments.cache_dir,
                                  rust_toolchain=arguments.rust_toolchain,
                                  allow_network=arguments.allow_network)
    except BootstrapError as error:
        raise BuildError(str(error)) from error
    output = arguments.output.resolve()
    candidate = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".wave-ready-", dir=output.parent, delete=False) as temporary:
            candidate = Path(temporary.name)
            temporary.write((json.dumps(result.paths(), sort_keys=True) + "\n").encode("utf-8"))
        os.replace(candidate, output)
    finally:
        if candidate is not None:
            candidate.unlink(missing_ok=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    compile_parser = commands.add_parser("build")
    compile_parser.add_argument("--cargo", required=True)
    compile_parser.add_argument("--rustc", required=True)
    for name in ["source-root", "target-dir", "output", "depfile"]:
        compile_parser.add_argument("--" + name, type=Path, required=True)
    compile_parser.add_argument("--host-target", default="")
    compile_parser.add_argument("--host-system", required=True)
    compile_parser.add_argument("--host-cpu-family", required=True)
    compile_parser.add_argument("--profile", choices=["dev", "release"], required=True)
    compile_parser.add_argument("--opt-level", choices=["0", "1", "2", "3", "s", "z"], required=True)
    compile_parser.add_argument("--debug-info", choices=["0", "2"], required=True)
    compile_parser.add_argument("--jobs", type=int, default=2)
    compile_parser.add_argument("--offline", action="store_true")
    compile_parser.add_argument("--feature", action="append", default=[])
    compile_parser.add_argument("--asmobj-dir", type=Path)
    smoke_parser = commands.add_parser("smoke")
    smoke_parser.add_argument("--binary", type=Path, required=True)
    smoke_parser.add_argument("--feature", action="append", default=[])
    smoke_parser.add_argument("--runner-arg", action="append", default=[])
    bootstrap_parser = commands.add_parser("bootstrap")
    bootstrap_parser.add_argument("--wave-source", type=Path, required=True)
    bootstrap_parser.add_argument("--cache-dir", type=Path, required=True)
    bootstrap_parser.add_argument("--rust-toolchain", required=True)
    bootstrap_parser.add_argument("--output", type=Path, required=True)
    bootstrap_parser.add_argument("--allow-network", action="store_true")
    arguments = parser.parse_args(argv)
    try:
        if arguments.command == "build":
            if arguments.jobs < 1:
                raise BuildError("Cargo jobs must be positive")
            build(arguments)
        elif arguments.command == "smoke":
            smoke(arguments)
        else:
            bootstrap(arguments)
    except (BuildError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Meson Cargo bridge: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
