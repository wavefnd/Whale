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

`text_parser.rs` reads every retained output, verifies it and requires exact
canonical print-parse-print equality. These tests do not require a Wave checkout
or claim native execution. To regenerate with a matching Wave checkout and a
Whale dependency containing this reader/printer, run from the Wave root:

```sh
cargo run -- --whale build /path/to/wave-control.wave --emit=ir
cargo run -- --whale build /path/to/wave-casts.wave --emit=ir
```

AST JSON remains format 2, independently of typed text IR format 3. Readers reject
unsupported versions; fixtures are migrated explicitly when the format changes.
