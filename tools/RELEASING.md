# Standalone packages

The validated full Wave product runs on **Linux x86_64 GNU**. Other native
package jobs explicitly build Rust reference products; their manifests identify
that mode. Cross-produced Wave objects do not establish another executable host.

Build a clean revision with Meson, then package it with Python 3.13+:

```sh
meson setup /tmp/whale-release --buildtype=release -Dsocket-cli=true \
  -Dwave-source=/absolute/Wave -Dwave-rust-toolchain=1.89
meson compile -C /tmp/whale-release
python3 tools/release.py package --binary /tmp/whale-release/whale \
  --directory /tmp/whale-dist --version 0.1.0 --source FULL_40_DIGIT_COMMIT \
  --target x86_64-unknown-linux-gnu --socket-cli --require-wave
python3 tools/release.py validate --directory /tmp/whale-dist --version 0.1.0 \
  --source FULL_40_DIGIT_COMMIT --target x86_64-unknown-linux-gnu --require-wave
```

Omit `--socket-cli` for the default selection. `whale build-info --json` reports
actual linked module stamps, selected architectures, source revision and dirty
state, Rust version, IR/bytecode semantics and portable Wave compiler/source/ABI/
archive hashes. Source exports without Git can supply `WHALE_SOURCE_REVISION`.
`--require-wave` rejects reference/mixed products, dirty sources and unvalidated
hosts. End users need only the extracted binary, with no compiler or checkout.

Archives contain exactly the binary, LICENSE, README.md and manifest.json.
Manifest format 2 embeds the binary's provenance and SHA256; format 1 remains
readable. The producer checks target/version/source/features against the binary.
Metadata, entry order and timestamps are fixed, so equivalent inputs produce
identical archive bytes. Publication is atomic and never overwrites an earlier
artifact. Failed creation or validation preserves it.

Validation rejects unsafe members, links, duplicate names, missing files,
invalid permissions, excessive sizes and checksum/provenance mismatches before
extraction. In a fresh directory with empty PATH and no Wave/LLVM environment,
it checks selected assembler bytes, failed-output preservation and bytecode
emit/verify/run. Disabling all assemblers is supported.

For a single package, generate and recheck exact-set checksums with:

```sh
python3 tools/release.py checksums --directory /tmp/whale-dist --version 0.1.0 \
  --source FULL_40_DIGIT_COMMIT --target x86_64-unknown-linux-gnu
python3 tools/release.py verify --directory /tmp/whale-dist --version 0.1.0 \
  --source FULL_40_DIGIT_COMMIT --target x86_64-unknown-linux-gnu
```

Repeat `--target` for an explicit set; omission selects the six native package
hosts. Missing, extra or stale-version archives fail. The manual workflow requires
an exact commit, matching Cargo version and existing `vVERSION` tag. Draft
publication is separately requested, defaults off and refuses existing releases.
Implementing or testing these helpers does not publish a release.

The CI runner is non-interactive and retains process-tree ownership until output
cleanup finishes. POSIX uses a private process group; Windows uses a gated
launcher inside a kill-on-close Job before the requested command can spawn.
See [Windows Job ownership](https://learn.microsoft.com/windows/win32/procthread/job-objects).
It bounds output draining, cleans descendants even when the direct child exits
first, and joins the nonblocking reader before closing its log. Cleanup failures
remain diagnostics; captured output and command exit status are retained.
