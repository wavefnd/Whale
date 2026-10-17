# AMD64 scalar assembler forms

These forms run in AMD64 long mode. `assemble` and `assemble_with_limits` use
Whale's parser and encoder directly; neither uses an external assembler.
Instructions and directives end at a newline or EOF. A label may precede its
instruction on the same line. CR, LF, and CRLF are supported. Quoted multiline
strings preserve their source bytes. Token positions index original UTF-8 bytes;
display columns count Unicode scalars, with a tab counting as one column.

```asm
section .text
global example
example:
    mov spl, al
    lea r8, [r9 + rcx*4 + 32]
    test r8, r9
    jne .different
    shl r8, 1
    shr r8, cl
    sar r8, 255
    not r8
    neg r8
    imul r8, r9, -128
.different:
    cqo
    idiv r9
    ret
```

`SPL`, `BPL`, `SIL`, and `DIL` require a REX prefix even with no extension bits.
`AH`, `CH`, `DH`, and `BH` cannot coexist with any REX prefix, including extended
address registers. These checks apply to existing moves and ALU operations, and
the new scalar forms.

| Family | Supported forms and boundaries |
| --- | --- |
| LEA | `lea r32/r64, [address]`, using 64-bit base/index registers and signed 32-bit displacement. Symbol-only addresses support the rel/abs rules below; 8/16-bit destinations are rejected. Operand size does not select 32-bit address size. |
| TEST | `test reg, reg`, matching 8/16/32/64-bit widths; flags are updated without changing the operands. |
| NOT / NEG | A single register at any scalar width; memory forms are rejected. |
| SHL / SAL / SHR / SAR | Register destination at any scalar width, with count `1`, unsigned immediate `0..255`, or `CL`; memory forms and other count registers are rejected. |
| DIV / IDIV | One register divisor at any scalar width; memory forms are rejected. |
| CWD / CDQ / CQO | No operands; sign extend AX into DX:AX, EAX into EDX:EAX, and RAX into RDX:RAX respectively. |
| IMUL | Existing `imul r16/r32/r64, reg/mem` and new `imul reg, reg, immediate`, with matching widths. The immediate uses signed imm8 when possible, otherwise imm16 for r16 or imm32 for r32/r64. The r64 immediate must fit signed 32 bits because hardware sign extends it. r16/r32 also accept the full unsigned bit pattern for their immediate width. Three-operand memory forms remain unsupported. |

DIV/IDIV consume AX for an 8-bit divisor (quotient AL, remainder AH); DX:AX,
EDX:EAX, or RDX:RAX for wider divisors (quotient AX/EAX/RAX, remainder
DX/EDX/RDX). Divide by zero and quotient overflow raise architectural divide
errors. CWD/CDQ/CQO prepare the signed dividend; unsigned division normally needs
a zero high half. Raw machine instructions do not enforce Whale IR's separate
wrap, trap, or pointer contracts.

Raw shifts use the AMD64 hardware count masks (five bits for 8/16/32-bit operands,
six for 64-bit operands). The assembler preserves the encoded immediate and does
not substitute this machine policy for Whale's IR count-modulo-width semantics.

All integer condition codes have short and near forms. Same-section branches
relax at the inclusive signed-byte displacement limits; cross-section or external
branches carry `Branch32` relocations with a `-4` addend.

| Condition | Mnemonics and aliases |
| --- | --- |
| Overflow / no overflow | JO / JNO |
| Unsigned below / above-or-equal | JB, JC, JNAE / JAE, JNB, JNC |
| Equal / not equal | JE, JZ / JNE, JNZ |
| Unsigned below-or-equal / above | JBE, JNA / JA, JNBE |
| Sign / no sign | JS / JNS |
| Parity / no parity | JP, JPE / JNP, JPO |
| Signed less / greater-or-equal | JL, JNGE / JGE, JNL |
| Signed less-or-equal / greater | JLE, JNG / JG, JNLE |

The exact-byte fixtures in `tests/amd64_scalar_forms.rs` are independent opcode
and addressing expectations. The 13 accepted LEA fixtures were additionally
checked with `objdump -D -b binary -m i386:x86-64 -M intel`; all disassembled to
the intended destination and effective address. Neither objdump nor NASM is a
normal test dependency.

## Literals, byte strings and memory widths

Decimal, hexadecimal (`0x`) and binary (`0b`) literals preserve every bit up to
`18446744073709551615`. Unary minus supports `-9223372036854775808`. Constant
expressions retain the signed-minimum through unsigned-maximum range; narrowing
occurs at the selected register, directive, displacement or relocation boundary.
For example `dq 0xffffffffffffffff` and `mov rax, 18446744073709551615` emit the
complete 64-bit pattern. Expression intermediates use checked i128 arithmetic,
so `0xffffffffffffffff + 1 - 1` and wide subtraction can cancel before the
final numeric value is checked against that range. Equ symbol expressions
check the final value after resolving their constant. ELF addends remain
checked signed 64-bit values; address displacements keep their existing ranges.

Quoted `db` strings encode ordinary source text as UTF-8. Supported escapes are
`\"`, `\\`, `\n`, `\r`, `\t`, `\0`, and `\xHH` with exactly two hexadecimal
digits. A byte escape inserts one raw byte, including `\xFF`; it is not converted
to a Unicode character. Unknown or truncated escapes are errors. Literal
backslashes must now be doubled: `db "C:\\tmp"`. Empty strings remain valid and
physical multiline strings preserve their original newline bytes.

Memory operands accept `byte`, `word`, `dword` or `qword` before `[address]`.
Register/memory forms infer width when omitted and reject conflicting explicit
widths. Memory immediates require the annotation:

```asm
mov byte [rax], 255
mov word [r8], 4660
mov dword [rsp+8], 4294967295
mov qword [r13], -2147483648
add qword [r12], 127
mov rax, [rcx*4]
mov rax, [r9*8+16]
```

Initial memory-immediate forms are MOV and ADD/SUB/AND/OR/XOR/CMP. Qword forms
require a signed 32-bit immediate because hardware sign extends it; other forms
accept their complete signed/unsigned immediate-width ranges. Arithmetic chooses
an imm8 encoding only when its sign extension preserves the value. Symbol-only
memory remains RIP-relative, and relocation addends include any trailing
immediate bytes. A scaled first register denotes an index without a base;
optional displacement and a subsequent base are supported, while invalid scales,
RSP as index and excess registers are rejected.

## Section identity, alignment and fixture coverage

`section NAME, text|rodata|data|bss` assigns an explicit supported kind to a
custom name. `text` is executable/read-only, `rodata` is initialized read-only,
`data` is initialized writable, and `bss` is writable zero-fill storage. The four
standard names retain their corresponding defaults, and other unqualified names
default to data. Reentering an existing section without attributes preserves its
kind; contradictory attributes, unknown kinds, and incompatible redeclarations
are errors. Custom BSS names use sparse storage and reject nonzero initializers,
relocations and instructions exactly as `.bss` does.

`align N` requires one positive power-of-two constant, including an `equ` value.
It raises the section's ELF alignment and pads the current logical offset. Text
uses `0x90` NOP bytes, initialized data uses zero bytes, and BSS only increases
logical zero-fill size. Code defaults to alignment 16, other kinds to 1. Output
budgets are checked before materializing padding. Alignment participates in label
layout and branch relaxation. A branch that grows after being shortened stays
near, preventing alignment-driven oscillation; configured pass limits still
return an explicit error instead of stale displacements.

```asm
section .startup, text
entry: nop
align 32
aligned: ret
section .constants, rodata
db "hello", 0
align 16
section .workspace, bss
resb 1099511627777
align 64
```

`test/fixtures.tsv` classifies every historical `.asm` fixture with an outcome,
expected diagnostic (for negative/future cases), and purpose. All 15 current
fixtures are supported-positive and run under Cargo/CI. The runner compares the
manifest with recursive file discovery, so missing, empty, duplicated and
unclassified entries fail. A future-syntax entry must name its feature issue;
negative/future entries must actually reject with their declared diagnostic.

## Additional AMD64 scalar forms

MOVZX/MOVSX accept an 8-bit source with a 16/32/64-bit destination, or a 16-bit
source with a 32/64-bit destination. MOVSXD accepts a 32-bit source and a 64-bit
destination. Sources can be registers or explicitly annotated byte/word/dword
memory. Destination width chooses `66`/REX.W independently of the source width;
AH/CH/DH/BH are rejected when any required REX prefix would change their meaning.

SETcc accepts one byte register or byte memory destination (memory width can be
omitted because the instruction intrinsically writes one byte). CMOVcc initially
accepts matching 16/32/64-bit register pairs. Both families use every condition and
alias in the Jcc table above, replacing the `j` prefix with `set` or `cmov`:
`setz`, `setne`, `setb`, `cmovge`, `cmovle`, and the other listed aliases. These
instructions do not implement source-language conditional evaluation or native
IR Bool/Select lowering.

```asm
movzx eax, byte [rbx]
movsx r8, r9b
movsxd rax, dword [r13]
setne spl
setz byte [r12]
cmovge r8, r9
call r8
jmp qword [r12]
push -128
adc r8, r9
sbb eax, ebx
```

Near indirect CALL/JMP accept a 64-bit register or qword memory target. Long mode
supplies their default 64-bit operand size without REX.W; address extension bits
still select extended registers. Symbol-only memory uses the existing RIP-relative
data-reference relocation, while direct labels retain their branch/PLT intent
and relaxation. These raw instructions do not grant tracked pointer authority.

Immediate PUSH supports AMD64's default 64-bit stack operation with signed imm8
or signed imm32, choosing imm8 when possible. The hardware sign extends the
immediate; positive values above `2147483647` are rejected rather than truncated.
An immediate 16-bit override and memory PUSH/POP are outside this initial subset;
existing 16/64-bit register forms remain supported. ADC/SBB initially accept
matching register pairs at 8/16/32/64 bits and use carry/borrow-input opcodes;
they do not substitute ADD/SUB or implement wide-integer native selection.

## Source diagnostics

`assemble_named("path/input.asm", source, &AMD64)` and
`assemble_named_with_limits` attach the caller-supplied filename. The original
`assemble` APIs use `<input>`. Diagnostics carry a half-open UTF-8 byte range and
one-based Unicode-scalar line/column coordinates; the byte range always addresses
the original source spelling, including escaped strings. Tokens retain end
coordinates, and parsed AST nodes retain their ranges through symbol resolution
and repeated encoding passes. Encoder errors point to their instruction or
directive; duplicate/conflicting symbol definitions identify both declaration
sites, and deferred unresolved-reference errors identify the referencing node.
No source file is read implicitly. The CLI supplies the actual input filename.

`AsmError::Diagnostic` exposes structured location, related declaration and inner
error fields. `AST::new(items)` supports manually built ASTs without invented
source ranges; callers can supply `spans` and `source_name` explicitly when they
have original source metadata.

## Explicit rel/abs memory addresses

`[symbol + displacement]` retains its RIP-relative default. Explicit
`[rel symbol + displacement]` uses the same signed PC32 relocation. The addend
accounts for the displacement field and any trailing instruction immediate.
`[abs symbol + displacement]` emits a no-base SIB address with a signed disp32
field and a distinct `SignedAbsolute32` relocation (ELF `R_X86_64_32S`). Its
addend is unchanged, and the linked absolute address must fit signed 32 bits.
This is distinct from `dd symbol`, which retains unsigned `Absolute32` intent.

Explicit numeric `[rel N]` stores N as the signed displacement from the end of
this instruction; N is not an absolute target address. `[abs N]` stores a
signed absolute disp32 address. Both require `-2147483648..2147483647`; there
is no address-size override or 64-bit absolute memory field in this subset.
Equ constants in these operands resolve to the same numeric contracts.

The qualifiers appear inside brackets, after an optional byte/word/dword/qword
width. A qualifier is separated from its expression by whitespace. Ordinary
symbol spellings `[rel]`, `[abs]`, `[rel+4]` and `[abs+4]` remain available;
`rel`/`abs` followed by a separated expression are reserved for this syntax.
Explicit modes initially reject base/index registers. Existing unqualified
register addresses remain unchanged, and unqualified numeric-only addresses
remain unsupported. LEA now shares these address forms, including symbolic
RIP-relative and signed absolute addresses. MOV, supported memory ALU/IMUL,
extensions, SETcc and indirect CALL/JMP use the same address/relocation path.

## Legacy scalar SSE forms

XMM0–XMM15 form a separate register class. The supported instructions operate
on the low f32 (`ss`) or f64 (`sd`) scalar lane; this adds assembly encoding,
not floating interpretation or a native IR backend.

| Family | Forms | Encoding |
| --- | --- | --- |
| MOVSS / MOVSD | `xmm, xmm`, `xmm, memory`, `memory, xmm` | mandatory F3 / F2, `0F 10 /r` loads and register moves; `0F 11 /r` stores |
| ADDSS / ADDSD | `xmm, xmm/memory` | F3 / F2, `0F 58 /r` |
| SUBSS / SUBSD | `xmm, xmm/memory` | F3 / F2, `0F 5C /r` |
| MULSS / MULSD | `xmm, xmm/memory` | F3 / F2, `0F 59 /r` |
| DIVSS / DIVSD | `xmm, xmm/memory` | F3 / F2, `0F 5E /r` |

The mandatory prefix precedes an optional REX prefix. REX.R/B extend XMM
register operands; REX.X/B extend address registers. REX.W stays clear for
these scalar SSE forms, including the f64 forms. Memory access width is inferred
from the mnemonic, or an explicit `dword`/`qword` annotation must match it.
Register/SIB, default symbol-relative, explicit rel and signed abs addresses
share the existing checked memory/relocation rules.

These are legacy scalar instructions: their architectural upper-lane and
floating-environment behavior is not replaced by Whale IR semantics. GPR/XMM
mixing, immediates, memory destinations for arithmetic, packed/vector and
VEX/EVEX forms, MOVD/MOVQ and conversion instructions remain unsupported.
Register recognition uses exact XMM spellings; names such as `xmm16` or
`xmm_data` remain ordinary identifiers outside valid register operands.
