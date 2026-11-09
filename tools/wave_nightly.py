#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Resolve one verified Wave nightly generation for a complete Meson build."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import urllib.request
import zipfile


RELEASE_URL = "https://api.github.com/repos/wavefnd/Wave/releases/tags/nightly"
DOWNLOAD_ROOT = "https://github.com/wavefnd/Wave/releases/download/nightly/"
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_UNPACKED = 2 * 1024 * 1024 * 1024


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def publish_json(path, value):
    data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_file() and not path.is_symlink() and path.read_bytes() == data:
        return
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        pending = Path(temporary.name)
        temporary.write(data)
        temporary.flush()
        os.fsync(temporary.fileno())
    try:
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def read_json(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError(f"invalid toolchain metadata: {path}")
    return json.loads(path.read_bytes())


def host_target():
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(
        platform.machine().lower(), platform.machine().lower()
    )
    system = platform.system().lower()
    if system == "linux" and machine in ("x86_64", "aarch64", "riscv64", "loongarch64"):
        return machine + "-unknown-linux-gnu", machine + "-linux-gnu.tar.gz"
    if system == "darwin" and machine in ("x86_64", "aarch64"):
        return machine + "-apple-darwin", machine + "-apple-darwin.tar.gz"
    if system == "windows" and machine in ("x86_64", "aarch64"):
        return machine + "-pc-windows-msvc", machine + "-pc-windows-msvc.zip"
    if system == "freebsd" and machine == "x86_64":
        return "x86_64-unknown-freebsd", "x86_64-unknown-freebsd.tar.gz"
    raise ValueError(f"no Wave nightly host mapping for {system}/{machine}")


def download(url, destination, maximum):
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Whale-build",
            "Accept": "application/vnd.github+json"
            if url == RELEASE_URL
            else "application/octet-stream",
        },
    )
    with (
        urllib.request.urlopen(request, timeout=60) as response,
        destination.open("wb") as output,
    ):
        if not response.url.startswith("https://"):
            raise ValueError("Wave download redirected away from HTTPS")
        count = 0
        while chunk := response.read(65536):
            count += len(chunk)
            if count > maximum:
                raise ValueError("Wave download exceeds its size limit")
            output.write(chunk)


def asset_fields(asset):
    name = asset.get("name")
    checksum = asset.get("digest")
    if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
        raise ValueError("invalid Wave asset name")
    if not isinstance(checksum, str) or not re.fullmatch(
        r"sha256:[0-9a-f]{64}", checksum
    ):
        raise ValueError("Wave asset requires a published SHA256 digest")
    url = DOWNLOAD_ROOT + name
    if asset.get("browser_download_url") != url or type(asset.get("size")) is not int:
        raise ValueError("invalid Wave asset origin or size")
    return name, checksum[7:], url


def select_assets(release, suffix):
    if (
        not isinstance(release, dict)
        or release.get("draft") is not False
        or release.get("tag_name") != "nightly"
    ):
        raise ValueError("expected a published Wave nightly release")
    assets = release.get("assets")
    if not isinstance(assets, list) or any(not isinstance(x, dict) for x in assets):
        raise ValueError("invalid Wave nightly asset list")
    pattern = r"wave-nightly-([0-9a-f]{40})-([0-9]+)-([0-9]+)-" + re.escape(suffix)
    matches = [
        (x, re.fullmatch(pattern, x.get("name", "")))
        for x in assets
        if isinstance(x.get("name"), str)
    ]
    matches = [(x, match) for x, match in matches if match]
    if len(matches) != 1:
        raise ValueError(f"expected one Wave nightly archive for {suffix}")
    archive, match = matches[0]
    name, checksum, url = asset_fields(archive)
    metadata = [x for x in assets if x.get("name") == name + ".metadata.json"]
    if len(metadata) != 1:
        raise ValueError("Wave nightly requires matching artifact metadata")
    asset_fields(metadata[0])
    if not 0 < archive["size"] <= MAX_ARCHIVE or not 0 < metadata[0]["size"] <= 65536:
        raise ValueError("invalid Wave nightly asset size")
    source = match[1]
    if release.get("target_commitish") not in (source, "master"):
        raise ValueError("Wave nightly source does not match its archive")
    return archive, metadata[0], source, "-".join(match.groups())


def fetch_asset(asset, destination, maximum):
    _, checksum, url = asset_fields(asset)
    download(url, destination, maximum)
    if destination.stat().st_size != asset["size"] or digest(destination) != checksum:
        raise ValueError("Wave nightly asset failed size/SHA256 verification")


def unpack(archive, output):
    """Extract regular payloads only; no archive-provided links or permissions."""
    count = total = 0
    roots = set()

    def destination(name, size):
        nonlocal count, total
        path = PurePosixPath(name)
        if (
            path.is_absolute()
            or ".." in path.parts
            or not path.parts
            or "\\" in name
            or ":" in name
        ):
            raise ValueError("invalid Wave archive path")
        count += 1
        total += size
        if count > 32768 or size < 0 or total > MAX_UNPACKED:
            raise ValueError("Wave archive exceeds extraction limits")
        roots.add(path.parts[0])
        result = output.joinpath(*path.parts)
        result.parent.mkdir(parents=True, exist_ok=True)
        return result

    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as source:
            for entry in source.infolist():
                mode = entry.external_attr >> 16
                if entry.is_dir():
                    destination(entry.filename, 0).mkdir(exist_ok=True)
                elif stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                    raise ValueError("Wave archive contains a nonregular entry")
                else:
                    path = destination(entry.filename, entry.file_size)
                    with source.open(entry) as data, path.open("xb") as target:
                        shutil.copyfileobj(data, target)
                    path.chmod(0o755 if path.suffix == ".exe" else 0o644)
    else:
        with tarfile.open(archive) as source:
            for entry in source:
                if entry.isdir():
                    destination(entry.name, 0).mkdir(exist_ok=True)
                elif entry.isfile():
                    path = destination(entry.name, entry.size)
                    with source.extractfile(entry) as data, path.open("xb") as target:
                        shutil.copyfileobj(data, target)
                    path.chmod(0o755 if entry.mode & 0o111 else 0o644)
                else:
                    raise ValueError("Wave archive contains a nonregular entry")
    if len(roots) != 1:
        raise ValueError("Wave archive must have one distribution root")
    return output / roots.pop()


def inventory(root):
    files = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError("Wave toolchain contains a nonregular entry")
        if path.is_file():
            files[path.relative_to(root).as_posix()] = digest(path)
    return files


def probe(compiler, standard):
    result = subprocess.run(
        [
            str(compiler),
            "--std-root",
            str(standard),
            "print",
            "vex-capabilities",
            "--format=json",
        ],
        check=True,
        capture_output=True,
        timeout=30,
    )
    capabilities = json.loads(result.stdout)
    if capabilities.get("schema_version") != 1 or any(
        capabilities.get(name) != 1
        for name in ("package_graph", "path_remap", "source_inputs")
    ):
        raise ValueError(
            "Wave nightly does not support the required Vex package contract"
        )


def load_generation(cache, selection):
    checksum = selection.get("archive_sha256")
    if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise ValueError("invalid cached Wave generation identity")
    generation = cache / checksum
    if generation.is_symlink():
        raise ValueError("cached Wave generation must not be a link")
    for key in ("compiler", "std"):
        path = selection.get(key)
        if (
            not isinstance(path, str)
            or not re.fullmatch(r"[A-Za-z0-9_./-]+", path)
            or PurePosixPath(path).is_absolute()
            or ".." in PurePosixPath(path).parts
        ):
            raise ValueError("invalid cached compiler/std path")
    record = read_json(generation / "toolchain.json")
    if record.get("selection") != selection:
        raise ValueError("cached Wave generation metadata differs")
    root = generation / "distribution"
    observed_files = inventory(root)
    if root.is_symlink() or observed_files != record.get("files"):
        raise ValueError("cached Wave compiler/std/runtime files changed")
    compiler = root / selection["compiler"]
    standard = root / selection["std"]
    if not compiler.is_file() or not standard.is_dir():
        raise ValueError("cached Wave compiler/std is incomplete")
    probe(compiler, standard)
    # Match Vex's versioned std-tree identity while reusing verified file hashes.
    std_hash = hashlib.sha256(b"vex.std-tree.v1\0")
    prefix = selection["std"].rstrip("/") + "/"
    for name, checksum in observed_files.items():
        if name.startswith(prefix):
            relative = name[len(prefix) :].encode()
            std_hash.update(len(relative).to_bytes(8, "little"))
            std_hash.update(relative)
            std_hash.update(checksum.encode())
    return dict(
        selection,
        wavec=str(compiler),
        std=str(standard),
        compiler_sha256=digest(compiler),
        std_sha256=std_hash.hexdigest(),
    )


def prepare(cache, output, offline=False):
    cache = cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    pointer = cache / "nightly.json"
    target, suffix = host_target()
    if offline:
        selection = read_json(pointer)
        if selection.get("host") != target:
            raise ValueError("cached Wave nightly belongs to another executable host")
    else:
        with tempfile.TemporaryDirectory(prefix=".nightly-", dir=cache) as temporary:
            temporary = Path(temporary)
            release_path = temporary / "release.json"
            download(RELEASE_URL, release_path, 8 * 1024 * 1024)
            archive, metadata, source, generation = select_assets(
                read_json(release_path), suffix
            )
            name, checksum, _ = asset_fields(archive)
            metadata_path = temporary / "artifact.json"
            fetch_asset(metadata, metadata_path, 65536)
            details = read_json(metadata_path)
            if (
                not isinstance(details, dict)
                or details.get("schema_version") != 1
                or details.get("channel") != "nightly"
                or details.get("source_sha") != source
                or details.get("generation") != generation
                or details.get("archive") != name
                or details.get("sha256") != checksum
                or details.get("target") != target
            ):
                raise ValueError(
                    "Wave nightly metadata does not match the selected archive"
                )
            paths = details.get("paths", {})
            if not isinstance(paths, dict):
                raise ValueError("invalid Wave distribution paths")
            for key in ("compiler", "std"):
                path = paths.get(key)
                if (
                    not isinstance(path, str)
                    or not re.fullmatch(r"[A-Za-z0-9_./-]+", path)
                    or PurePosixPath(path).is_absolute()
                    or ".." in PurePosixPath(path).parts
                ):
                    raise ValueError("invalid Wave distribution compiler/std path")
            selection = dict(
                schema_version=1,
                host=target,
                source_revision=source,
                generation=generation,
                archive=name,
                archive_sha256=checksum,
                compiler=paths["compiler"],
                std=paths["std"],
            )
            destination = cache / checksum
            if not destination.exists():
                payload = temporary / "archive"
                fetch_asset(archive, payload, MAX_ARCHIVE)
                extracted = unpack(payload, temporary / "unpacked")
                pending = temporary / "generation"
                pending.mkdir()
                shutil.move(extracted, pending / "distribution")
                publish_json(
                    pending / "toolchain.json",
                    dict(
                        selection=selection, files=inventory(pending / "distribution")
                    ),
                )
                probe(
                    pending / "distribution" / paths["compiler"],
                    pending / "distribution" / paths["std"],
                )
                try:
                    pending.rename(destination)
                except FileExistsError:
                    pass  # Another build published this exact content generation.
    record = load_generation(cache, selection)
    publish_json(pointer, selection)
    publish_json(output, record)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    try:
        record = prepare(args.cache_dir, args.output, args.offline)
        print(f"Wave nightly {record['source_revision'][:12]} ({record['host']})")
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Wave toolchain preparation failed: {error}\n")


if __name__ == "__main__":
    main()
