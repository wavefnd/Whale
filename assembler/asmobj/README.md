# Assembler output to object model

This complete module is implemented in Wave. It has no Rust crate, Rust
implementation, or language-specific `wave/` subdirectory.

| Source | Role |
| --- | --- |
| `src/model.wave` | Private borrowed input/output records and resource checks |
| `src/sections.wave` | Section role and sparse-storage mapping |
| `src/symbols.wave` | Symbol mapping, name lookup and undefined-symbol creation |
| `src/relocations.wave` | Bounded namespace/target dispatch and relocation records |
| `src/relocations/*.wave` | ISA-owned ELF, COFF and Mach-O translation rules |
| `src/lib.wave` | Validate the complete conversion before publishing output |

Meson asks Vex to build this package as one static library. The calling Rust code in
`bindings/rust/asmobj/src/lib.rs` transfers data across the language boundary;
`object/` owns generic object validation and file serialization. There is no Rust
copy of asmobj's mapping rules. An executable built without this module reports
that assembly-to-object conversion is unavailable.

New architectures add a relocation role file and its tests, together with
ordinary target/build-list edits elsewhere in Whale. The borrowed records are
private implementation details.

The implementation preserves source order, creates each missing external symbol
once, and distinguishes control transfers from data relocations. It preflights
input, capacity and bounded name-lookup work before changing output records.
It borrows immutable input only for the duration of a call.

See [BUILDING.md](../../BUILDING.md) for build commands. Independent native
checks are in `tools/check_asmobj.py`; public Rust-call integration tests are in
`bindings/rust/asmobj/tests/assembly_object.rs`.
