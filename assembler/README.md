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

`Assembler::object_target()` identifies native output. The separate
`assembler-object` crate converts assembly metadata into checked ELF64 objects;
it validates relocation namespaces against that identity. The CLI composes these
pieces through `toolchain::Registry` in `src/builtins.rs`.

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

All new outputs are little-endian ELF64 ET_REL. These are explicitly bounded
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
MOV immediates must fit one wide-move instruction; use explicit MOVZ/MOVK for
larger constants. Loads/stores use a base plus immediate, with checked scaling.

RV64I emits no compressed instructions, M/A/F/D/CSR extensions, relaxation hints
or multi-instruction `li`/`call`/`tail` pseudos. LUI/AUIPC take a raw unsigned
20-bit upper field. Use explicit JAL/JALR for calls; `%hi`/`%lo`/`%pcrel_hi`
address construction is not implemented. The object has soft-float ABI flags and
RVC disabled. LoongArch64 uses ELF object ABI v1 with LP64S flags (`0x41`).

Registers use A64 `x0..x30`/`w0..w30`, SP/ZR and FP/LR aliases; RV `x0..x31`
and standard ABI aliases; LoongArch `$r0..$r31` and ABI aliases. LoongArch also
accepts the names without `$`. Numeric control-transfer operands are **byte
displacements from the current instruction**. JIRL takes a numeric byte offset
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
Whale static linker and Wave reverse core continue to accept their documented
AMD64 subset. Passing another machine to them fails explicitly; serializing an
object does not enable a native IR backend or static linker for that machine.

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
