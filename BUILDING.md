# Building Whale

Meson automates the complete build. Vex builds Wave packages and static libraries;
Cargo builds the remaining Rust components and data bindings. Linked binaries
need none of these tools at runtime. The initial full native host is Linux x86_64 GNU.

## Build, test and install

Install Vex **v0.0.3-beta or newer**, Meson **1.8.5+**, Ninja, Python **3.13+**,
Rust **1.86+** and a native linker. Fetch Cargo dependencies once, then build:

```sh
cargo fetch --locked
meson setup /tmp/whale-build --buildtype=release -Dsocket-cli=true
meson compile -C /tmp/whale-build
meson test -C /tmp/whale-build --print-errorlogs
/tmp/whale-build/whale build-info --json
DESTDIR=/tmp/whale-install meson install -C /tmp/whale-build
```

Each compilation queries Wave's moving `nightly` release once. The helper verifies
its published archive and metadata SHA256, records the source commit, and selects
one immutable compiler/std/runtime generation for every module in that build.
A later build checks for a newer nightly. Failed downloads or compilation preserve
previous products; an online failure is reported rather than silently using old code.
No local Wave checkout, LLVM development libraries or compiler bootstrap is needed.

Use `-Dwave-cache-dir=/absolute/cache` to share verified generations. After one
online build, `meson configure /tmp/whale-build -Dwave-offline=true` reuses that
cache without network access. Compiler, std and runtime files are rechecked before
reuse. `wave-toolchain.json` in the build directory records its exact selection;
`whale build-info --json` records the linked archive/compiler identities.

Cargo is locked and offline by default; `-Dcargo-offline=false` allows missing
Rust dependencies to download. `-Dvex=/absolute/path/to/vex` selects Vex explicitly.
Use a separate build directory outside the source tree's `target/`.

## Reuse built modules or run Cargo checks

Meson accepts prebuilt directories through `asmobj-dir`, `wave-ir-dir`,
`wave-assembler-dir` and `wave-ARCH-dir` for each selected ISA. Each directory
must contain its archive, `module.meta` and `provenance.json` from the current
source and Rust binding ABI. Unselected ISAs are not linked.

For direct Cargo checks, select the Meson products explicitly:

```sh
export WHALE_ASMOBJ_DIR=/tmp/whale-build/assembler/asmobj
export WHALE_WAVE_IR_DIR=/tmp/whale-build/ir
export WHALE_WAVE_ASSEMBLER_DIR=/tmp/whale-build/assembler
export WHALE_WAVE_AMD64_DIR=/tmp/whale-build/assembler/amd64
export WHALE_WAVE_AARCH64_DIR=/tmp/whale-build/assembler/aarch64
export WHALE_WAVE_RISCV64_DIR=/tmp/whale-build/assembler/riscv64
export WHALE_WAVE_LOONGARCH64_DIR=/tmp/whale-build/assembler/loongarch64
cargo test --workspace --all-features --locked --offline
```

Cargo verifies source, ABI, archive and provenance fingerprints before linking;
it never downloads or invokes Wave. Rebuild affected modules after source changes.
For deliberate reference tests, omit the Wave variables and select
`rust-reference` explicitly, for example `cargo test -p ir --features rust-reference`.
`asmobj` has no Rust implementation. Supplying an archive always selects Wave.

## Select assembler outputs and host machines

All four assemblers are enabled by default. For an AArch64-only output product:

```sh
meson setup /tmp/whale-aarch64 \
  -Dasm-amd64=false -Dasm-riscv64=false -Dasm-loongarch64=false
```

Also disable `asm-aarch64` for a product without assemblers. `native-linker=false`
removes native executable linking. These selections describe generated code,
independently of the host running Whale.

A Meson cross file plus `-Dcargo-host-target=RUST_TRIPLE` describes the executable
host. Cross builds also need the Rust target and linker. Vex produces matching
ELF, COFF or Mach-O archives; cross-produced objects do not establish native
execution support. Cross runtime tests require explicit
`-Dhost-runner='["wrapper", "argument"]'`; otherwise they are omitted.

## Add an architecture in the source tree

Place Wave code under `assembler/ARCH/src/`, split by registers, operands,
parsing, instructions and encoding. Put Rust transport bindings under
`bindings/rust/ARCH/`. Add its Vex workspace member and Meson selection, update
`target-spec`, CLI composition and `asmobj` relocation mappings as needed, and
test bytes, rejected inputs and object metadata. Shared modules must not import
concrete ISAs. See [CONTRIBUTING.md](CONTRIBUTING.md#2-repository-structure-and-module-boundaries).
