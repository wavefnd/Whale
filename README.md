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
| Assembler | AMD64 plus initial AArch64, RV64I and LoongArch64 integer subsets; sections, symbols and ELF64 relocations | Available; new ISAs are initial subsets |
| IR | Typed IR construction, text parsing/printing, bounded verification, integer/control-flow, tracked stack-memory and bounded Whale-call interpretation, and scalar AST JSON lowering | Experimental |
| Object library | Object model, bounded ELF64 relocatable object reading and serialization | Available |
| Object CLI | Wrap raw input bytes in an ELF64 object with a `.text` section | Limited |
| Reverse | ELF inspection plus opt-in Wave AMD64 disassembly and CFG/xrefs | Experimental |
| Linker | Static Linux AMD64 ELF executables from objects and ordinary archives, checked relocations and explicit entry | Initial subset |

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
and sizes for every input section. The executable layout additionally accounts for
ELF headers and page-congruent RX/R/RW load segments with sparse BSS.

The [object input API](object/README.md) reads the supported bounded AMD64 ELF64
relocatable subset and preserves section, symbol and relocation semantics.
[Shared diagnostic conversions](diagnostic/README.md) retain machine-readable
phase/category and available source/object context alongside typed component errors.

ELF64 object output supports AMD64, AArch64, RV64I and LoongArch64 little-endian
identities. The bounded reader, static linker and reverse tools retain their AMD64
scope. Object metadata records machine, format, byte order and address width;
writers and consumers check their respective supported combinations. `ObjectFile::new(ObjectFormat::ELF64)` remains an AMD64 convenience
constructor; explicit identities use `ObjectFile::with_target`, and format access
is now `object.target.format`. An object file is not a linked
executable. CI runs host checks on Linux, Windows, and macOS; running Whale on a
host does not imply support for that host's native object format or instruction
set as an output target.

See the [assembly module guide](assembler/README.md) for the new ISA instruction
subsets, source syntax, relocation types and examples.

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

Enable experimental AST JSON lowering when building with:

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

The `asm` and `object` commands reject unknown options, extra input paths,
duplicate `-o` options, and missing output paths before reading input or writing
output. Specify the assembly architecture once. For a path beginning with `-`,
use a relative `./` prefix or an absolute path.


### Wrap raw bytes in an object

```sh
cargo run --locked -- object raw.bin -o raw.o
cargo run --locked -- object --help
```

This places the input bytes in an AMD64 ELF64 `.text` section with a global
`start` symbol. The input must already contain the intended machine code.

Both `asm` and `object` support `--debug-whale` with `--bytes`, `--dump-hex`,
`--dump-bin`, or `--dump-json` to inspect the first 256 bytes of the emitted
object. JSON includes the full object length and a bounded byte array.
For example:

```sh
cargo run --locked -- object raw.bin -o raw.o --debug-whale --dump-hex
```

### CLI input and output limits

`whale --version` (or `-V`) prints the package version. CLI arguments must be
UTF-8; unsupported non-UTF-8 OS paths and option names produce a diagnostic
with exit status 2 before any input is read or output is replaced.

`asm` reads at most 8 MiB by default and `object` reads at most 256 MiB.
Use `--max-input N` to set an explicit decimal byte budget for either command.
The reader consumes at most N + 1 bytes, including for growing or streamed
files. This budget is independent of assembly reservation and ELF output limits.
Oversized or invalid UTF-8 assembly input preserves an existing output file.

IR stdout writes are fallible: a closed pipe exits successfully, and other write
errors exit with status 1 and a diagnostic. Canonical IR and scalar results
retain their normal stdout format. `-o` still publishes atomically. The optional
Wave bootstrap likewise replaces its archive only after successful creation,
preserving the previous archive on an archiver failure.

### Execute scalar integer IR

Save this format 7 module as `answer.wir`:

```text
module {
  format_version 7
  semantics_version 1
  target "x86_64-whale-linux"
  datalayout { ptr=64, endian=little }

  declare @f7 "answer": whale () -> i32, linkage internal

  fn @f7 "answer"() -> i32, entry %b11 {
  %b11 "entry":
    %v42: i32 = const i32 42
    ret i32 %v42
  }

}
```

```sh
cargo run --locked -- ir run answer.wir --function @f7
```

The output is `i32 42`. The default build verifies the whole module and runs
one explicit Whale-convention function with integer/bool parameters and scalar
or void return. Use repeated `--arg` decimal literals (`true`/`false` for Bool)
and `--max-steps` to set the instruction/terminator budget (default 1,000,000).
Zero division/remainder and explicit traps return structured errors; unused
executed operations still trap. Loops evaluate phi inputs simultaneously.

This oracle supports integer constants, arithmetic, comparisons, bit casts,
checked pairs, select and control flow. It also supports tracked stack allocations, integer/Bool/data-pointer storage,
typed GEP, byte copies and fills, with initialization and capability checks.
It executes direct and signature-checked indirect Whale calls with integer/Bool,
data-pointer and function-pointer arguments/results, using one shared fuel and
memory arena. Function pointers retain their full signature and calling convention
through mov/phi/select, storage and complete or split byte copies. It rejects
floating point, external/SysV execution and general aggregate values in every
block of the entry and reachable callees, including unreachable blocks. Native
code generation remains unfinished. Legacy `undef` is a verification error. See the Rust `interpret_with_options` API for
adjustable verification limits and trap IR identities.

### Execute tracked stack memory

Save [the complete memory fixture](ir/tests/fixtures/tracked-memory-v4.wir)
as `tracked-memory.wir`. It copies an initialized pointer and then reads its
pointee. Pointer bits and capability metadata travel separately.

```sh
cargo run --locked -- ir run tracked-memory.wir --function @f0
cargo run --locked -- ir run tracked-memory.wir --function @f1
```

The first command prints `u32 42`. The second exits nonzero with
`uninitialized byte at allocation offset 0`: physical zero bytes do not count
as initialization. AST lowering emits `uninit` at each uninitialized declaration,
including each execution of a loop body, without writing a value.

The interpreter uses synthetic 64-bit addresses, never host address dereferences.
Tracked access checks provenance, generation/lifetime, bounds, permissions and
explicit alignment. One-past pointers can be formed but cannot read or write
nonempty ranges. Address arithmetic overflow traps. Integer-to-pointer casts
create raw addresses without recovered authority. Byte copies transfer both
initialization state and byte fragments of pointer metadata; nonempty overlapping
copies trap. Zero-length copy/fill succeeds without an access.

`--max-memory` sets the logical allocation-byte budget (default 64 MiB).
`InterpreterOptions::memory_limits` separately limits allocation count (16384),
pointer metadata fragments (262144), and byte/metadata work (256 Mi units).
Failures return IR-located `MemoryLimit` errors. Scalar/checked-pair reads exclude
padding from initialization checks. Each invocation owns its stack allocations and
retires them on return. Caller-owned pointers remain usable across nested calls;
returned callee-local pointers retain expired capabilities and trap on access.
Explicit `lifetime_end` and addressable scalar globals are supported as described
below. Foreign-memory adapters and native shadow metadata are future work.

### Addressable globals, explicit retirement and pointer offsets

[The executable storage fixture](ir/tests/fixtures/runtime-storage-v6.wir) covers
mutable/read-only/uninitialized globals, calls, expired aliases and byte differences:

```sh
cargo run --locked -- ir run ir/tests/fixtures/runtime-storage-v6.wir --function @f1
cargo run --locked -- ir run ir/tests/fixtures/runtime-storage-v6.wir --function @f3
cargo run --locked -- ir run ir/tests/fixtures/runtime-storage-v6.wir --function @f7
```

These print `u32 12`, `u32 42` and `i128 16`. `@f4` traps on an uninitialized
global read, `@f5` on a read-only write, and `@f6` on an expired stored pointer.

Typed IR format 6 adds explicit addressable declarations:

```text
storage @g0 "counter": u32 = value u32 10, align 4, access readwrite, linkage internal
storage @g1 "answer": u32 = value u32 42, align 4, access readonly, linkage external, link_name "public_answer"
storage @g2 "pending": u32 = uninitialized, align 4, access readwrite, linkage internal
%v0: ptr<u32> = global_addr @g0
```

`GlobalStorage`/`Module.global_storage` share GlobalId and global-name uniqueness
with existing compile-time `Global` declarations, while functions have their own
source-name namespace. `ConstRef` still refers only to compile-time declarations;
it never becomes a runtime address. Initialized storage accepts exact scalar
literal values; pointer/function-pointer cells can start explicitly uninitialized.
Read-only uninitialized declarations and aggregate storage are rejected. Floating
storage literals round-trip, but the current integer interpreter rejects their
instantiation. Integer/Bool initializers initialize their value bytes; physical
zero-fill of uninitialized storage does not initialize a value.

External storage definitions require an explicit nonempty `link_name`; internal
storage has none. Names are preserved unchanged. Function/global external-name
collisions are errors. These identities are an IR boundary; object/linker migration
remains separate. `ModuleBuilder::try_add_global_storage` and
`FunctionBuilder::try_global_addr` provide checked construction.

Each `interpret`/`interpret_values` call creates a **fresh runtime instance** and
initializes its globals once before any invocation frame. Direct/indirect/nested
calls share these allocations and permissions; frame returns never retire globals.
A second public invocation starts again from the declarations. Persistent runtime
sessions are not exposed. Returned data addresses cannot regain authority in a
fresh arena. Global initialization and stack allocations share cumulative byte,
allocation, metadata and work budgets; initialization failures identify GlobalId.

`lifetime_end %v0` is an explicit observable effect. It retires the **entire current
frame-owned stack allocation**, using a live full-bounds pointer at offset zero.
Mov/stored/bitcast aliases of that exact base are valid, including GEP back to
base; a nonzero GEP offset, restricted bounds, global or caller-owned allocation
is an invalid request. Null/raw pointers trap, and repeated end/use traps as
expired allocation. All stored/copied/GEP aliases expire together. Zero-length
copy/fill retains its no-access behavior. Return retirement tolerates an already
ended allocation. Bytes/allocation budgets are cumulative, pointer-fragment
metadata is released, and synthetic addresses and allocation slots are never
reused. Frontends choose lifetime ends explicitly; Whale does not infer lexical
scope ends or shorten an O0 lifetime.

`%v2: i128 = ptrdiff %v0, %v1` returns **lhs offset minus rhs offset in bytes**.
Both operands must have the identical data-pointer type and the same live tracked
allocation/generation. The signed i128 result represents every possible difference
of u64 offsets. It is independent of pointee stride: three u32 elements differ by
12 bytes; an array<u32,4> base and one-past differ by 16 bytes. Equal offsets yield
zero and reversed operands negate the result. `cmp`/`icmp` ordered predicates on
matching data pointers compare allocation offsets; signed/unsigned predicate
spellings agree. Null, raw, expired and cross-allocation ordering/difference trap.
Equality retains its existing allocation-identity rules, including inequality of
different live allocations. `try_lifetime_end` and `try_ptrdiff` are checked builder
APIs; all new forms, including pointer ordering, require format 6 or later.

### Lower an AST to IR

Save this minimal typed AST as `program.json`:

```json
{
  "format_version": 3,
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
schema is documented in [AST JSON Schema](ir/schema/ast-v4.schema.json) and
[the frontend AST types](ir/src/lower_ast/frontend.rs).

The envelope requires `format_version: 4` (legacy formats 2 and 3 are also accepted), `semantics_version: 1`, and
`features: []`. Unversioned inputs, unknown fields/versions/features, duplicate
JSON keys, and trailing JSON are rejected even with `--no-verify`. The raw JSON
entry point is `ir::lower_ast::interchange::decode`; its default input limit is
8 MiB, configurable with `decode_with_limit`. Integers use decimal strings;
f16/f32/f64 values use `0x` followed by exactly 4/8/16 hexadecimal storage digits.
For example, `{"Float":{"bits":32,"value":"0x80000000"}}` preserves negative zero.
Migrate format 1 inputs by setting `format_version` to 4, adding a `declarations`
array to `program`, and adding `convention` and `linkage` to every definition.
Internal functions use `"Whale"` and `"Internal"`; external functions must supply
a nonempty `link_name`. Missing versions and format 1 inputs are rejected.
Integers and floats retain their exact string encoding. AST and printed typed IR have independent format versions and a shared
semantics version; printed IR includes both version fields. Typed text IR formats 3, 4, 5, 6 and 7
can be read and verified with `ir::parse_module`.

The AST supports scalar literals, variables/constants, arithmetic, comparisons,
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

This exits nonzero with `unsupported AST format_version 99; expected 4`. It does
not create `rejected.wir`; an existing output is preserved. Lowering/type errors
likewise fail before output publication.

AST format 3 adds integer `BitAnd`, `BitOr`, `BitXor`, `Div`, `Rem`,
`ShiftLeft`, `ShiftRight`, unary `BitNot`, and Bool-only `LogicalAnd`/`LogicalOr`.
Integer operands have the same width and signedness. Division/remainder by zero
trap at runtime and report a constant-evaluation error when evaluated at compile
time; signed MIN/-1 wraps (remainder zero). Shift counts use their unsigned
N-bit pattern modulo N, including signed negative bit patterns; unsigned negative
literals are invalid. Right shift is arithmetic for signed values and logical
for unsigned values. Bool logical expressions branch and merge through phi,
evaluating left first and the right side only when needed. O0 retains the right
side's IR and full constant-expression tree even when it is skipped.

### Typed IR output and validation

Verification builds one borrowed SSA type index per function and shares verified
global constants through a read-only overlay. Dominance queries use intervals
in an iteratively constructed dominator tree; expensive CFG construction,
intersection and phi work use the module-wide `IrLimits::max_analysis_work`
budget (default 16,777,216 units). Exhaustion returns an `AnalysisWork` resource
error. `verify_module_with_stats` reports type lookups, dominance queries and
analysis work without changing IR identities or ordering.

Repeatable scaling probes can be run explicitly; they record operation counts
and timings without timing-based pass/fail assertions:

```sh
cargo test -p ir --locked --test verification_scaling -- --ignored --nocapture
cargo test -p object --locked --test relocation_grouping -- --ignored --nocapture
```

ELF serialization groups validated relocations in one input-order pass, keeping
section order and the order of signed-addend entries within each section.

Optional [original source metadata and bounded call traces](ir/SOURCE_METADATA.md)
preserve locations for stores, traps, terminators and unused constants. AST 4
`Located` wrappers retain innermost lowering errors; unannotated inputs retain
IR identities. Runtime errors expose the underlying cause through `root_cause()`
and structured source/trace details through `diagnostic()`.

The printer emits typed IR format 7 with semantics version 1. AST JSON writes
format 4. Definitions and references use explicit `@fN`, `@gN`, `%vN` and `%bN`
identities; names are quoted annotations, and each function records its entry
block. Name/string fields escape quotes, backslashes, control characters and
Unicode line separators. See the [identity fixture](ir/tests/fixtures/text-identities-v3.wir)
for repeated block names and IDs scoped to different functions.

The verifier checks cast operands, opcode categories and width direction, and
checks signed/unsigned checked arithmetic with a `tuple<T, bool>` result.
Tuple extraction requires an existing field and its exact type. These static checks are distinct from runtime conversion traps and tracked
pointer metadata.

### Reading and verifying typed text IR

These commands are available in the default build:

```sh
cargo run --locked -- ir verify ir/tests/fixtures/calls-v3.wir
cargo run --locked -- ir print ir/tests/fixtures/calls-v3.wir -o canonical.wir
```

`ir::parse_module` reads **and verifies** formats 3, 4, 5, 6 and 7. Print-parse-print
preserves explicit scoped IDs, quoted names, entry and block order, types,
exact integer/float payloads, constant-expression trees, alignment, signatures
and external link names. Whitespace and `//` comments are canonicalized. It
accepts current printer instruction forms; format 4 adds `uninit`, and format 5 adds preserved constant bitwise,
division/remainder, shift and lazy logical operators. Formats 3/4 without `undef`
remain readable and canonicalize to format 7. Extended constant syntax requires
format 5. Format 6 adds explicit storage, global addresses, lifetime_end and same-allocation
pointer ordering/differences; extended frontend operators require AST format 3. Regenerate old
`undef` lowering from its AST rather than replacing unspecified values with zero. Unknown versions, fields,
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

`FunctionBuilder` provides `try_*` operations for checked parameter/block/value
IDs and construction order. Failed operations preserve instructions, values and
terminators. Legacy convenience operations panic on invalid construction order;
use `call`, `function_addr`, and `null_function` directly for their existing
fallible call/signature checks. Public `layout_of_with_limit` clamps requested
nesting to the safe `MAX_IR_NESTING` ceiling while accepting shallow types with
larger overrides.

Direct frontend AST processing uses `AstLimits`: default depth 32, 100,000 nodes,
8 MiB total string bytes, 8 MiB encoded output and 16,777,216 source-mapping work units. `lower_o0_with_limits` and
`interchange::encode_with_limits` accept overrides; recursive work remains capped
at `MAX_AST_DEPTH` (32). Shared iterative inspection precedes lowering or pretty
JSON serialization, and owned rejected ASTs are disposed iteratively. Borrowed
ASTs stay caller-owned. The bounded encoder stops before output storage exceeds
its byte budget.

`ObjectFile::validate` checks target identity, section alignment/names, symbol
names/references/extents and relocation patch ranges without serialization.
Symbol resolution applies this contract before consuming any input object. ELF
writing additionally preflights all payloads, names, synthesized symbols, string
tables, metadata tables and alignment against the output budget before copying
payloads or names. The default remains 256 MiB; `write_with_limit` overrides it.

[Wave control flow](ir/tests/fixtures/wave-control.wave) and
[casts](ir/tests/fixtures/wave-casts.wave) have retained format 3 outputs from
Wave's current typed HIR adapter, tested for canonical print-parse-print equality (format 7 output).
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
defined runtime trap contract in the interpreter; native dispatch remains future work.
This IR support does not implement machine ABI lowering or carry identities
through object emission and linking yet.

The [executable call fixture](ir/tests/fixtures/interpreted-calls-v4.wir) covers
caller-owned pointers, nested calls, recursion and callable returns:

```sh
cargo run --locked -- ir run ir/tests/fixtures/interpreted-calls-v4.wir --function @f3
cargo run --locked -- ir run ir/tests/fixtures/interpreted-calls-v4.wir --function @f6 --arg 4
```

These print `u32 42` and `u32 4`. `InterpreterOptions::max_call_depth` limits
simultaneously active frames (default 256, including entry), and `max_frames`
limits total invocations (default 100000), including frames already returned.
`max_live_values` bounds reserved SSA slots across active frames (default 1000000).
Calls and callee instructions/terminators all consume the same `max_steps` budget.
Every frame keeps its own SSA environment; recursion does not use the host stack.
Before effects, preflight follows direct calls and function-address identities
that can flow to matching indirect calls from retained reachable bodies or
checked entry arguments. Unrelated declarations/bodies remain verified without
requiring executable support.

The scalar `interpret`/CLI boundary remains integer/Bool only. Library users can
use `interpret_values` with checked `RuntimeValue` constructors and inspect typed
results via `ty`, `scalar_value`, `pointer_address` and `function_target`. Data
addresses from `RuntimeValue::raw_pointer` never grant allocation authority, and
each run owns a distinct arena so a returned expired pointer cannot acquire a
new invocation's allocation. No CLI raw-address literal is introduced. Callable
constructors and dispatch check the complete signature, convention and target
identity. Defined callable values borrow their originating module and cannot
rebind an equal FunctionId/signature in another module; ordinary bytes cannot manufacture callable authority. Pointer-to-pointer
bitcasts permit storage byte views, while casts of actual function-pointer values
remain forbidden. The interpreter's synthetic callable bits are not a native ABI.

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

### Assembly symbol metadata and library resolution

Unannotated labels remain ELF `STT_NOTYPE`, size zero and default visibility.
Optional declarations attach explicit metadata without changing the label's
binding or inventing an external name:

```asm
section .text
global entry
symbol entry, function, 1, hidden
entry: ret
section .data
symbol payload, object, 4, default
payload: dd 42
```

`symbol NAME, KIND, SIZE, VISIBILITY` accepts `function`, `object`, or `notype`;
`SIZE` is a nonnegative integer or `equ` expression; visibility is `default` or
`hidden`. Metadata may precede or follow the label/`extern` declaration. Local
names retain the surrounding nonlocal label's scope. Unknown symbols, duplicate
metadata, invalid fields and extents outside the defining section are errors.
The Rust `AsmSymbol` and `ObjectSymbol` APIs expose kind, size and visibility.

The linker library's `SymbolTable::resolve` preserves object-local scope. Strong
definitions override weak ones in either input order; weak/weak selects the first
input, and strong/strong diagnoses both input indices. Actual relocation
references resolve before layout. Unreferenced undefined declarations impose no
requirement; a referenced undefined weak symbol has the defined value zero.
Unresolved strong references report their input object, section and offset.
`SymbolTable::references` records each selected target and whether it is weak
undefined. The static executable path uses this selection before patching relocations.

Normal ELF regression tests independently decode symbols and RELA records without
external programs. An additional GNU binutils oracle is opt-in:

```sh
cargo test -p whale --locked --test symbol_metadata readelf_fixture_confirms_symbol_metadata -- --ignored --nocapture
```

The checked-in fixture is [symbol-metadata.asm](tests/fixtures/symbol-metadata.asm).

## Release packages

Native Linux, macOS and Windows x64/ARM64 packaging, extracted-binary checks and
exact checksum manifests are documented in [the release guide](tools/RELEASING.md).
The manual workflow validates one tagged source commit across every package;
draft creation is opt-in and runs only after all checks pass.

## Development

| Path | Responsibility |
| --- | --- |
| [assembler/](assembler/) | Assembly parsing and instruction encoding |
| [ir/](ir/) | IR types, builders, lowering, verification, and printing |
| [object/](object/) | Sections, symbols, relocations, and ELF serialization |
| [linker/](linker/) | Static symbol/archive selection, relocation application and ELF executable output |
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

## AMD64 assembler limits and symbols

See [the scalar instruction reference](assembler/amd64/README.md) for LEA, Jcc aliases,
shifts, division, TEST, NOT/NEG, and immediate IMUL forms and their boundaries.

Materialized assembly output is limited to 64 MiB across all sections by default.
The public `assemble_with_limits(source, &AMD64, &AssemblyLimits { ... })` API
adjusts `max_output_bytes` and `max_relax_iterations` (default 1024 passes).
Reservations and data growth are checked before allocation; exceeding a budget
returns `AsmError`. BSS reservations use logical `u64` storage and do not consume
this byte budget. No artifact is returned until branch label layouts converge.

A `global` name requires a label definition, including a forward definition.
`global name` with `extern name` is allowed and exports an undefined external;
a local definition and `extern` remain conflicting declarations.

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

## Static Linux AMD64 linking

```sh
whale asm --amd64 start.asm -o start.o
whale asm --amd64 helper.asm -o helper.o
whale link start.o helper.o -o app --entry _start
./app
```

Inputs are the supported ELF64 little-endian AMD64 relocatable subset or ordinary
Unix archives. The entry defaults to `_start` and must be a defined global symbol
inside file-backed executable code. No libc, startup object, main wrapper, dynamic
loader, GOT or dynamic PLT is added. A statically resolved PLT32 is a direct PC32
reference. Thin archives, shared libraries and unsupported relocation kinds fail.

Archives are searched left to right and rescanned to a fixed point internally.
Only unresolved strong references and the entry demand extract members; undefined
weak references do not. Later inputs do not rescan earlier archives. GNU and BSD
long names are supported. Unused members do not introduce duplicate definitions.

`--max-input` caps the sum of input bytes and `--max-output` caps the complete
executable, both defaulting to 256 MiB. Decoded inputs share additional cumulative
limits: 256 MiB of copied payloads, 16 MiB of inspected/copied names, 65,279 raw
sections, one million raw symbols and relocations each, and 536,870,912 reader
work units. Auxiliary ELF records and unused archive members consume these limits;
physical archive bytes are counted once. Archive parsing also limits each archive
to 4096 members and 1 MiB of member names, checking names before copying them.
Selection has 10 million work units and a 4096-object cap.

Library clients can choose reader limits with `Linker::with_read_limits` and load
objects through `add_object_bytes`; all archives and byte-loaded objects share the
same remaining budget. `ObjectFile::read_with_usage` exposes successful reader
charges for other streaming consumers. Failed additions leave both the input list
and remaining budget unchanged. Already-owned objects passed to `add_object`
remain caller allocations and still undergo structural and output validation.

Loadable ranges must fit the lower 48-bit user address space, excluding its final
4 KiB guard page; optional five-level paging is not assumed. BSS remains sparse:
a 1 TiB zero-filled region can still produce a small executable when its addresses
fit this range. Alignment and logical BSS extent cannot bypass the address check.
Outputs publish atomically after validation and protect every input alias. Unix
executables receive mode 0755 without copying setuid/setgid bits. The produced
artifact targets Linux AMD64 regardless of the machine running Whale.

## Wave reverse tools

The reusable [reverse module](reverse/README.md) uses a Wave decoding/analysis core
with Rust file/ELF/CLI adapters. The initial linked core supports Linux AMD64
hosts and decodes AMD64 bytes; these are separate host and decoded-ISA contracts.
Normal Cargo builds need no Wave compiler and retain ELF inspection; disassembly
reports an explicit unavailable capability until the core is built and linked.

```sh
python3 tools/build_wave_reverse.py --wave-source /path/to/Wave --out-dir /tmp/whale-wave-reverse
WHALE_WAVE_REVERSE_DIR=/tmp/whale-wave-reverse cargo build --locked --features wave-reverse
whale disasm --amd64 app.o --json
whale reverse inspect app --json
```

The explicit bootstrap uses pinned official Wave sources, matching standard
library, Rust 1.89.0 and LLVM 21. It reads committed sources without modifying the
Wave checkout; ordinary Cargo never downloads or bootstraps a compiler. See the
module guide for byte, encoding, unsupported-instruction and build contracts.
The supported decoding scope is integer/scalar-SSE AMD64 long mode, not complete
x86 coverage or recovery of original source code.
