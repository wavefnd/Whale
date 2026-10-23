# IR interchange fixtures

`calls-v3.wir` and `text-identities-v3.wir` cover exact signatures, stored
function pointers, scoped and sparse IDs, repeated names and entry blocks stored
out of execution order. Their corresponding builder/lowering tests specify the
expected canonical output.

`wave-control-v3.wir` and `wave-casts-v3.wir` are retained historical text
IR outputs from Wave's typed HIR adapter. Only the language-independent IR
fixtures remain here; the old Wave source examples have been removed.
They exercise control flow, scoped IDs, calls and numeric conversions.

`text_parser.rs` verifies these outputs and requires canonical print-parse-print
equality, upgrading the header to format 8. These tests run the Rust IR module
and do not require a Wave compiler.

Legacy AST JSON formats 2/3/4 remain readable; canonical AST JSON is format 5.
Canonical typed text IR is format 8 (the reader also accepts 3/4/5/6/7). Readers reject
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


`runtime-storage-v6.wir` executes scalar global state shared across calls, explicit
lifetime end and same-allocation pointer ordering/differences. Run `@f1` for u32 12,
`@f3` for u32 42 and `@f7` for i128 16. `@f4` traps on uninitialized global storage,
`@f5` on a read-only write and `@f6` on an expired stored alias. A fresh public
interpreter invocation recreates globals; calls in that invocation share them.


## Source-aware runtime failures

`runtime-diagnostics-v7.wir` retains original source identities for a direct call,
an indirect call, division and a terminator. `ir run ... --function @f0` reports
`division by zero`, `original.wave:11:13`, IR `@f2 %b0 instruction 1`, and both
caller edges. Source filenames are provided metadata; the interpreter never opens
them or invents locations from display names/canonical text.

Library failures add `InterpreterError::Diagnostic` only for relevant original
metadata or nested caller edges. `root_cause()` and `into_root_cause()` expose the
unchanged typed trap/budget cause; `diagnostic()` exposes structured original/IR
sites and callers. Trace edges are outermost-to-innermost within the retained
innermost tail. `InterpreterOptions::max_trace_frames` defaults to 64 and is hard
clamped to 256; zero retains only an omitted count. Diagnostic source filename/span
copies have a separate 1 MiB total budget. Omitted source details and caller edges
have explicit counts; original/IR IDs remain when source details are omitted.

```sh
cargo test -p ir --all-features --locked --test runtime_diagnostics
cargo test --locked --test runtime_diagnostics_cli
```
