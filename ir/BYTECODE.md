# Whale bytecode v1

The internal `whale-bytecode-v1` target uses tracked 64-bit little-endian storage,
with scalar alignments 1/2/4/8/16, pointer alignment 8, and 16-byte allocation
alignment for arrays of at least 16 bytes. Wave writes, loads, verifies and
executes it. Rust owns files, initialized buffers and typed runtime requests.
The initial executable Wave host is Linux AMD64.

```sh
whale ir emit program.wir --target whale-bytecode-v1 -o program.wbc
whale bytecode verify program.wbc
whale bytecode run program.wbc --function @f7 --arg 42
```

The source IR must name this target; compilation does not relabel another target.
Input, decoded bytes, workspace and work have cumulative bounds. Execution also
limits steps, call depth, frames, values and tracked memory. No assembler or
native executable linker is required. V1 has no runtime imports: referenced
external declarations fail before publication or execution.

## File layout

All integers are unsigned little-endian u64 fields; signed literals use their
exact sign-extended 128-bit low/high representation. No native struct padding,
address, capability, allocation generation or runtime state appears in a file.

The 64-byte header contains eight words: magic bytes `WHALEBC\0`, format 1,
IR semantics 1, storage profile 1, total file bytes, table count 16, flags 0,
and header-plus-directory size 576. Sixteen 32-byte directory entries follow:
`kind, offset, count, stride`. Kinds are ordered 1–16; payloads are contiguous
in that order. Gaps, overlaps, unknown versions and trailing bytes are rejected.

| Kind | Table | Bytes per record |
| --- | --- | --- |
| 1 | Type: kind, child, first, count, extra | 40 |
| 2 | Type-edge index | 8 |
| 3 | Function: id, signature, entry, first_block, block_count, first_parameter, parameter_count, value_count, first_value_type, has_body, linkage | 88 |
| 4 | Block: id, function, first_instruction, instruction_count | 32 |
| 5 | Instruction, fields below | 136 |
| 6 | Operand word | 8 |
| 7 | Dense-slot type index | 8 |
| 8 | Storage global: id, type, align, readonly, initialized, literal, low, high | 64 |
| 9 | Site: original function, block, instruction ordinal, value ID | 32 |
| 10 | Text slice: first byte, byte count | 16 |
| 11 | UTF-8 text bytes | 1 |
| 12 | Source file: original ID, text index | 16 |
| 13 | Original node: id, has_span, file, line, column, end_line, end_column | 56 |
| 14 | Source mapping: kind, function, block, index, original | 40 |
| 15 | Original SSA ID, parallel to dense-slot types | 8 |
| 16 | Folded constant-global ID | 8 |

Instruction fields are `op, sub, type, other_type, dst, a, b, c, align, first,
count, low, high, literal, signature, flags, global`. `u64::MAX` means no value
or destination. Type and opcode tags match `runtime_model.wave` and the typed
runtime transport. Types are postorder: children precede parents. Type kinds
0–21 denote Void, Bool, I1, U1, I8/I16/I32/I64/I128, U8/U16/U32/U64/U128,
F16/F32/F64, Ptr, FnPtr, Array, Struct and Tuple. Derived layout sizes and GEP
plans are recomputed, never loaded. Text slices cover the blob contiguously;
repeated filename copies and expanded type trees consume workspace limits.

Opcodes 0–35 are GlobalAddr, LifetimeEnd, PtrDiff, ConstDecl, Const, Undef,
Mov, Bin, Neg, Not, Cmp, ICmp, FCmp, Select, Cast, Phi, Extract, Checked,
Alloca, VolatileLoad, VolatileStore, Load, Store, Uninit, GEP, Memcpy, Memset,
NullFunction, FunctionAddr, Call, TrapIf, Br, CBr, Switch, Ret and Trap.
Undef and unsupported aggregate value operations are rejected even in unused
functions. Aggregate tracked storage is supported. Numeric subcodes retain
the IR runtime mappings; unknown subcodes are rejected, never executed.

Phi operands are `(slot, predecessor block)` pairs; switch operands are
`(low, high, target block)` triples; calls and GEP use slot lists. Block indices
are global table indices; SSA slots are local to each function. Source mapping
kinds are instruction 0, terminator 1 and global 2. Sites preserve sparse IR IDs.
Each block ends in exactly one terminator. Wave derives definitions, predecessors,
reachability and dominators before checking types, phi edges and SSA availability.
Only complete admission can reach the resumable Wave runtime and memory effects.

`tools/check_wave_bytecode.py` builds independent LE fixtures and mutations.
`tests/fixtures/bytecode` contains valid and malformed binaries generated without
the Wave writer. The separate writer checker compares exact expected bytes.
