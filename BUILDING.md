# Building Whale with Meson

Meson builds the Wave IR, shared assembler, selected ISA modules and `asmobj`,
then uses Cargo for Rust data transport, files, object formats and CLI components.
IR and assembler production algorithms run in Wave. Rust references require the
explicit `rust-reference` feature and are for comparison tests.
Generated files and Cargo outputs stay in the selected build directory.

## Build, test and install

Install Meson 1.8 or newer, Ninja, Python 3.12 or newer, Git, Rust 1.86 or newer,
and a native linker. Building `asmobj` also requires LLVM 21 development
libraries, `ar`, and installed Rust **1.89.0** for the pinned Wave compiler.
The initial executable host for the linked module is **Linux x86_64 GNU**.
Whale's Rust MSRV and the Wave bootstrap's Rust version are separate requirements.
`whale build-info --json` reports linked module identities and build fingerprints.

Provide a Wave Git repository containing commit
`e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b`. The helper reads that commit with
`git archive` and leaves the checkout untouched, including any local changes.
Run these commands from the Whale source root:

```sh
# Fetch Rust dependencies separately when they are not cached.
cargo fetch --locked
# Adjust this prefix to the installed LLVM 21 location.
export LLVM_SYS_211_PREFIX=/usr/lib64/llvm21
meson setup /tmp/whale-build \
  -Dwave-source=/absolute/path/to/Wave \
  -Dwave-bootstrap-dir=/tmp/whale-bootstrap \
  -Dwave-rust-toolchain=1.89
meson compile -C /tmp/whale-build
meson test -C /tmp/whale-build --print-errorlogs
/tmp/whale-build/whale build-info --json
DESTDIR=/tmp/whale-install meson install -C /tmp/whale-build
/tmp/whale-install/usr/local/bin/whale --version
```

Use the installed alias `1.89` above only if it reports exactly Rust 1.89.0;
the option defaults to `1.89.0`. All four assembler architectures are selected
by default. `--buildtype=release` selects optimized Rust output, and
`-Dsocket-cli=true` includes experimental AST JSON lowering.

Cargo resolution is locked and offline by default. `-Dcargo-offline=false`
permits host dependency downloads; `-Dwave-allow-network=true` separately permits
locked Wave compiler dependency downloads. Setup does not run Cargo or bootstrap
the compiler. Compilation schedules the pinned compiler and matching std, the
complete `asmobj` archive, and then the Whale executable.

Each build directory has its own Cargo output and executable. Meson tracks Cargo
dependency files, local Rust sources and manifests, and the explicit `asmobj`
source list and artifact metadata. A compiler/std cache can be shared through
`wave-bootstrap-dir`; cache identities and hashes are validated before reuse.
Do not use the source tree's `target/` as a Meson build directory.

## Reuse built modules or run Cargo checks

The helpers read the pinned Wave commit without changing its checkout:

| Helper | Module |
| --- | --- |
| `tools/build_asmobj.py` | Assembly-to-object mapping |
| `tools/build_wave_ir.py` | IR, passes, interpreter and bytecode |
| `tools/build_wave_assembler.py --module common` | Shared assembler |
| `tools/build_wave_assembler.py --module amd64` | Selected ISA; also `aarch64`, `riscv64`, `loongarch64` |

Each accepts `--wave-source`, `--out-dir`, `--bootstrap-dir` and
`--rust-toolchain` as in this example:

```sh
python3 tools/build_wave_ir.py --wave-source /absolute/path/to/Wave \
  --out-dir /tmp/whale-ir --bootstrap-dir /tmp/whale-bootstrap --rust-toolchain 1.89
```

Prebuilt Meson products require `-Dasmobj-dir=...`, `-Dwave-ir-dir=...`,
`-Dwave-assembler-dir=...` and `-Dwave-ARCH-dir=...` for every selected ISA.
Unselected ISA archives are not linked. Meson requires all selected modules;
ambient environment variables cannot replace its explicit selections.

For direct native Cargo checks, select those same archives explicitly:

```sh
export WHALE_ASMOBJ_DIR=/path/to/asmobj
export WHALE_WAVE_IR_DIR=/path/to/ir
export WHALE_WAVE_ASSEMBLER_DIR=/path/to/common
export WHALE_WAVE_AMD64_DIR=/path/to/amd64
export WHALE_WAVE_AARCH64_DIR=/path/to/aarch64
export WHALE_WAVE_RISCV64_DIR=/path/to/riscv64
export WHALE_WAVE_LOONGARCH64_DIR=/path/to/loongarch64
cargo test --workspace --all-features --locked
```

Source, ABI, archive and pinned compiler/std fingerprints are checked before
linking; rebuild the affected archive after changing its boundary or Wave source.
Failed compilation or archiving preserves the previous complete publication.
Cargo never bootstraps or downloads Wave automatically. A linked executable
needs no `wavec` at runtime. The numeric core uses two 64-bit words for 128-bit
integers and exact IEEE rounding without host floating-point arithmetic.

For deliberate Rust reference checks, omit the `WHALE_WAVE_*` variables and use
`cargo test --workspace --all-features --locked`, or select `--features rust-reference`
on the component being tested. For a reference CLI:

```sh
cargo build --no-default-features \
  --features asm-amd64,socket-cli,native-linker,rust-reference
```

Supplying a Wave archive selects Wave even with `rust-reference` enabled.
Missing production archives fail with setup instructions. `asmobj` has no Rust
implementation; reference CLI assembly-to-object checks still need its archive.

## Select assembler outputs and host machines

Architecture selection controls generated code independently of the machine
running Whale. For an AArch64-only product:

```sh
meson setup /tmp/whale-aarch64 -Dwave-source=/absolute/path/to/Wave \
  -Dasm-amd64=false -Dasm-riscv64=false -Dasm-loongarch64=false
meson compile -C /tmp/whale-aarch64
```

Also set `-Dasm-aarch64=false` for a product without concrete assemblers.
`meson configure BUILD_DIR -DOPTION=VALUE` changes an existing selection.

A Meson cross file and `-Dcargo-host-target=RUST_TRIPLE` describe the final
Whale executable host. The Rust target must agree with Meson's host machine.
Cross compilation additionally needs the matching Rust target, linker, and
module archive. The build helper's `--target` defaults to
`x86_64-unknown-linux-gnu`; producing an object for another target does not
establish that Whale runs on that host. COFF and Mach-O cross archives need
LLVM ar: set `AR=/absolute/path/to/llvm-ar`. The helper explicitly selects the
linker's archive format; GNU ar cannot produce these cross-format indexes.
The initial linked native host remains Linux x86_64 GNU.

Native runtime tests run directly. Cross runtime tests require an explicit
`-Dhost-runner='["wrapper", "argument"]'`; otherwise they are omitted.
Arguments are passed without shell interpretation. Record omitted checks as
omitted, without treating them as executed native tests.

## Add an architecture in the source tree

Keep an architecture's registers, operands, parser, instructions and encoder in
separate files under `assembler/<architecture>/`. Shared helpers contain only
behavior needed by multiple architectures. IR target lowering belongs beside
its target implementation.

A contribution normally updates these explicit points:

1. Target identity and data layout in `target-spec/` when required.
2. Source/build lists and the architecture's Meson selection option.
3. Product composition in `src/builtins.rs` and deliberate CLI aliases.
4. Relocation translation in `assembler/asmobj/` and object format support.
5. Encoding, rejection, object and CLI tests, plus capability documentation.

Official and contributed architectures use the same layout. Small central enum,
dispatch and build-list edits are expected. Run
`python3 tools/check_module_layers.py` to check dependency direction. See the [contribution guide](CONTRIBUTING.md#2-repository-structure-and-module-boundaries)
for source responsibilities and checks.
