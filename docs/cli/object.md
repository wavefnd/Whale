# Whale Object CLI Documentation

The `object` command is used to wrap raw binary data or IR into standard object file formats (like ELF).
This allows Whale-generated code to be linked with other tools or linked into final executables.

---

## Basic Usage

```bash
whale object <input.bin> -o <output.o>
```

---

## Description

Currently, this command takes a raw binary file and wraps it into an **ELF64** relocatable object file.
It automatically adds a global symbol `start` at the beginning of the code.

Future versions will support:
* Converting Whale IR to Object files
* Adding multiple symbols and relocations via CLI or metadata files
* Supporting Mach-O (macOS) and PE (Windows) formats

---

## Options

| Option | Description |
| --- | --- |
| `<input.bin>` | Input raw binary file |
| `-o <output.o>` | Output object file (.o) |

---

## Developer Options (Debug Mode)

These options are identical to the `asm` command and are intended for Whale developers.
All of these require `--debug-whale` to be active.

### Enable Debug Mode

```bash
--debug-whale
```

### Debug Options

| Option | Description |
| --- | --- |
| `--token` | Show tokens (Currently not applicable for binary input) |
| `--ast` | Show AST (Currently not applicable for binary input) |
| `--bytes` | Print raw byte array of the generated object file |
| `--dump-hex` | Print hex dump of the generated object file |
| `--dump-bin` | Print binary (bit-level) representation |
| `--dump-json` | Output internal metadata in JSON format |
| `--stats` | Print statistics (input size, output size, time) |
| `--trace` | Print pipeline execution steps |
| `--no-color` | Disable ANSI color output |
| `--no-warn-extension` | Suppress extension warning |

---

## Example

```bash
# Assemble to bin first
whale asm --amd64 mycode.asm -o mycode.bin

# Wrap into ELF object
whale object mycode.bin -o mycode.o
```
