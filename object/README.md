# Object input and diagnostics

The object library reads the bounded AMD64 ELF64 little-endian relocatable subset.
Its writer additionally serializes AArch64, RV64I and LoongArch64 little-endian
ELF64 objects, using explicit machine identities, ABI flags and relocation namespaces.
See the [assembler guide](../assembler/README.md) for those initial output subsets. Reading uses only checked byte access, independent of host
alignment, struct layout or byte order. It neither loads nor executes code.

```rust
use object::{ObjectFile, ObjectReadLimits};
use diagnostic::{Context, ToDiagnostic};

fn inspect(bytes: &[u8]) -> Result<ObjectFile, diagnostic::Diagnostic> {
    ObjectFile::read_with_limits(bytes, ObjectReadLimits {
        max_input_bytes: 1024 * 1024,
        max_work: 8 * 1024 * 1024,
        ..Default::default()
    }).map_err(|error| error.diagnostic().with_context(Context::Object {
        name: "input.o".into(), member: None,
    }))
}
```

`ObjectFile::read` uses default limits: 256 MiB input, 65,279 section headers,
4,096 program headers, one million symbol/relocation records each, 256 MiB
cumulative copied payloads, 16 MiB cumulative name bytes inspected/copied and
512 MiB of inspection/copy work units. Per-field overrides are independent;
metadata aliases and repeated strings still consume cumulative budgets. BSS
retains its logical size in `zero_fill` without allocating zero bytes.

The importer accepts ordinary allocated text, read-only data, initialized data
and BSS; local/global/weak NOTYPE/FUNC/OBJECT symbols with default/hidden
visibility; and the RELA types emitted by this writer (64, 32, PC32, PC8,
GOTPCREL, PLT32). Imported dynamic relocation intent is metadata, not a claim
that the static linker can apply every kind. Unknown types/flags are errors.

NASM FILE/ABS auxiliary symbols are validated and omitted from link definitions.
SECTION symbols become deterministic collision-free local names, and every
relocation index is remapped to its intended symbol. Ordinary symbol names are
never rewritten. Other ABS/COMMON definitions, TLS, dynamic symbol tables,
non-allocating payload sections, extended numbering, non-UTF-8 names and multiple
symbol tables are outside this initial subset. Retained NASM fixtures run under
Cargo without an external assembler; provenance is in tests/fixtures/README.md.
Writer→reader round trips preserve semantic contents, not table ordering or
auxiliary producer metadata.

`ObjectError` exposes `kind`, `phase`, optional `resource`, and typed contexts.
`validate_diagnostic`, `write_diagnostic` and `write_diagnostic_with_limit` keep
validation, range, overflow, output-budget and allocation failures inspectable.
The existing `validate`/`write`/`write_with_limit` String APIs render the same
causes. `ToDiagnostic` converts them to the common rendering envelope; adding
object/archive-member context never requires parsing English messages.

`formats::elf_read::ElfView` exposes checked borrowed ELF header, section and
program-table access for read-only consumers. It validates layout and target
identity; container consumers must enforce their own ET_REL/ET_EXEC, section,
program-segment and work-budget semantics. This shared view alone does not
advertise executable-image support.

Signed absolute-32 intent is `RelocKind::SignedAbsolute32`, represented by
R_X86_64_32S (11). The reader/writer keep it distinct from unsigned Absolute32
(R_X86_64_32, 10) and PC-relative relocation kinds. Static application uses signed
32-bit range checks; declaring a signed kind never changes unsigned behavior.
