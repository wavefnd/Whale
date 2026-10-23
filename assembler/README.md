# Assembly modules

The `assembler` crate owns output sections/symbols/relocations, limits and source
diagnostics. `extension::Assembler` accepts source directly, so implementations
can use their own syntax. `fixed` supplies bounded two-pass source, label and
section handling for four-byte instruction sets. AMD64 keeps its existing parser
and relaxation engine. Its complete existing [instruction and source reference](amd64/README.md)
is preserved in the AMD64 crate. Each ISA is an independent crate under this directory.

Each new ISA follows the same responsibility boundaries:

```text
src/lib.rs               Public assembler entry point and trait implementations
src/parser.rs            ISA operand syntax and register validation
src/tables.rs            Register aliases, opcode/form tables and relocation IDs
src/encoder.rs           Instruction selection, aliases and relocation resolution
src/encoding/branch.rs   Checked branch ranges and immediate bit layouts
src/encoding/            Other instruction formats and ISA-specific families
tests/encodings.rs       Independent LLVM encodings, owned by each ISA crate
```

A64 separates arithmetic and load/store encoding into `encoding/arithmetic.rs`
and `encoding/memory.rs`. RV64I and LoongArch64 put register/immediate format
packing in `encoding/formats.rs`. The shared fixed-width source front end handles
sections, labels, directives and resource limits; ISA crates own operand syntax
and machine instruction encoding. Parser and displacement-boundary unit tests
live beside their implementations. Root integration tests cover registration,
CLI behavior and the assembly-to-object boundary.

`Assembler::object_target()` identifies native output. The
[`asmobj`](asmobj/) module under this directory is implemented entirely in Wave.
Its model, section, symbol, relocation and entry files validate assembly metadata
and map relocation namespaces to the selected object identity. Rust-owned buffer
conversion lives outside the module in `toolchain/src/assembly_object.rs`; the
separate Rust `object` module serializes its result. The CLI composes assemblers
through `toolchain::Registry` in `src/builtins.rs`.

Build the complete product with [Meson](../BUILDING.md). For Cargo development,
set `WHALE_ASMOBJ_DIR` to an archive built by `tools/build_asmobj.py` before the
assembly-to-object examples and integration tests below. Without the module,
these commands return an explicit unavailable error. The initial linked module
host is Linux x86_64 GNU; assembler output architectures remain independent.

```sh
cargo run -- asm --aarch64 examples/isa/aarch64.s -o /tmp/aarch64.o
cargo run -- asm --riscv64 examples/isa/riscv64.s -o /tmp/riscv64.o
cargo run -- asm --loongarch64 examples/isa/loongarch64.s -o /tmp/loongarch64.o
```

`--arm64` aliases `--aarch64`. `--arch ID` selects a registered assembler directly
(e.g. `--arch riscv64`); extending the built-in registry does not require another
CLI switch. Architecture switches are mutually exclusive.
Existing AMD64 syntax and flags retain their behavior. `--ast` and `--token`
expose only the AMD64 parser; byte dumps, tracing, statistics, input limits and
atomic output publication work for all four native assemblers.

## Initial 64-bit instruction subsets

The default outputs are little-endian ELF64 ET_REL. These are explicitly bounded
integer subsets. ARM32, RV32, floating point, SIMD, macros and runtime IR backend
generation are outside these assembler implementations. Unsupported syntax is a
source-located error; it never becomes a guessed encoding.

| Crate | Instructions/forms |
| --- | --- |
| `aarch64` | X/W register MOV and single-MOVZ/MOVN immediates; MOVZ/MOVN/MOVK with legal 16-bit shifts; ADD/ADDS/SUB/SUBS, CMP/CMN register and imm12 forms; AND/ANDS/ORR/EOR register forms; immediate LSL/LSR/ASR; immediate-offset LDR/STR, byte/halfword transfers, LDRSW and signed-offset LDUR/STUR; ADR, B/BL, all B.cond conditions, CBZ/CBNZ, BR/BLR/RET, NOP/SVC/BRK |
| `riscv64` | RV64I ADD/SUB/SLT/SLTU/XOR/OR/AND/shift register and immediate forms, word arithmetic/shifts, LUI/AUIPC, all integer loads/stores, six conditional branches, JAL/JALR, FENCE/ECALL/EBREAK; single-instruction NOP/RET/MV/NOT/NEG/NEGW/J/JR/BEQZ/BNEZ and signed-imm12 LI |
| `loongarch64` | ADD/SUB .W/.D, SLT/SLTU, NOR/AND/OR/XOR, immediate arithmetic/logical forms, register/immediate .W/.D shifts, LU12I.W/LU32I.D/LU52I.D, integer loads/stores, B/BL/BEQ/BNE/BLT/BGE/BLTU/BGEU/BEQZ/BNEZ/JIRL, SYSCALL/BREAK and NOP/RET/MOVE/signed-imm12 LI.W/LI.D |

A64 distinguishes SP from ZR and checks operand widths. Logical immediates,
extended-register arithmetic and load/store writeback are not implemented.
MOV immediates select one MOVZ or MOVN in any legal 16-bit lane: 0/16
for W registers and 0/16/32/48 for X registers. Signed values and unsigned
spellings must fit the destination width. Constants requiring several wide
moves still require explicit MOVZ/MOVK instructions. Loads/stores use a base plus
immediate, with checked scaling.

RV64I emits no compressed instructions, M/A/F/D/CSR extensions, relaxation hints
or multi-instruction `li`/`call`/`tail` pseudos. LUI/AUIPC take a raw unsigned
20-bit upper field. Use explicit JAL/JALR for calls; `%hi`/`%lo`/`%pcrel_hi`
address construction is not implemented. The object has soft-float ABI flags and
RVC disabled. FENCE defaults to `iorw,iorw` without operands; explicit sets use
unique `i`, `o`, `r`, `w` flags or exactly `0` for an empty set, as in
`fence 0,0`, `fence rw,0` and `fence 0,rw`. LoongArch64 uses ELF object ABI v1
with LP64S flags (`0x41`).

Registers use A64 `x0..x30`/`w0..w30`, SP/ZR and FP/LR aliases; RV `x0..x31`
and standard ABI aliases; LoongArch `$r0..$r31` and ABI aliases. LoongArch also
accepts the names without `$`. Register names are case-insensitive. Numeric
suffixes use unsigned decimal digits without leading zeros, so names such as
`x+1`, `x01` and `$r01` are rejected rather than normalized to register 1.
Numeric control-transfer operands are **byte displacements from the current
instruction**. JIRL takes a numeric byte offset
relative to its register; indirect control flow does not create label fixups.

## Shared source and relocation rules

The fixed-width front end supports named labels, `.text`, `.data`, `.rodata`,
`.bss`, `.section NAME`, `.global`/`.globl`, `.extern`, `.byte`, `.hword`/`.short`,
`.word`/`.long`, `.quad`, `.zero`/`.space`, `.balign BYTES`, and `.p2align N`/
`.align N` (alignment is `2^N`). `.section` names use the common section-kind
defaults; executable instructions belong in `.text`. Comments use `//` or `;`;
RV and LoongArch additionally accept `#` comments, while A64 retains `#` immediates.
Expressions are integers or one symbol with an optional signed integer addend.
Local label names are explicit identifiers, without AMD64 parent-label scoping.

Local branches/ADR within one section resolve in the second pass. Cross-section
and explicitly declared external references retain RELA entries. Unknown symbols
and duplicate definitions are errors. `.extern` declarations cannot also be
defined in that file. `.word` and `.quad` emit architecture-correct absolute
relocations. Initial instruction relocations are:

| ISA | ELF relocation types |
| --- | --- |
| A64 | ADR_PREL_LO21 (274), CONDBR19 (280), JUMP26 (282), CALL26 (283) |
| RV64I | BRANCH (16), JAL (17) |
| LoongArch64 | B16 (64), B21 (65), B26 (66) |

BSS accepts only labels, alignment and zero reservations; it stays sparse.
Instructions require four-byte alignment in a text section. The existing materialized
output limit defaults to 64 MiB. The fixed-width front end additionally caps input
at 8 MiB, output at 256 MiB, records at 262,144, symbols/globals at 65,536 and
sections at 4,096. Alignment/reservation sizes are checked before allocation.

The object writer supports the four output identities. The bounded object reader,
Whale static linker and reverse image inspection retain their documented AMD64
subset. Instruction decoding and CFG/xrefs are currently unavailable. Serializing
another machine's object does not enable a native IR backend, decoder or static
linker for that machine.

## Verification

```sh
cargo test -p aarch64 -p riscv64 -p loongarch64 --locked
cargo test --test multi_isa --locked
# Optional independent LLVM comparison, not needed by ordinary Cargo tests:
cargo build --locked
python3 tools/check_isa_oracle.py --binary target/debug/whale \
  --llvm-mc /path/to/llvm-mc --cases 4096
# Linux with LLD and qemu-aarch64/qemu-riscv64/qemu-loongarch64 on PATH:
python3 tools/check_isa_oracle.py --binary target/debug/whale \
  --llvm-mc /path/to/llvm-mc --cases 256 --execute
```

The checked-in `tests/fixtures/encodings.tsv` files retain 186 exact instruction encodings
from LLVM 21.1.8, including register, immediate, width and branch boundaries.
Ordinary tests also cover label direction, ELF machine/ABI fields, RELA fields,
invalid ranges/register classes, namespace rejection, sparse BSS, budgets and
failure publication. The optional tool compares both those fixtures and seeded
random instructions against LLVM, plus external-control/data RELA metadata.
`--execute` additionally uses the external LLD linker for two-object calls with
nonzero branch/data addends and checks exit 42 under each ISA's QEMU user runner.

Reference contracts: [Arm A64 ISA](https://developer.arm.com/documentation/ddi0602/2026-03),
[RISC-V RV64I](https://docs.riscv.org/reference/isa/unpriv/rv64.html),
[LoongArch architecture](https://loongson.github.io/LoongArch-Documentation/LoongArch-Vol1-EN.pdf)
and [LoongArch ABI](https://github.com/loongson/la-abi-specs).

## COFF leaf objects

The Wave `asmobj` module validates metadata and maps the selected object
identity; the Rust object writer serializes it. Select `ObjectTarget::coff`
with AMD64 or AArch64, or use `whale asm --arch aarch64 --format coff source.s
-o leaf.obj`. ELF remains the default for all four assemblers.

COFF supports text/data/read-only/BSS sections, local/global symbols, long names,
absolute32/absolute64 and AMD64 PC32/call relocations. AArch64 supports B/BL,
conditional B/CBZ/CBNZ and ADR relocations with checked embedded addends. ELF
machine relocation IDs are translated explicitly at the bridge; direct ELF-only
relocations in a COFF model are rejected. Weak/hidden symbols, COMDAT, BigObj,
relocation-overflow records, GOT and signed ELF absolute32 have no initial COFF
mapping. Sized global function symbols retain a function-definition auxiliary;
COFF has no general object-symbol size field. BSS stays sparse, within COFF's
32-bit section extent. Standard section indices, alignments, output limits and
relocation overlaps are checked before publication. The reader/static linker
retain their ELF AMD64 contract; the COFF writer emits no PE executable.

The format follows the [Microsoft PE/COFF specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format).
Ordinary tests inspect independent field fixtures without external tools. Optional
`tools/check_object_oracle.py --binary target/debug/whale --format coff --readobj
/path/to/llvm-readobj --linker /path/to/lld-link` checks generated objects with
LLVM and cross-links both CPU identities. COFF object generation and cross-link
inspection are distinct from executing Whale's linked Wave module on Windows;
the initial linked module host remains Linux x86_64 GNU.

## Mach-O leaf objects

`ObjectTarget::macho64(Machine::X86_64)` and `Machine::AArch64` select bounded
little-endian MH_OBJECT serialization. CLI spelling is `--format macho`, with
`--arch amd64|aarch64` and `.o` output. There are no implicit target aliases,
Mach-O executables, new reader formats or static-linker target changes.

Headers/load commands, sections, symbols and relocations have separate modules.
The writer emits LC_SEGMENT_64, LC_SYMTAB, LC_DYSYMTAB and LC_BUILD_VERSION
(macOS 11 minimum). `.text/.rodata/.data/.bss` map to
`__TEXT,__text / __TEXT,__const / __DATA,__data / __DATA,__bss`.
Custom section names retain a leading `__` or replace their optional leading dot
with `__`; the resulting name must fit 16 bytes. Mapped collisions fail. BSS
uses virtual size without payload allocation. At most 255 sections, 24-bit symbol
indices, signed32 relocation offsets and a checked output ceiling are supported.

Local, defined-global and undefined-global symbol partitions have explicit
indices; names are preserved exactly. **No underscore is inserted automatically**:
Darwin C `entry` requires assembly `_entry`. nlist has no general size/function
kind field; these model attributes remain validated but have no such output field.
Weak/hidden symbols, scattered/subtractor/GOT/TLV/page-pair relocations, absolute32
and unsupported machines fail explicitly. No metadata is silently inferred from
an ELF or COFF numeric relocation code.

AMD64 supports unsigned absolute64, signed PC32 and branch32. PC-relative model
addends use S+A-P; the in-place Mach-O addend adds four for next-IP semantics.
ARM64 supports unsigned absolute64 and explicit MachSpecific BRANCH26 on aligned
B/BL instructions with zero branch addend. Assembler B/BL symbolic references map
through their machine namespace to this relocation. ADR, conditional branches,
page pairs and nonzero branch addends remain explicit unsupported forms.
Absolute pointer addends, including the cross-object `payload+4` fixture, are
written in-place; original object bytes are preserved on success/failure.

The binary layout and numeric IDs follow the primary
[LLVM Mach-O definitions](https://github.com/llvm/llvm-project/blob/llvmorg-21.1.0/llvm/include/llvm/BinaryFormat/MachO.h).
Optional interoperability uses `tools/check_object_oracle.py --format macho` with
LLVM inspection/ld64.lld cross-linking. The initial linked Wave module host is
Linux x86_64 GNU; cross-object inspection does not establish native macOS module
execution. Object-format tests use independent field expectations and require no
external assembler or linker. Full assembly-to-object integration links `asmobj`.
