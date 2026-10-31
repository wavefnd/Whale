# Assembly and IR performance measurements

## Actual Wave CLI pipelines

Select the native Wave archives from [BUILDING.md](../BUILDING.md#reuse-built-modules-or-run-cargo-checks), then run on an otherwise idle Linux host:

```sh
python3 tools/benchmark_cli.py --output /tmp/whale-bench/cli.json --runs 3 --warmups 1
```

This builds a release CLI with `socket-cli` and all four ISAs, locked/offline
with two Cargo jobs. Repeat `--isa amd64`, `--isa aarch64`, etc. to measure a
selected product; selections must match its `build-info` architecture list.
The driver requires Wave identities for every selected module and `socket-cli`.
Missing, reference or mixed implementations fail the run. `--skip-build --cli PATH`
measures an existing product; its hash is recorded, without claiming that the
harness built it from the reported revision or profile.

Each size runs these real commands in separate processes:

| Commands | Correctness gate outside the timed process |
| --- | --- |
| `asm --arch ID` for each selected ISA | ELF machine and exact independently specified instruction bytes |
| `ir verify`, `bytecode verify` | Successful verification of the retained input |
| `ir lower` | All unused O0 loads remain in order; execution returns `i32 42` |
| `ir fold` | Constant values and SSA IDs match; execution returns 42 and a second fold is identical |
| `ir emit` | Canonical bytecode tables, literal bits, instructions and SSA IDs; deterministic output and result 42 |
| `bytecode run` | Exact `i32 42` result |

The fixed defaults are 64 and 32,768 operations. Assembly adds zero repeatedly.
IR folding/compilation use dependent i32 add-zero chains. Lowering uses flat
unused scalar-parameter references, which fit existing AST limits and deliberately
retain O0 loads. These are scalar pipeline workloads, not a claim about all IR
or ISA performance.

Reports retain every measured sample, medians, process wall time, peak RSS,
commands, input/output SHA256, source revision/dirty state, executable/driver
hashes, host/toolchains, build flags, complete `build-info --json` and selected
archive fingerprints. GNU
`time` RSS covers the timed CLI process; validation subprocesses are excluded.
Outputs are removed before each sample. A failed command, changed input, wrong
result or nondeterministic output preserves the previous report.
`--compare PREVIOUS.json` requires matching workloads, environment and verified
results, then reports median time/RSS ratios without timing pass/fail thresholds.

## Public library phases

Issue #76 is measured with real public library calls and CLI processes. The
harness currently requires Linux, Python 3.13+, GNU `/usr/bin/time`, and the Rust
toolchain. Select native Wave archives as above; it needs no external assembler, release
credentials or network.
Ordinary Cargo builds do not run benchmarks. Run from the repository root on an
otherwise idle host; CPU contention and frequency changes affect short samples.

```sh
python3 tools/benchmark.py --output /tmp/whale-bench/baseline.json --runs 5 --warmups 1
```

The command builds `benchmark_probe` and `whale` with `--release --locked
--offline --jobs 2` and `CARGO_INCREMENTAL=0` in `target/benchmark`. Build time is
excluded from pipeline timings. Use `--target-dir` to isolate another build and
`--profile debug` only for a separate debug baseline. `--skip-build --probe PATH
--cli PATH` selects prebuilt executables; their hashes are recorded, and this
option cannot prove that they were built from the reported source revision or
with the stated profile/flags (`binaries_built_by_command` is false).
`--library-only` explicitly omits the two CLI phases and records that omission.

Each size/phase runs in a fresh process: one discarded warmup and five measured
samples by default. Warmups warm shared file/page caches; they do not turn the
samples into one long-running process. The report retains all samples and their
medians, seconds, bytes, actual commands, input/output hashes and sizes, revision,
dirty-tree state, rustc/LLVM/Cargo/Python versions, CPU/host identity, profile and
executable/source hashes and relevant Rust/profile environment flags. It retains generated inputs beside the report for inspection.

The generator is `scalar-add-zero-v1`. Small/large default to 64/32,768 operations;
`--small`/`--large` accept at most 100,000. Assembly contains a `.text` section,
global `bench`, `mov eax,42`, N `add eax,0` instructions and `ret`. Independent
expected bytes are `B8 2A 00 00 00`, N repetitions of `83 C0 00`, then `C3`.
IR has one function returning i32, constants 42 and 0, and N dependent adds of
zero. Its interpreted result must be 42 and its canonical print/parse/print
must be identical. This intentionally measures linear scalar pipelines; it does
not characterize branch-heavy verification, calls, SSE throughput or all inputs.

| Phase | Timed work |
| --- | --- |
| asm-tokenize | Source to tokens |
| asm-parse | Prebuilt tokens to AST; excludes lexing |
| asm-encode | Prebuilt AST to code/symbols/relocations; includes relaxation |
| asm-object | Metadata construction/data clone and checked ELF serialization |
| asm-library-e2e | Source to encoded object bytes in memory |
| ir-parse-verify | Public typed-text reader, which includes verification |
| ir-verify | Verification of a prebuilt module |
| ir-print | Canonical serialization of a prebuilt module |
| ir-library-e2e | Typed text through parse/verification and canonical output |
| asm-cli-e2e | Actual `whale asm`, including startup, file I/O and publication |
| ir-cli-e2e | Actual `whale ir print`, including startup, file I/O and publication |

Library phase times use `Instant`; the report also records process wall time
including preparation/startup/checking. CLI phase times are process wall time.
Correctness checks run outside library timing. GNU time `%M` is converted from
KiB to bytes: this is the **whole child-process** high-water RSS, including
fixture preparation, retained inputs, validation and runtime startup. It is not
an incremental allocator count or a phase-only memory measurement. Compare the
same phase on the same host, rather than subtracting unrelated RSS measurements.

Every preparation checks exact code bytes, independently inspects ELF `.text`,
verifies canonical IR and executes the IR. Each measured output must match its
validated fingerprint; CLI output is deleted before each run and its full SHA256
must match the prepared expected object/canonical IR. A failed command, timeout,
wrong output or changed fingerprint prevents publishing a new report. There are
no timing assertions in unit tests or CI.

To compare two revisions, run both with identical host/toolchain/profile,
workloads, warmups and run counts, then:

```sh
python3 tools/benchmark.py --output /tmp/whale-bench/current.json --runs 5 --warmups 1 --compare /tmp/whale-bench/baseline.json
```

The comparison rejects differing environments, phase sets, input/expected hashes
or correctness fingerprints. It reports median current/baseline time and RSS
ratios, with no statistical-significance claim or automatic pass/fail threshold.
For fair multi-revision work, retain separate checkout/build directories and run
correctness checks for each revision. Do not claim competitor superiority from
these measurements.

Focused checks:

```sh
cargo test --example benchmark_probe --locked --offline
python3 -m unittest discover -s tools -p test_benchmark.py -v
```
