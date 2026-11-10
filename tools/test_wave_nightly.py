# SPDX-License-Identifier: MPL-2.0
"""Nightly selection, immutable cache integrity and failure preservation."""

import copy
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import wave_nightly as helper


def checksum(data):
    return hashlib.sha256(data).hexdigest()


def distribution(source="a", payload=b"compiler"):
    generation = source * 40 + "-123-1"
    name = "wave-nightly-" + generation + "-x86_64-linux-gnu.tar.gz"
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as archive:
        for path, data in {
            "wave/wavec": payload,
            "wave/std/lib.wave": b"std",
            "wave/llvm/bin/ar": b"runtime",
        }.items():
            entry = tarfile.TarInfo(path)
            entry.size, entry.mode = len(data), 0o755
            archive.addfile(entry, io.BytesIO(data))
    payload = stream.getvalue()
    metadata = json.dumps(
        dict(
            schema_version=1,
            channel="nightly",
            source_sha=source * 40,
            generation=generation,
            archive=name,
            sha256=checksum(payload),
            target="x86_64-unknown-linux-gnu",
            paths={"compiler": "wavec", "std": "std"},
        )
    ).encode()
    entries = {name: payload, name + ".metadata.json": metadata}
    assets = [
        dict(
            name=n,
            digest="sha256:" + checksum(data),
            size=len(data),
            browser_download_url=helper.DOWNLOAD_ROOT + n,
        )
        for n, data in entries.items()
    ]
    release = dict(
        draft=False, tag_name="nightly", target_commitish=source * 40, assets=assets
    )
    return release, entries


class NightlyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.cache, self.output = (
            self.root / "cache",
            self.root / "build/toolchain.json",
        )
        self.release, self.payloads = distribution()
        self.downloads = []
        self.addCleanup(patch.stopall)
        patch.object(
            helper,
            "host_target",
            return_value=("x86_64-unknown-linux-gnu", "x86_64-linux-gnu.tar.gz"),
        ).start()
        self.probe = patch.object(helper, "probe").start()
        patch.object(helper, "download", side_effect=self.download).start()

    def download(self, url, destination, maximum):
        self.downloads.append(url)
        data = (
            json.dumps(self.release).encode()
            if url == helper.RELEASE_URL
            else self.payloads[url.removeprefix(helper.DOWNLOAD_ROOT)]
        )
        self.assertLessEqual(len(data), maximum)
        destination.write_bytes(data)

    def prepare(self, offline=False):
        return helper.prepare(self.cache, self.output, offline)

    def test_success_records_exact_generation_and_verifies_cache_before_offline_reuse(
        self,
    ):
        first = self.prepare()
        self.assertEqual(first["source_revision"], "a" * 40)
        self.assertEqual(first["compiler_sha256"], checksum(b"compiler"))
        before, mtime = self.output.read_bytes(), self.output.stat().st_mtime_ns
        self.downloads.clear()
        self.assertEqual(self.prepare(True), first)
        self.assertEqual(self.downloads, [])
        self.assertEqual(self.output.read_bytes(), before)
        self.assertEqual(self.output.stat().st_mtime_ns, mtime)

    def test_new_nightly_keeps_the_old_generation_and_updates_only_this_build_snapshot(
        self,
    ):
        first = self.prepare()
        separate = self.root / "other-build.json"
        separate.write_bytes(self.output.read_bytes())
        self.release, self.payloads = distribution("b", b"new compiler")
        second = self.prepare()
        self.assertNotEqual(first["archive_sha256"], second["archive_sha256"])
        self.assertTrue(Path(first["wavec"]).is_file())
        self.assertEqual(json.loads(separate.read_bytes()), first)
        self.assertEqual(json.loads(self.output.read_bytes()), second)

    def test_resolution_queries_the_moving_release_once_per_build(self):
        first_download = self.download

        def moving(url, destination, maximum):
            first_download(url, destination, maximum)
            if url == helper.RELEASE_URL:
                self.release = distribution("b")[0]

        with patch.object(helper, "download", side_effect=moving):
            result = self.prepare()
        self.assertEqual(result["source_revision"], "a" * 40)
        self.assertEqual(self.downloads.count(helper.RELEASE_URL), 1)

    def test_changed_compiler_std_or_runtime_is_rejected_before_cached_execution(self):
        record = self.prepare()
        distribution_root = Path(record["wavec"]).parent
        for relative in ("wavec", "std/lib.wave", "llvm/bin/ar"):
            with self.subTest(relative=relative):
                path = distribution_root / relative
                previous, report = path.read_bytes(), self.output.read_bytes()
                path.write_bytes(b"tampered")
                self.probe.reset_mock()
                try:
                    with self.assertRaisesRegex(ValueError, "files changed"):
                        self.prepare(True)
                    self.probe.assert_not_called()
                    self.assertEqual(self.output.read_bytes(), report)
                finally:
                    path.write_bytes(previous)

    def test_download_or_capability_failure_preserves_previous_pointer_and_output(self):
        self.prepare()
        before = self.output.read_bytes(), (self.cache / "nightly.json").read_bytes()
        for failure in ("download", "capabilities"):
            with self.subTest(failure=failure):
                self.release, self.payloads = distribution("b", b"new")
                if failure == "download":
                    archive = self.release["assets"][0]["name"]
                    self.payloads[archive] += b"corrupt"
                self.probe.side_effect = (
                    ValueError("unsupported capabilities")
                    if failure == "capabilities"
                    else None
                )
                with self.assertRaises(ValueError):
                    self.prepare()
                self.assertEqual(
                    (
                        self.output.read_bytes(),
                        (self.cache / "nightly.json").read_bytes(),
                    ),
                    before,
                )

    def test_first_failure_never_publishes_a_ready_toolchain(self):
        self.probe.side_effect = ValueError("unsupported compiler")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.output.exists())
        self.assertFalse((self.cache / "nightly.json").exists())

    def test_offline_requires_a_matching_prepared_host(self):
        with self.assertRaises(ValueError):
            self.prepare(True)
        self.prepare()
        with patch.object(
            helper, "host_target", return_value=("aarch64-unknown-linux-gnu", "unused")
        ):
            with self.assertRaisesRegex(ValueError, "another executable host"):
                self.prepare(True)

    def test_ambiguous_malformed_and_unverified_release_assets_fail(self):
        bad = [
            dict(self.release, draft=True),
            dict(self.release, assets=[]),
            dict(
                self.release,
                assets=[*self.release["assets"], self.release["assets"][0]],
            ),
            dict(self.release, target_commitish="b" * 40),
        ]
        changed = copy.deepcopy(self.release)
        changed["assets"][0]["digest"] = None
        bad.append(changed)
        changed = copy.deepcopy(self.release)
        changed["assets"][0]["browser_download_url"] = "https://example.com/compiler"
        bad.append(changed)
        for release in bad:
            with self.subTest(release=release), self.assertRaises(ValueError):
                helper.select_assets(release, "x86_64-linux-gnu.tar.gz")

    def test_metadata_source_and_paths_are_validated_before_archive_execution(self):
        name = self.release["assets"][1]["name"]
        original = json.loads(self.payloads[name])
        for details in (
            [],
            dict(original, source_sha="b" * 40),
            dict(original, paths={"compiler": "../outside", "std": "std"}),
        ):
            with self.subTest(details=details):
                data = json.dumps(details).encode()
                self.payloads[name] = data
                self.release["assets"][1].update(
                    digest="sha256:" + checksum(data), size=len(data)
                )
                with self.assertRaises(ValueError):
                    self.prepare()
                self.probe.assert_not_called()

    def test_cache_metadata_cannot_redirect_compiler_outside_its_generation(self):
        self.prepare()
        pointer = helper.read_json(self.cache / "nightly.json")
        pointer["compiler"] = "../outside"
        helper.publish_json(self.cache / "nightly.json", pointer)
        with self.assertRaisesRegex(ValueError, "compiler/std path"):
            self.prepare(True)

    def test_archive_traversal_links_duplicates_and_multiple_roots_are_rejected(self):
        for names, linked in (
            (["../outside"], False),
            (["wave/link"], True),
            (["wave/file", "wave/file"], False),
            (["one/file", "two/file"], False),
        ):
            with (
                self.subTest(names=names),
                tempfile.TemporaryDirectory(dir=self.root) as temporary,
            ):
                archive = Path(temporary) / "test.tar"
                with tarfile.open(archive, "w") as source:
                    for name in names:
                        entry = tarfile.TarInfo(name)
                        entry.type = tarfile.SYMTYPE if linked else tarfile.REGTYPE
                        entry.linkname = "/outside" if linked else ""
                        entry.size = 0
                        source.addfile(entry, io.BytesIO())
                with self.assertRaises((ValueError, FileExistsError)):
                    helper.unpack(archive, Path(temporary) / "output")


if __name__ == "__main__":
    unittest.main()
