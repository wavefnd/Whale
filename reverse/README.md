# Reverse image inspection

The `reverse` crate provides bounded ELF/raw image inspection and shared typed
instruction, control-flow and reference records. Its decoder and CFG
implementation are currently unavailable. `backend_available()` returns
`false`, `backend_name()` returns `"unavailable"`, `decode` returns
`DecodeError::CoreUnavailable`, and `analyze` returns
`CfgError::CoreUnavailable`.

The former partial Wave decoder and CFG implementation, build helper and
archive integration have been removed. No decoder archive or Cargo feature can
enable them. The complete Wave module currently built by Whale is the separate
[assembly-to-object module](../assembler/asmobj/).

## Image inspection

`image::Image` borrows code bytes and distinguishes region-relative offsets,
file offsets and virtual addresses. ET_REL uses the bounded AMD64 object
importer and retains symbolic RELA annotations. ET_EXEC uses checked ELF header
access and executable PT_LOAD file ranges, including files without section
tables. BSS tails and pure-BSS segments are never materialized; a zero file size
requires no file-backed range.

Zero-sized NOTYPE linker boundary markers outside their defining section
remain non-code metadata. TLS symbols are omitted because their offsets are
not process virtual addresses. Dynamic/PIE images are outside this subset.

```sh
cargo run --locked -- reverse inspect app --json
cargo run --locked -- reverse inspect raw.bin --raw --amd64 --base 0x400000 --json
```

Inspection needs no Wave compiler or archive. Input, record, work and output
budgets remain explicit. Text and JSON reports stream into the output byte
ceiling, and file output publishes atomically after success.

`whale disasm`, `whale reverse cfg` and `whale reverse xrefs` report their
unavailable capability and preserve an existing output file. The retained
record/rendering APIs describe supplied typed facts; they do not implement
instruction decoding or establish reachability. Source recovery, decompilation
and IR lifting remain outside the current scope.
