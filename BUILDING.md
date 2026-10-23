# Building Whale with Meson

Meson configures the product, compiles the Wave `asmobj` module, then builds the
Rust components with Cargo. `assembler/asmobj/` is implemented entirely in Wave;
the remaining assembler, IR, object, linker and CLI implementations remain Rust.
Generated files and Cargo outputs stay in the selected build directory.

## Build, test and install

Install Meson 1.8 or newer, Ninja, Python 3.12 or newer, Git, Rust 1.86 or newer,
and a native linker. Building `asmobj` also requires LLVM 21 development
libraries, `ar`, and installed Rust **1.89.0** for the pinned Wave compiler.
The initial executable host for the linked module is **Linux x86_64 GNU**.
Whale's Rust MSRV and the Wave bootstrap's Rust version are separate requirements.

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

## Reuse a built module

Build the complete module directly when an existing archive is preferable:

```sh
LLVM_SYS_211_PREFIX=/usr/lib64/llvm21 python3 tools/build_asmobj.py \
  --wave-source /absolute/path/to/Wave --out-dir /tmp/whale-asmobj \
  --bootstrap-dir /tmp/whale-bootstrap --rust-toolchain 1.89
meson setup /tmp/whale-prebuilt -Dasmobj-dir=/tmp/whale-asmobj
meson compile -C /tmp/whale-prebuilt
meson test -C /tmp/whale-prebuilt --print-errorlogs
```

Meson requires either `wave-source` or `asmobj-dir`; `asmobj` is part of the
product. Explicit archive metadata is checked before linking. The helper records
the pinned compiler, std, source and archive hashes. Failed compilation or
archiving preserves the previous complete publication. Ambient environment
variables cannot select a different module for a Meson product.

For work confined to Rust components, ordinary Cargo builds remain available:

```sh
cargo build --workspace --locked
cargo test -p ir --all-features --locked
# Include the complete Wave module for assembly-to-object integration tests.
export WHALE_ASMOBJ_DIR=/tmp/whale-asmobj
cargo test --workspace --all-features --locked
```

Cargo never bootstraps or downloads Wave automatically. A build without the
module can inspect objects and use Rust IR APIs; assembly-to-object commands
return an explicit unavailable error. There is no Rust implementation of the
`asmobj` policies selected in its place. A linked executable needs no `wavec`
at runtime.

## Select assembler outputs and host machines

Architecture selection controls generated code independently of the machine
running Whale. For an AArch64-only product:

```sh
meson setup /tmp/whale-aarch64 -Dasmobj-dir=/tmp/whale-asmobj \
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
