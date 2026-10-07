# IR interchange fixtures

`calls-v3.wir` and `text-identities-v3.wir` cover exact signatures, stored
function pointers, scoped and sparse IDs, repeated names and entry blocks stored
out of execution order. Their corresponding builder/lowering tests specify the
expected canonical output.

`wave-control.wave` and `wave-casts.wave` are complete Wave programs.
`wave-control-v3.wir` and `wave-casts-v3.wir` were produced by Wave's typed HIR
adapter from working-tree revision `13a406d0fe2802ccc234264baab113c19249c7a9`.
The frontend crates and adapter were copied into a temporary workspace, with
only that copy's Whale dependency pointed at this IR implementation. The source
Wave checkout was not changed. No LLVM backend or external assembler was used.
The control-flow case includes a loop, branches, short-circuit phi and direct
calls; the cast case includes signed extension, integer-to-float and float
truncation with memory loads/stores.

`text_parser.rs` reads every retained output, verifies it and requires canonical
print-parse-print equality, upgrading the header to format 4. These tests do not require a Wave checkout
or claim native execution. To regenerate with a matching Wave checkout and a
Whale dependency containing this reader/printer, run from the Wave root:

```sh
cargo run -- --whale build /path/to/wave-control.wave --emit=ir
cargo run -- --whale build /path/to/wave-casts.wave --emit=ir
```

AST JSON remains format 2, independently of canonical typed text IR format 4 (reader also accepts 3). Readers reject
unsupported versions; fixtures are migrated explicitly when the format changes.

## Scalar integer execution oracle

`integer-operations-v3.wir` is a complete, executable conformance module:
`@f0` returns `u8 0`, `@f1` explicitly traps on the checked overflow flag,
`@f2` returns `i8 -128` for MIN / -1, `@f3` returns `i8 0` for MIN % -1,
and `@f4` returns `i8 -128` for a shift count with signed value -1.
`interpreter-loop-v3.wir` uses reordered blocks, sparse IDs and simultaneously
updated loop-carried phis; `@f7` with `u32 3` returns `i32 22` in 32 steps.

The interpreter regression suite supplies independent expected boundary values
for all twelve integer types (signed/unsigned widths 1, 8, 16, 32, 64, 128).
It checks wrapping add/sub/mul against both the runtime and the separate
ConstExpr evaluator. ConstExpr currently exposes only add/sub/mul and
comparisons; division/remainder/shifts remain runtime instructions, with
explicit edge vectors ready for any future constant-evaluation extension.
The preserved Wave fixtures use memory and are parser/verifier fixtures;
`wave-control-v3.wir` @f0 now executes through tracked integer memory; its main
function still rejects calls, and the cast fixture still rejects float execution.

`tracked-memory-v4.wir` provides runnable pointer-copy and uninitialized-read
cases. `initialization-loop-v4.wir` resets a hoisted slot on every declaration
and traps on the second loop iteration instead of reading the first value.
