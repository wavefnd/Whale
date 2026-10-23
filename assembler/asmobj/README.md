# Assembler output to object model

This complete module is implemented in Wave. It has no Rust crate, Rust
implementation, or language-specific `wave/` subdirectory.

| Source | Role |
| --- | --- |
| `model.wave` | Private borrowed input/output records and resource checks |
| `sections.wave` | Section role and sparse-storage mapping |
| `symbols.wave` | Symbol mapping, name lookup and undefined-symbol creation |
| `relocations.wave` | ELF, COFF and Mach-O relocation translation by ISA |
| `entry.wave` | Validate the complete conversion before publishing output |

Meson builds these explicit sources as one module. The calling Rust code in
`toolchain/src/assembly_object.rs` transfers data across the language boundary;
`object/` owns generic object validation and file serialization. There is no Rust
copy of asmobj's mapping rules. An executable built without this module reports
that assembly-to-object conversion is unavailable.

New architectures extend the relocation source and its tests, together with
ordinary target/build-list edits elsewhere in Whale. The borrowed records are
private implementation details.

The implementation preserves source order, creates each missing external symbol
once, and distinguishes control transfers from data relocations. It preflights
input, capacity and bounded name-lookup work before changing output records.
It borrows immutable input only for the duration of a call.

See [BUILDING.md](../../BUILDING.md) for build commands. Independent native
checks are in `tools/check_asmobj.py`; public Rust-call integration tests are in
`toolchain/tests/assembly_object.rs`.
