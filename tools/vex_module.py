#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Link Vex's verified Wave results to Whale's existing Cargo data bindings."""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

from wave_modules import MODULES, ROOT, fnv64, input_hashes, inputs
from wave_nightly import digest, publish_json, read_json


def environment(toolchain):
    return dict(os.environ, VEX_WAVEC=toolchain["wavec"], VEX_WAVE_STD=toolchain["std"])


def command(vex, build, state):
    return [
        str(vex),
        "--manifest-path",
        str(ROOT / "vex.ws"),
        "--build-dir",
        str(build),
        "--state-dir",
        str(state),
    ]


def vex_identity(vex):
    version = subprocess.check_output([str(vex), "--version"], text=True).strip()
    match = re.fullmatch(r"vex (\d+)\.(\d+)\.(\d+)(?:-[A-Za-z0-9.-]+)?", version)
    if not match or tuple(map(int, match.groups())) < (0, 0, 3):
        raise ValueError("Vex v0.0.3-beta or newer is required")
    return version, digest(vex.resolve())


@contextmanager
def generation_lease(build, generation):
    """Vex v0.0.3-beta's documented generation lease (MPL-2.0).

    Adapted from wavefnd/Vex tools/meson_consumer.py at v0.0.3-beta.
    """
    if (
        generation.parent != build / "target/.vex-run"
        or not generation.name
        or any(c not in "0123456789-" for c in generation.name)
    ):
        raise ValueError("invalid Vex artifact generation")
    lock = build / ".vex/run-locks" / (generation.name + ".lock")
    if lock.is_symlink() or not lock.is_file():
        raise ValueError("missing Vex generation lease")
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.CreateFileW.argtypes = [
            wintypes.LPCWSTR,
            wintypes.DWORD,
            wintypes.DWORD,
            wintypes.LPVOID,
            wintypes.DWORD,
            wintypes.DWORD,
            wintypes.HANDLE,
        ]
        kernel.CreateFileW.restype = wintypes.HANDLE
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        deadline = time.monotonic() + 30
        while True:
            handle = kernel.CreateFileW(
                str(lock), 0x80000000, 1, None, 3, 0x00200000, None
            )
            if handle != wintypes.HANDLE(-1).value:
                break
            code = ctypes.get_last_error()
            if code != 32 or time.monotonic() >= deadline:
                raise OSError(code, "cannot lease Vex artifact generation")
            time.sleep(0.04)
        try:
            yield
        finally:
            kernel.CloseHandle(handle)
    else:
        import fcntl

        descriptor = os.open(lock, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_SH)
            yield
        finally:
            os.close(descriptor)


def publish(output, files):
    """Stage the entire binding set before replacing any prior good file."""
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".vex-module-", dir=output) as temporary:
        temporary = Path(temporary)
        changed = []
        for name, data in files.items():
            if Path(name).name != name or name in (".", ".."):
                raise ValueError("module output names must be plain filenames")
            destination = output / name
            if destination.is_symlink():
                raise ValueError("module outputs must not be symlinks")
            if destination.exists() and destination.read_bytes() == data:
                continue
            (temporary / name).write_bytes(data)
            if destination.exists():
                shutil.copyfile(destination, temporary / (name + ".previous"))
            changed.append(name)
        published = []
        try:
            for name in changed:
                os.replace(temporary / name, output / name)
                published.append(name)
        except OSError:
            for name in reversed(published):
                previous = temporary / (name + ".previous")
                if previous.exists():
                    os.replace(previous, output / name)
                else:
                    (output / name).unlink()
            raise


def escape(path):
    text = str(path)
    if any(c in text for c in "\r\n\0"):
        raise ValueError("depfile cannot represent this path")
    return (
        text.replace("\\", "\\\\")
        .replace("$", "$$")
        .replace(" ", "\\ ")
        .replace("#", "\\#")
        .replace(":", "\\:")
    )


def build(args):
    toolchain = read_json(args.toolchain)
    version, vex_hash = vex_identity(args.vex)
    _, package, variable = MODULES[args.module]
    sources, abi = inputs(args.module)
    before = input_hashes(sources), input_hashes(abi)
    report = args.build_dir / "result.json"
    args.build_dir.mkdir(parents=True, exist_ok=True)
    wave_target = args.target.replace("riscv64gc-", "riscv64-")
    invoke = command(args.vex, args.build_dir, args.state_dir)
    invoke += [
        "-p",
        package,
        "build",
        "--lib",
        "--locked",
        "--offline",
        "--target",
        wave_target,
        "--profile",
        args.profile,
        "--artifact-manifest",
        str(report),
        "--depfile",
        str(args.build_dir / "inputs.d"),
    ]
    subprocess.run(invoke, env=environment(toolchain), check=True)
    value = read_json(report)
    if (
        value.get("schema") != "vex.artifacts"
        or value.get("schema_version") != 1
        or len(value.get("results", [])) != 1
    ):
        raise ValueError("expected one completed Vex library result")
    result = value["results"][0]
    details = result["metadata"]
    if (
        result.get("schema") != "vex.artifact-generation"
        or result.get("schema_version") != 1
        or details["artifact_kind"] != "static"
        or details["target"] != wave_target
        or details["profile"]
        != {"name": args.profile, "opt_level": 0 if args.profile == "dev" else 2}
        or details["package_target"]["name"] != package
    ):
        raise ValueError("Vex result differs from the selected module/target/profile")
    compiler = details["compiler"]
    if (
        compiler["compiler_sha256"] != toolchain["compiler_sha256"]
        or compiler["std_sha256"] != toolchain["std_sha256"]
    ):
        raise ValueError("Vex did not use the build's selected compiler/std snapshot")
    after_sources, after_abi = inputs(args.module)
    if before != (input_hashes(after_sources), input_hashes(after_abi)):
        raise ValueError("module source or Rust ABI changed during compilation")
    if vex_hash != digest(args.vex.resolve()):
        raise ValueError("Vex executable changed during compilation")
    library = package + ".lib" if "msvc" in args.target else "lib" + package + ".a"
    generation = Path(result["generation"])
    with generation_lease(args.build_dir, generation):
        primary = []
        for artifact in result["artifacts"]:
            path = Path(artifact["path"])
            if (
                not path.is_relative_to(generation)
                or ".." in path.parts
                or path.is_symlink()
                or digest(path) != artifact["sha256"]
            ):
                raise ValueError("Vex artifact failed generation/SHA256 validation")
            if artifact["role"] == "primary":
                primary.append(path)
        if len(primary) != 1:
            raise ValueError("expected one verified static library")
        archive = primary[0].read_bytes()
    source_hash, abi_hash = before
    portable = json.dumps(
        result["provenance"], sort_keys=True, separators=(",", ":")
    ).encode()
    provenance = dict(
        wave_revision=toolchain["source_revision"],
        target=args.target,
        opt_level="0" if args.profile == "dev" else "2",
        source_sha256=source_hash[1],
        abi_sha256=abi_hash[1],
        archive_sha256=hashlib.sha256(archive).hexdigest(),
        compiler_sha256=compiler["compiler_sha256"],
        std_sha256=compiler["std_sha256"],
        compiler_version=compiler["compiler_version"],
        nightly_archive_sha256=toolchain["archive_sha256"],
        vex_version=version,
        vex_sha256=vex_hash,
        vex_provenance_sha256=hashlib.sha256(portable).hexdigest(),
    )
    provenance_bytes = (
        json.dumps(provenance, sort_keys=True, indent=2) + "\n"
    ).encode()
    metadata = dict(
        schema="3",
        target=args.target,
        wave_revision=toolchain["source_revision"],
        opt_level=provenance["opt_level"],
        source_fnv64=source_hash[0],
        abi_fnv64=abi_hash[0],
        archive_fnv64=fnv64([archive]),
        provenance_fnv64=fnv64([provenance_bytes]),
    )
    files = {
        library: archive,
        "provenance.json": provenance_bytes,
        "module.meta": "".join(
            f"{key}={value}\n" for key, value in metadata.items()
        ).encode(),
    }
    if args.depfile:
        dependencies = [
            *sources,
            *abi,
            args.toolchain.resolve(),
            Path(__file__).resolve(),
            ROOT / "tools/wave_modules.py",
            *map(Path, details["inputs"]["files"]),
            *map(Path, details["inputs"]["directories"]),
        ]
        depfile = (
            escape(args.output_dir / library)
            + ": "
            + " ".join(escape(path) for path in sorted(set(dependencies)))
            + "\n"
        )
        # Meson supplies a sibling depfile in this module's output directory.
        if args.depfile.parent != args.output_dir:
            raise ValueError("module depfile must be in its output directory")
        files[args.depfile.name] = depfile.encode()
    publish(args.output_dir, files)
    print(f"{variable}={args.output_dir}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--module", choices=MODULES)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--vex", type=Path, required=True)
    parser.add_argument("--toolchain", type=Path, required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--ready", type=Path)
    parser.add_argument("--depfile", type=Path)
    parser.add_argument("--target", default="x86_64-unknown-linux-gnu")
    parser.add_argument("--profile", choices=("dev", "release"), default="dev")
    args = parser.parse_args()
    for name in ("toolchain", "build_dir", "state_dir", "output_dir", "depfile"):
        value = getattr(args, name)
        if value is not None:
            setattr(args, name, value.absolute())
    try:
        if args.prepare:
            vex_identity(args.vex)
            subprocess.run(
                command(args.vex, args.build_dir, args.state_dir)
                + ["fetch", "--locked", "--offline"],
                env=environment(read_json(args.toolchain)),
                check=True,
            )
            if args.ready:
                publish_json(
                    args.ready,
                    dict(
                        toolchain=read_json(args.toolchain),
                        vex_sha256=digest(args.vex.resolve()),
                        lock_sha256=digest(ROOT / "vex.lock"),
                    ),
                )
        else:
            if args.module is None or args.output_dir is None:
                parser.error(
                    "--module and --output-dir are required for a library build"
                )
            build(args)
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        subprocess.SubprocessError,
    ) as error:
        parser.exit(1, f"Wave module build failed: {error}\n")


if __name__ == "__main__":
    main()
