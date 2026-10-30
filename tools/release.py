"""Build, validate and checksum native Whale distributions (Python 3.13+)."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

from ci_smoke import text_section

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-unknown-linux-gnu": ("elf", 62),
    "aarch64-unknown-linux-gnu": ("elf", 183),
    "x86_64-apple-darwin": ("macho", 0x01000007),
    "aarch64-apple-darwin": ("macho", 0x0100000C),
    "x86_64-pc-windows-msvc": ("pe", 0x8664),
    "aarch64-pc-windows-msvc": ("pe", 0xAA64),
}
MAX_BINARY = 256 * 1024 * 1024
MAX_ARCHIVE = 300 * 1024 * 1024


def identity(version, target, source):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("invalid release version")
    if target not in TARGETS or not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("unsupported native target or invalid source commit")
    return f"whale-{version}-{target}"


def archive_name(version, target, source):
    return identity(version, target, source) + (".zip" if "windows" in target else ".tar.gz")


def binary_name(target):
    return "whale.exe" if "windows" in target else "whale"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate_machine(data, target):
    kind, machine = TARGETS[target]
    actual = None
    if kind == "elf" and len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01":
        if struct.unpack_from("<H", data, 16)[0] in (2, 3):
            actual = struct.unpack_from("<H", data, 18)[0]
    elif kind == "macho" and len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe":
        if struct.unpack_from("<I", data, 12)[0] == 2:
            actual = struct.unpack_from("<I", data, 4)[0]
    elif kind == "pe" and len(data) >= 64 and data[:2] == b"MZ":
        offset = struct.unpack_from("<I", data, 60)[0]
        if offset <= len(data) - 24 and data[offset:offset + 4] == b"PE\0\0":
            actual = struct.unpack_from("<H", data, offset + 4)[0]
    if actual != machine:
        raise ValueError(f"binary does not match executable target {target}")


def limited_read(path, limit):
    with path.open("rb") as stream:
        data = bytearray()
        while len(data) <= limit:
            chunk = stream.read(min(65536, limit + 1 - len(data)))
            if not chunk:
                return bytes(data)
            data.extend(chunk)
    raise ValueError(f"file exceeds byte budget: {path}")


def publish_bytes(path, data):
    """Publish a complete file without replacing an earlier artifact."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
            stream.close()
            os.link(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)


def invoke(binary, args, cwd):
    environment = dict(os.environ, PATH=str(cwd / "empty-path"))
    return subprocess.run([str(binary), *map(str, args)], cwd=cwd, env=environment,
                          capture_output=True, timeout=20, check=False)


def check_socket(binary, directory, enabled):
    result = invoke(binary, ["ir", "lower", directory / "missing-feature-probe.json"], directory)
    disabled = b"requires feature 'socket-cli'" in result.stderr
    if result.returncode != (1 if enabled else 2) or disabled == enabled:
        raise ValueError("manifest socket-cli availability disagrees with binary")


def package(binary, output, version, target, source, socket=False, root=ROOT):
    stem = identity(version, target, source)
    cargo_version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if cargo_version != version:
        raise ValueError("requested version does not match Cargo.toml")
    binary = binary.resolve(strict=True)
    payload = limited_read(binary, MAX_BINARY)
    validate_machine(payload, target)
    with tempfile.TemporaryDirectory(prefix="whale package ") as directory:
        result = invoke(binary, ["--version"], Path(directory))
        if result.returncode or result.stdout.strip() != f"whale {version}".encode():
            raise ValueError("binary version does not match requested version")
        check_socket(binary, Path(directory), socket)
    manifest = {"format_version": 1, "version": version, "target": target,
                "source_commit": source, "socket_cli": socket,
                "binary": binary_name(target), "binary_sha256": digest(payload)}
    entries = {manifest["binary"]: payload,
               "LICENSE": limited_read(root / "LICENSE", 1024 * 1024),
               "README.md": limited_read(root / "README.md", 1024 * 1024),
               "manifest.json": (json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode()}
    archive = io.BytesIO()
    if "windows" in target:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED,
                             compresslevel=9) as writer:
            for name, data in sorted(entries.items()):
                info = zipfile.ZipInfo(f"{stem}/{name}", (1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = (0o100755 if name == manifest["binary"] else 0o100644) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                writer.writestr(info, data)
    else:
        with gzip.GzipFile(fileobj=archive, mode="wb", filename="", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as writer:
                for name, data in sorted(entries.items()):
                    info = tarfile.TarInfo(f"{stem}/{name}")
                    info.size = len(data)
                    info.mode = 0o755 if name == manifest["binary"] else 0o644
                    info.mtime = info.uid = info.gid = 0
                    writer.addfile(info, io.BytesIO(data))
    destination = output / archive_name(version, target, source)
    publish_bytes(destination, archive.getvalue())
    return destination


def preflight_ustar(raw):
    """Reject unsupported tar metadata before tarfile allocates its payload."""
    total = 0
    members = 0
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(raw)) as stream:
            while True:
                header = stream.read(512)
                if len(header) != 512:
                    raise ValueError("truncated release tar header")
                total += len(header)
                if header == bytes(512):
                    return
                if header[257:263] != b"ustar\0" or header[156:157] not in (b"0", b"\0"):
                    raise ValueError("release tar requires regular USTAR members")
                members += 1
                if members > 4:
                    raise ValueError("release tar has too many members")
                field = header[124:136].strip(b"\0 ")
                try:
                    size = int(field, 8)
                except ValueError as error:
                    raise ValueError("invalid release tar member size") from error
                padded = (size + 511) // 512 * 512
                if total + padded > MAX_ARCHIVE:
                    raise ValueError("release tar exceeds byte budget")
                remaining = padded
                while remaining:
                    chunk = stream.read(min(65536, remaining))
                    if not chunk:
                        raise ValueError("truncated release tar member")
                    remaining -= len(chunk)
                    total += len(chunk)
    except (EOFError, OSError) as error:
        raise ValueError("invalid compressed release tar") from error


def unpack(archive, directory, version, target, source):
    stem = identity(version, target, source)
    expected = {f"{stem}/{name}" for name in (binary_name(target), "LICENSE", "README.md", "manifest.json")}
    raw = limited_read(archive, MAX_ARCHIVE)
    entries = {}

    def keep(name, size, mode, stream):
        if name not in expected or name in entries or size < 0:
            raise ValueError("unexpected or duplicate archive member")
        limit = MAX_BINARY if name.endswith("/" + binary_name(target)) else 1024 * 1024
        if size > limit or sum(len(value[0]) for value in entries.values()) + size > MAX_ARCHIVE:
            raise ValueError("archive member exceeds byte budget")
        data = stream.read(size + 1)
        if len(data) != size:
            raise ValueError("archive member size mismatch")
        entries[name] = (data, mode)

    if archive.suffix == ".zip":
        with zipfile.ZipFile(io.BytesIO(raw)) as reader:
            for info in reader.infolist():
                if len(entries) >= 4 or (info.external_attr >> 16) & 0o170000 != 0o100000:
                    raise ValueError("archive must contain four regular files")
                with reader.open(info) as stream:
                    keep(info.filename, info.file_size, info.external_attr >> 16, stream)
    else:
        preflight_ustar(raw)
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as reader:
            for info in reader:
                if len(entries) >= 4 or not info.isfile():
                    raise ValueError("archive must contain four regular files")
                with reader.extractfile(info) as stream:
                    keep(info.name, info.size, info.mode, stream)
    if set(entries) != expected:
        raise ValueError("incomplete release archive")
    manifest = json.loads(entries[f"{stem}/manifest.json"][0])
    required = {"format_version", "version", "target", "source_commit", "socket_cli", "binary", "binary_sha256"}
    if (type(manifest) is not dict or set(manifest) != required
            or type(manifest["format_version"]) is not int or manifest["format_version"] != 1
            or type(manifest["socket_cli"]) is not bool
            or any(type(manifest[key]) is not str for key in
                   ("version", "target", "source_commit", "binary", "binary_sha256"))
            or manifest["version"] != version
            or manifest["target"] != target or manifest["source_commit"] != source
            or manifest["binary"] != binary_name(target)):
        raise ValueError("archive manifest identity mismatch")
    data, mode = entries[f"{stem}/{manifest['binary']}"]
    if digest(data) != manifest["binary_sha256"] or not mode & 0o111:
        raise ValueError("binary checksum or executable permission mismatch")
    validate_machine(data, target)
    # All names/types/lengths are checked before writing any extracted file.
    for name, (data, mode) in entries.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(mode & 0o777)
    return directory / stem / manifest["binary"], manifest


def validate(archive, version, target, source, execute=True):
    with tempfile.TemporaryDirectory(prefix="whale extracted ") as directory:
        root = Path(directory)
        binary, manifest = unpack(archive, root, version, target, source)
        if not execute:
            return manifest
        outside = root / "outside checkout"
        outside.mkdir()
        for flag in ("--help", "--version"):
            result = invoke(binary, [flag], outside)
            expected = b"whale asm" if flag == "--help" else f"whale {version}".encode()
            if result.returncode or expected not in result.stdout:
                raise ValueError("extracted CLI help/version failed")
        assembly, obj = outside / "input.asm", outside / "out.o"
        assembly.write_text("section .text\nmov eax, 42\nret\n", encoding="utf-8")
        result = invoke(binary, ["asm", "--amd64", assembly, "-o", obj], outside)
        if result.returncode or text_section(obj.read_bytes()) != bytes.fromhex("b82a000000c3"):
            raise ValueError("extracted CLI assembly failed")
        assembly.write_text("mov rax, [rbx\n", encoding="utf-8")
        before = obj.read_bytes()
        result = invoke(binary, ["asm", "--amd64", assembly, "-o", obj], outside)
        if not result.returncode or obj.read_bytes() != before:
            raise ValueError("extracted CLI malformed-input preservation failed")
        check_socket(binary, outside, manifest["socket_cli"])
        return manifest


def checksums(directory, version, source, targets, verify=False):
    if len(targets) != len(set(targets)) or not targets:
        raise ValueError("expected target set is empty or duplicated")
    names = {archive_name(version, target, source): target for target in targets}
    actual = {path.name for path in directory.iterdir()}
    expected = set(names) | ({"SHA256SUMS"} if verify else set())
    if actual != expected:
        raise ValueError("missing, unexpected or stale release artifacts")
    lines = []
    for name, target in sorted(names.items()):
        validate(directory / name, version, target, source, execute=False)
        lines.append(f"{digest(limited_read(directory / name, MAX_ARCHIVE))}  {name}\n")
    result = "".join(lines).encode()
    path = directory / "SHA256SUMS"
    if verify:
        if limited_read(path, 16384) != result:
            raise ValueError("checksum manifest mismatch")
    else:
        publish_bytes(path, result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["package", "validate", "checksums", "verify"])
    parser.add_argument("--version", required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--target", action="append", choices=TARGETS)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--socket-cli", action="store_true")
    parser.add_argument("--directory", type=Path, default=Path("dist"))
    args = parser.parse_args()
    targets = args.target or list(TARGETS)
    try:
        if args.operation in ("package", "validate"):
            if len(targets) != 1:
                parser.error("package/validate require exactly one --target")
            target = targets[0]
            if args.operation == "package":
                if args.binary is None:
                    parser.error("package requires --binary")
                package(args.binary, args.directory, args.version, target, args.source, args.socket_cli)
            else:
                validate(args.directory / archive_name(args.version, target, args.source),
                         args.version, target, args.source)
        else:
            checksums(args.directory, args.version, args.source, targets, args.operation == "verify")
    except (ValueError, OSError, subprocess.SubprocessError, tarfile.TarError,
            zipfile.BadZipFile) as error:
        parser.exit(1, f"release validation failed: {error}\n")


if __name__ == "__main__":
    main()
