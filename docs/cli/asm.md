# WhaleASM CLI Documentation

WhaleASM is the first component of the Whale Toolchain.
Its role is to transform Assembly code into raw machine code (`.bin`).

Currently supported architectures:
* amd64 (fully supported)
* aarch64 (option exists, not yet implemented)

WhaleASM is designed with two modes:
* Quiet Mode for regular users
* Debug Mode for Whale developers and internal analysis

---

## Basic Usage

```bash
whale asm --amd64 <input.asm> -o <output.bin>
```

Default output (Quiet Mode) prints only one line:

```text
Wrote <N> bytes to <output.bin>
```

No internal information is shown unless Debug Mode is enabled.

---

## Command Structure

```bash
whale asm <ARCH> <INPUT> -o <OUTPUT> [OPTIONS...]
```

---

## Architecture Options (Required)

| Option      | Description                                         |
| ----------- | --------------------------------------------------- |
| `--amd64`   | Run the assembler for AMD64 (x86_64)                |
| `--aarch64` | Run the assembler for AArch64 (not implemented yet) |

---

## Basic Options

| Option            | Description                           |
| ----------------- | ------------------------------------- |
| `<input.asm>`     | Assembly input file                   |
| `-o <output.bin>` | Output binary file (.bin recommended) |


Although the output extension is not enforced, `.bin` is recommended because WhaleASM produces raw binary data.

---

## Developer Options (Debug Mode)

These options are intended for Whale developers or internal debugging.
All of these require Debug Mode to be active.

### Enable Debug Mode

```bash
--debug-whale
```

### Debug Options

| Option                | Description                                              |
| --------------------- | -------------------------------------------------------- |
| `--token`             | Print tokens                                             |
| `--ast`               | Print AST                                                |
| `--bytes`             | Print raw byte array                                     |
| `--dump-hex`          | Print hex dump with memory offsets                       |
| `--dump-bin`          | Print binary (bit-level) representation                  |
| `--dump-json`         | Output internal data in JSON format                      |
| `--stats`             | Print statistics (tokens, AST nodes, output bytes, time) |
| `--trace`             | Print pipeline execution steps                           |
| `--no-color`          | Disable ANSI color output                                |
| `--no-warn-extension` | Suppress extension warning (for non-.bin outputs)        |

---

<details>
<summary>Usage Examples (All Supported Patterns)</summary>

### 1) Basic usage

```bash
whale asm --amd64 msg.asm -o msg.bin
```

### 2) Print tokens

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --token
```

### 3) Print AST

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --ast
```

### 4) Print raw byte array

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --bytes
```

### 5) Print hex dump

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --dump-hex
```

### 6) Print bit-level dump

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --dump-bin
```

### 7) JSON output

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --dump-json
```

### 8) Print statistics

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --stats
```

### 9) Print execution trace

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --trace
```

### 10) Disable extension warning

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --no-warn-extension
```

### 11) Disable ANSI colors

```bash
whale asm --amd64 code.asm -o code.bin --debug-whale --no-color
```

### 12) Combined developer options

```bash
whale asm --amd64 code.asm -o out.bin \
    --debug-whale --token --ast --bytes
```

### 13) Full debug option set

```bash
whale asm --amd64 code.asm -o out.bin \
    --debug-whale --token --ast --bytes --dump-hex --dump-bin \
    --dump-json --stats --trace
```

### 14) Print help

```bash
whale asm --help
```

</details>