# Bounded validation campaigns

These mutation targets call the project APIs. With the selected Wave archives,
IR/AST/assembler/bytecode algorithms execute in Wave; Rust owns the harness.
Build the archives as described in [CONTRIBUTING.md](../CONTRIBUTING.md).
Missing Wave modules fail explicitly; `rust-reference` is a separate comparison build.
A fixed seed and case count make each campaign reproducible; the normal Rust
suite also exercises every retained seed and small deterministic mutations.

```sh
cargo build --locked --features socket-cli --examples
python3 tools/fuzz_smoke.py --binary target/debug/examples/fuzz_boundaries --cases 512 --seed 1464353100
```

Targets are independently selected: `assembler`, `object`, `ir`, `ast`, `bytecode`. The runner
rejects an empty corpus or a corpus with no valid seed. Arguments cap runs at
100,000 mutations; the Python wrapper bounds each process to 60 seconds by
default, with a Linux RLIMIT_AS ceiling of 512 MiB (`--memory-mib`). Inputs and seed/replay reads are capped at 8192 bytes. Assembler output
is capped at 64 KiB and relaxation at 32 passes. IR parsing/verifying uses 4096
tokens, 8192 nodes, depth 32 and 131072 analysis work. AST lowering additionally
uses depth 16, 2048 nodes, 8192 string bytes, 65536 source-mapping work units and
64 KiB encoded output. Successful
lowering verifies, and canonical IR print-parse-print must retain its identities,
constant trees and supported metadata. Older version seeds remain useful when
the canonical writer advances its version.

Seed cases retain malformed syntax, numeric overflow, huge reservations, sparse
IR identities, unknown versions, nested aggregates, strict JSON and exact-width
float/call inputs. Mutations are byte insertion/deletion/repetition/bit flips and
boundary-token injection. Library rejections are expected outcomes; a panic is
a failure. Before every native call, one immutable current input and an atomic
JSON replay manifest are published; SIGSEGV, abort and timeout evidence therefore
survives outside Rust panic handling. On panic, the original failing bytes and
replay command are saved before at
most 256 deletion-minimization attempts or five seconds of reduction. Both the
original and minimized bytes are written under
`WHALE_FUZZ_ARTIFACT_DIR` (default `/tmp/whale-fuzz` for direct runs, or
`.ci-artifacts/fuzz` through the smoke script), with the originating seed and an
exact replay command:

```sh
target/debug/examples/fuzz_boundaries ir --replay /path/to/ir-SEED-CASE.original
```

The byte/token model for tracked memory is independent of production access and
fragment helpers. Cells track initialized bytes and abstract data/callable
identity tokens. Generated streams exercise GEP, scalar/pointer/callable
store/load, copy, set, uninit, permissions, one-past and zero-length operations.
Stored pointer and callable authority are checked across split copies, identical
byte overwrites, mismatched signatures, mixed permissions and frame retirement.
Resource regressions check allocation/byte/fragment/work failures without partial
publication; executable IR fixtures also enforce fuel and explicit retirement
when the version supporting that operation is present.

```sh
cargo test -p ir --locked --lib generated
WHALE_MEMORY_CASES=256 WHALE_MEMORY_SEED=1 cargo test -p ir --locked --lib generated_memory_campaign -- --ignored --nocapture
```

A campaign contains at most 4096 streams of 128 operations; ordinary tests use
16 streams of 64 operations and retained `.ops` files. Each stream has five
allocations, at most 96 bytes, 128 pointer/callable fragments and four million
memory work. A failed campaign deletes redundant operations, writes a minimized
`.ops` file and prints the seed/case. Replay with `WHALE_MEMORY_REPLAY=/path/...ops`
using the same ignored test. Six decimal bytes per operation encode opcode,
destination alias/buffer, destination offset, source alias or token, source
offset, and length/value. The checked-in sequences retain targeted interaction cases; newly found failures
are minimized automatically. Opcode 9 explicitly retires a selected stack
allocation: its fields select alias flags, offset, buffer/data/global/null/raw
target, owning-frame checkpoint and an unused byte. The independent model checks
selective liveness, repeated/invalid termination, caller/global ownership and
failure atomicity. Retiring data invalidates copied aliases while callable tags
in surviving storage retain their signatures and authority. Generated streams
force exact-base retirement every 32 operations; the retained explicit-lifetime
sequence covers rejected ends and zero-length operations after retirement.

## Versioned AST schema agreement

Use a separate environment to install the pinned validator and its dependencies:

```sh
python3 -m venv /tmp/whale-schema
/tmp/whale-schema/bin/python -m pip install -r tools/requirements-schema.txt
cargo build --locked --all-features --example schema_probe
/tmp/whale-schema/bin/python tools/schema_agreement.py --binary target/debug/examples/schema_probe
```

Windows uses the virtual environment's `Scripts/python.exe`. The script discovers
all `ir/schema/ast-v*.schema.json` files by their declared version, validates the
schemas and applies identical acceptance/rejection fixtures to the real decoder
probe. It includes declarations, direct/indirect calls, optional/null link names,
unit enum encodings, exact-width floats and version-gated extensions (including
located wrappers for schemas supporting them). Numeric widths and decimal/hex
spelling are wire syntax and must agree; numeric ranges, expression types,
foreign ABI restrictions and required external link names remain semantic checks
in lowering/verification. For located annotations, cross-field source ranges and
duplicate original node IDs are semantic checks in the bounded decoder guard;
the agreement matrix labels those intentional structural-schema differences. Raw duplicate/escaped JSON keys are tested only by the
decoder because JSON Schema sees an already-parsed object. Every accepted
lowering result must verify. Schema references are local; validation never fetches
remote schemas. The normal Rust numeric syntax regression runs without Python.

The object target feeds arbitrary ELF bytes through the bounded ET_REL reader
and writer, and `WOM1` model bytes through public-model validation and semantic
write/read/write comparisons. Retained seeds include NASM metadata, malformed
relocations/tables, sparse BSS and conceptual NOBITS offsets beyond EOF. The
reader bounds sections, symbols, relocations, payload, copied/inspected strings
and work; BSS stays sparse.

Bytecode inputs use the bounded Wave loader/verifier with 64 KiB decoded bytes,
4096 records, depth 32, 131072 work and 4 MiB workspace. Independent O0/O2 bytecode
checkers also retain original inputs and literal replay argument arrays.

CI runs all five targets on pull requests. `fuzz.yml` gives them separate jobs,
with weekly scheduled campaigns of 50,000 cases each,
300-second process deadlines and 512 MiB address-space limits. Logs, originals,
minimized inputs and exact replay commands are retained as artifacts.
