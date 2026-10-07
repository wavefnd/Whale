# Whale

[![Rust CI](https://github.com/wavefnd/Whale/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/wavefnd/Whale/actions/workflows/rust.yml)
[![Code quality](https://github.com/wavefnd/Whale/actions/workflows/quality.yml/badge.svg?branch=master)](https://github.com/wavefnd/Whale/actions/workflows/quality.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)

Whale brings together an intermediate representation, assembler, object-file
library, and linker infrastructure. Developed within the
[Wave ecosystem](https://github.com/wavefnd/Wave), its components are intended to
serve language implementations and compiler tooling as reusable Rust libraries.

Whale is in active, early development. APIs and IR formats are evolving; the
current capabilities are listed below.

[Getting started](#getting-started) · [Usage](#usage) · [Development](#development) · [Contributing](#contributing) · [Support](#support)

## Design goals

- **Defined behavior:** specify program behavior explicitly, including invalid
  operations, with the goal of an IR without undefined behavior.
- **Explicit O0 IR:** keep operations and required safety behavior visible in IR,
  preserving unused operations and unreachable blocks for debugging. O0 is the
  current development priority; O1 and higher optimizations are future work.
  Verification diagnoses invalid IR without removing or simplifying it.
  Explicit constant declarations retain their typed initializer expressions and
  evaluated values, including unused local declarations. Their compile-time
  evaluation does not introduce runtime arithmetic instructions.
- **Reusable components:** expose the IR, assembler, object model, and linker as
  separate crates.

These are project goals. Complete memory-safety semantics and an end-to-end
native compilation pipeline are still being developed.

## Current capabilities

| Component | Available today | Status |
| --- | --- | --- |
| Assembler | AMD64 assembly, sections, symbols, and relocations emitted as ELF64 object files | Available |
| IR | Typed IR construction, text parsing/printing, bounded verification, and scalar AST JSON lowering | Experimental |
| Object library | Object model and ELF64 relocatable object serialization | Available |
| Object CLI | Wrap raw input bytes in an ELF64 object with a `.text` section | Limited |
| Linker | Initial library infrastructure; `whale link` remains a placeholder | In development |

The IR target selector accepts only `x86_64-whale-linux`; unknown targets fail
with the supported choice, including with `--no-verify`. Its output data layout
is 64-bit little endian on every build host. Library lowering and verification
reject target/layout mismatches. The IR layout API computes checked sizes,
field offsets, array strides, and natural/allocation alignments for this target.
It does not implement aggregate ABI passing or native code generation.

BSS reservations retain a logical `zero_fill` count instead of allocating their
zero bytes. Object section memory size is `data.len() + zero_fill`; non-BSS
sections require zero `zero_fill`. ELF output uses checked field conversions and
layout arithmetic and rejects extended section numbering. Its default output
budget is 256 MiB; library clients can override it with `write_with_limit`.
The linker layout API returns `Result` and records separate file/memory positions
and sizes for every input section. It does not yet emit executable segments.

The current emitted object target is AMD64 ELF64. Object metadata records the machine,
format, byte order, and address width; writers and linker inputs reject unsupported
combinations. `ObjectFile::new(ObjectFormat::ELF64)` remains an AMD64 convenience
constructor; explicit identities use `ObjectFile::with_target`, and format access
is now `object.target.format`. An object file is not a linked
executable. CI runs host checks on Linux, Windows, and macOS; running Whale on a
host does not imply support for that host's native object format or instruction
set as an output target.

## Getting started

You need Git and **Rust 1.86.0 or newer**, including Cargo. Stable Rust is
recommended for development.

```sh
git clone https://github.com/wavefnd/Whale.git
cd Whale
cargo build --release --locked
```

The executable is written to `target/release/whale`, or
`target/release/whale.exe` on Windows. The examples below use `cargo run` so that
installing Whale on your `PATH` is optional.

Enable the experimental IR command when building with:

```sh
cargo build --release --locked --features socket-cli
```

## Usage

### Assemble an AMD64 object

Save the following as `example.asm`:

```asm
section .text
global answer

answer:
    mov eax, 42
    ret
```

```sh
cargo run --release --locked -- asm --amd64 example.asm -o example.o
```

This produces a relocatable ELF64 object. Assembly is implemented within Whale;
no external assembler is needed.

### Lower an AST to IR

Save this minimal typed AST as `program.json`:

```json
{
  "format_version": 2,
  "semantics_version": 1,
  "features": [],
  "program": {
    "globals": [],
    "functions": [
      {
        "name": "answer",
        "parameters": [],
        "return_type": {
          "Int": {
            "bits": 32,
            "signed": true
          }
        },
        "body": [
          {
            "Return": {
              "Lit": {
                "Int": {
                  "bits": 32,
                  "signed": true,
                  "value": "42"
                }
              }
            }
          }
        ],
        "convention": "Whale",
        "linkage": "Internal",
        "link_name": null
      }
    ],
    "declarations": []
  }
}
```

```sh
cargo run --release --locked --features socket-cli -- ir lower program.json -o program.wir
```

The command lowers and verifies the module, then writes textual IR to
`program.wir`. Omit `-o program.wir` to print it to standard output. The input
schema is documented in [AST JSON Schema](ir/schema/ast-v2.schema.json) and
[the frontend AST types](ir/src/lower_ast/frontend.rs).

The envelope requires `format_version: 2`, `semantics_version: 1`, and
`features: []`. Unversioned inputs, unknown fields/versions/features, duplicate
JSON keys, and trailing JSON are rejected even with `--no-verify`. The raw JSON
entry point is `ir::lower_ast::interchange::decode`; its default input limit is
8 MiB, configurable with `decode_with_limit`. Integers use decimal strings;
f16/f32/f64 values use `0x` followed by exactly 4/8/16 hexadecimal storage digits.
For example, `{"Float":{"bits":32,"value":"0x80000000"}}` preserves negative zero.
Migrate format 1 inputs by setting `format_version` to 2, adding a `declarations`
array to `program`, and adding `convention` and `linkage` to every definition.
Internal functions use `"Whale"` and `"Internal"`; external functions must supply
a nonempty `link_name`. Missing versions and format 1 inputs are rejected.
Integers and floats retain their exact string encoding. AST and printed typed IR have independent format versions and a shared
semantics version; printed IR includes both version fields. Typed text IR format 3
can be read and verified with `ir::parse_module`.

The AST supports scalar literals, variables/constants, add/sub/mul, comparisons,
assignment, return, if/while, break/continue, function declarations, direct calls,
and typed function pointers with indirect calls. Aggregate expressions, variadic
signatures, and SysV64 aggregate signatures are not supported. Unsupported forms fail explicitly.
If the binary lacks `socket-cli`, `whale ir lower` exits with status 2 and prints the
feature-enabled recovery command shown above.

For a complete invalid input, save the following as `invalid.json`:

```json
{
  "format_version": 99,
  "semantics_version": 1,
  "features": [],
  "program": {
    "declarations": [],
    "globals": [],
    "functions": []
  }
}
```

```sh
cargo run --locked --features socket-cli -- ir lower invalid.json -o rejected.wir
```

This exits nonzero with `unsupported AST format_version 99; expected 2`. It does
not create `rejected.wir`; an existing output is preserved. Lowering/type errors
likewise fail before output publication.

### Typed IR output and validation

The printer emits typed IR format 3 with semantics version 1. AST JSON remains
format 2. Definitions and references use explicit `@fN`, `@gN`, `%vN` and `%bN`
identities; names are quoted annotations, and each function records its entry
block. Name/string fields escape quotes, backslashes, control characters and
Unicode line separators. See the [identity fixture](ir/tests/fixtures/text-identities-v3.wir)
for repeated block names and IDs scoped to different functions.

The verifier checks cast operands, opcode categories and width direction, and
checks signed/unsigned checked arithmetic with a `tuple<T, bool>` result.
Tuple extraction requires an existing field and its exact type. These checks do
not implement runtime conversion traps or pointer metadata.

### Reading and verifying typed text IR

These commands are available in the default build:

```sh
cargo run --locked -- ir verify ir/tests/fixtures/calls-v3.wir
cargo run --locked -- ir print ir/tests/fixtures/calls-v3.wir -o canonical.wir
```

`ir::parse_module` reads **and verifies** format 3 text. Print-parse-print
preserves explicit scoped IDs, quoted names, entry and block order, types,
exact integer/float payloads, constant-expression trees, alignment, signatures
and external link names. Whitespace and `//` comments are canonicalized. It
accepts all current printer instruction forms, including legacy `undef`; runtime
initialization tracking remains separate work. Unknown versions, fields,
opcodes, escapes, trailing input and inconsistent type annotations are errors.
Diagnostics include a byte offset and one-based Unicode-scalar line/column;
semantic errors are attached to the containing function/global when available.

`IrLimits` configures input bytes, tokens, traversal nodes, type depth and
constant-expression depth. Defaults are 8 MiB, 1,000,000 tokens/nodes, and depth
128 (root depth 0). Nesting can be raised up to `MAX_IR_NESTING` (256), or lowered.
Use `parse_module_with_limits`, `verify_module_with_limits`,
`ConstExpr::evaluate_with_limits`, `validate_signature_with_limits` or
`ModuleBuilder::declare_function_with_limits` at the appropriate boundary.
Iterative preflight precedes recursive type helpers; constant evaluation uses
an explicit work stack. Limit failures return structured errors. Budgets bound
traversal, not elapsed time or allocations outside these APIs. Borrowed Rust
inputs remain caller-owned; arbitrary unverified trees still have Rust's usual
recursive clone/drop behavior. Checked declaration rejects and disposes its
owned oversized signature iteratively.

[Wave control flow](ir/tests/fixtures/wave-control.wave) and
[casts](ir/tests/fixtures/wave-casts.wave) have retained format 3 outputs from
Wave's current typed HIR adapter, tested for exact print-parse-print equality.
Text parsing does not provide IR execution or native code generation. Format 2
printed text requires explicit migration; it is not silently accepted. New
syntax or semantics requires an appropriate version change, with unknown
versions rejected.

### Typed function calls

The [complete call input](ir/tests/fixtures/ast-v2-calls.json) lowers a local
function, a stored function pointer, an indirect call, and a SysV64 external
call. Its [printed IR](ir/tests/fixtures/calls-v3.wir) is checked by a regression test.

```sh
cargo run --locked --features socket-cli -- ir lower ir/tests/fixtures/ast-v2-calls.json
```

Declarations carry `FunctionId`, parameter/result types, calling convention,
linkage, and explicit external link names. Calls resolve by function identity or
by a `fnptr<signature>` SSA value. The verifier checks arity, argument/result
types, conventions, and indirect-callee dominance. Callee expressions evaluate
before arguments, which evaluate left to right. Void calls have no result;
unused nonvoid results remain in O0 IR. Null or invalid indirect targets have a
defined trap contract; runtime checks await the interpreter/native backend.
This IR support does not implement machine ABI lowering or carry identities
through object emission and linking yet.

### Wrap raw bytes in an object

For an existing raw binary file:

```sh
cargo run --release --locked -- object code.bin -o code.o
```

This places the input bytes in an ELF64 `.text` section and defines a global
`start` symbol at offset zero. It does not compile textual IR or disassemble
existing object files.

### Optional Wave ELF record implementation

The default Rust path needs no Wave compiler. An explicit build can use Wave for
ELF64 header, section, symbol and RELA record serialization. Layout, validation,
allocation and symbol resolution remain in Rust. This bootstrap supports a Linux
x86_64 host building a Linux x86_64 Whale binary; it does not add an output target.

With Rust, LLVM 21 development libraries, a C linker and `ar` installed:

```sh
git clone https://github.com/wavefnd/Wave.git /tmp/whale-wave-bootstrap
git -C /tmp/whale-wave-bootstrap checkout --detach 8a465e30aeea4b817d925cdd0e8d08c1bb029c9a
python3 tools/build_wave_elf.py --wave-source /tmp/whale-wave-bootstrap --out-dir /tmp/whale-wave-elf
WHALE_WAVE_ELF_DIR=/tmp/whale-wave-elf cargo build --locked --all-features
WHALE_WAVE_ELF_DIR=/tmp/whale-wave-elf cargo test --locked --workspace --all-features
```

The script verifies the source revision and tracked modifications, builds the
bootstrap compiler with its lockfile, then compiles `object/wave/elf_records.wave`
with its LLVM backend. The Wave object is linked statically; the resulting Whale
needs no `wavec` at runtime. An invalid requested archive/host is a build error,
not an automatic fallback. Unset `WHALE_WAVE_ELF_DIR` to build the Rust path.
`object::formats::elf::WAVE_ELF_ENABLED` reports the selected build path.

The ABI uses u64 field arrays and caller-owned output buffers with explicit
counts/capacity. No allocator ownership crosses the boundary. The Wave routine
rejects unsupported records, short buffers and field overflow before writing.
Dedicated tests compare complete ELF output with the Rust path. This is the
first partial Wave implementation, not a self-hosted Whale build.

## Development

| Path | Responsibility |
| --- | --- |
| [assembler/](assembler/) | Assembly parsing and instruction encoding |
| [ir/](ir/) | IR types, builders, lowering, verification, and printing |
| [object/](object/) | Sections, symbols, relocations, and ELF serialization |
| [linker/](linker/) | Symbol resolution and layout infrastructure |
| [src/](src/) | Command-line interface |
| [tests/](tests/) | CLI integration tests |
| [tools/](tools/) | CI helpers and standalone CLI smoke checks |

Run the workspace checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
```

[GitHub Actions](https://github.com/wavefnd/Whale/actions) also covers native host
configurations, the minimum supported Rust version, optimized tests, rustdoc,
coverage, and scheduled maintenance. The checked-in
[workflows](.github/workflows/) contain the commands used by CI.

## Contributing

Contributions to correctness, diagnostics, tests, and toolchain capabilities are
welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, validation, and
signed-off commits.

- Find a bounded task in [good first issues](https://github.com/wavefnd/Whale/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22).
- Follow planned work in the [toolchain backlog](https://github.com/wavefnd/Whale/issues/12).
- [Report a bug or propose a feature](https://github.com/wavefnd/Whale/issues/new/choose).
- See [MAINTAINERS](MAINTAINERS) for review contacts and the
  [Code of Conduct](CODE_OF_CONDUCT.md) for community expectations.

The repository's AI-use policy is recorded in [ai.txt](ai.txt).

## Support

Support development through [Open Collective](https://opencollective.com/wave-lang)
or [GitHub Sponsors](https://github.com/sponsors/LunaStev).

## License

Whale is licensed under the [Mozilla Public License 2.0](LICENSE).
See [COPYRIGHT](COPYRIGHT) and [NOTICE](NOTICE) for attribution and notices.
