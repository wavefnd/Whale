# Whale Toolchain

Whale is a general-purpose, high-performance low-level toolchain written in Rust. It is designed to be a modern, lightweight competitor to LLVM and GCC, serving as the primary backend for the **Wave** programming language.

## Key Components

- **Whale Assembler (`asm`)**: A professional-grade assembler supporting modern instruction sets (AMD64) with advanced features like multi-section support, standard directives (`global`, `section`, `extern`), and precise ELF relocation generation.
- **Whale Object (`object`)**: A modular object file manipulation library and CLI. It handles internal representations of sections, symbols, and relocations, providing a pure Rust implementation of ELF64 generation.
- **Whale Linker (`linker`)**: A next-generation linker designed for modularity and speed. It features advanced symbol resolution, memory layout computation, and cross-object relocation processing.

## Project Philosophy

Whale is built on the belief that modern systems programming deserves a toolchain that is:
1. **Modular**: Every component is a reusable Rust crate.
2. **Transparent**: Built from scratch to eliminate the "black box" nature of legacy toolchains.
3. **Performant**: Leverages Rust's memory safety and zero-cost abstractions for maximum efficiency.

## Getting Started

### Installation

```bash
cargo build --release
```

### Basic Usage

#### 1. Assemble Source
```bash
# Assemble to ELF object file
whale asm --amd64 input.asm -o output.o

# Or assemble to raw binary
whale asm --amd64 input.asm -o output.bin
```

#### 2. Create Object from Binary
```bash
whale object input.bin -o output.o
```

#### 3. Link Objects (Experimental)
```bash
whale link obj1.o obj2.o -o executable
```

## Documentation

Detailed CLI documentation can be found in the `docs/cli` directory:
- [Assembler CLI](docs/cli/asm.md)
- [Object CLI](docs/cli/object.md)

## License

This project is licensed under the MPL-2.0 License - see the [LICENSE](LICENSE) file for details.
