"""Compare native assembler bytes/RELA metadata against LLVM (optional development tool).

Ordinary Cargo tests use the retained fixtures and do not require LLVM.
"""
import argparse
from pathlib import Path
import random
import re
import shutil
import struct
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def sections(data):
    if len(data) < 64 or data[:7] != b"\x7fELF\x02\x01\x01":
        raise ValueError("expected ELF64 little endian")
    offset = struct.unpack_from("<Q", data, 40)[0]
    size, count, names_index = struct.unpack_from("<HHH", data, 58)
    if size != 64 or not 0 < names_index < count or offset + count * size > len(data):
        raise ValueError("invalid section table")
    headers = [struct.unpack_from("<IIQQQQIIQQ", data, offset + i * size) for i in range(count)]

    def contents(header):
        if header[1] == 8:
            return b""
        start, length = header[4:6]
        if start + length > len(data):
            raise ValueError("invalid section range")
        return data[start:start + length]

    names = contents(headers[names_index])
    result = {}
    for header in headers:
        start = header[0]
        end = names.find(b"\0", start)
        if end < 0:
            raise ValueError("invalid section name")
        result[names[start:end].decode()] = contents(header)
    return result


def generated(arch, count, rng):
    for i in range(count):
        rd, rn, rm = (rng.randrange(31 if arch == "aarch64" else 32) for _ in range(3))
        if arch == "aarch64":
            width = rng.choice(["x", "w"])
            if i % 3 == 0:
                yield f"{rng.choice(['add', 'sub', 'adds', 'subs', 'and', 'orr', 'eor', 'ands'])} {width}{rd},{width}{rn},{width}{rm}"
            elif i % 3 == 1:
                yield f"{rng.choice(['ldr', 'str'])} {width}{rd},[x{rn},#{rng.randrange(4096) * (8 if width == 'x' else 4)}]"
            else:
                yield f"{rng.choice(['b', 'bl'])} {rng.randrange(-(1 << 25), 1 << 25) * 4}"
        elif arch == "riscv64":
            if i % 3 == 0:
                yield f"{rng.choice(['add', 'sub', 'xor', 'or', 'and', 'sll', 'srl', 'sra', 'slt', 'sltu', 'addw', 'subw'])} x{rd},x{rn},x{rm}"
            elif i % 3 == 1:
                yield f"{rng.choice(['lb', 'lh', 'lw', 'ld', 'lbu', 'lhu', 'lwu'])} x{rd},{rng.randrange(-2048,2048)}(x{rn})"
            else:
                yield f"{rng.choice(['beq', 'bne', 'blt', 'bge', 'bltu', 'bgeu'])} x{rn},x{rm},{rng.randrange(-2048,2048) * 2}"
        else:
            if i % 3 == 0:
                yield f"{rng.choice(['add.w', 'add.d', 'sub.w', 'sub.d', 'slt', 'sltu', 'and', 'or', 'xor', 'nor', 'sll.d', 'srl.d', 'sra.d'])} $r{rd},$r{rn},$r{rm}"
            elif i % 3 == 1:
                yield f"{rng.choice(['ld.b', 'ld.h', 'ld.w', 'ld.d', 'ld.bu', 'ld.hu', 'ld.wu', 'st.b', 'st.h', 'st.w', 'st.d'])} $r{rd},$r{rn},{rng.randrange(-2048,2048)}"
            else:
                yield f"{rng.choice(['beq', 'bne', 'blt', 'bge', 'bltu', 'bgeu'])} $r{rn},$r{rm},{rng.randrange(-32768,32768) * 4}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--llvm-mc", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=256)
    parser.add_argument("--seed", type=int, default=1464353100)
    parser.add_argument("--execute", action="store_true", help="also link two objects with LLD and run with qemu-ARCH (Linux)")
    args = parser.parse_args()
    if not 1 <= args.cases <= 4096 or not 0 <= args.seed < 2**64:
        parser.error("cases must be 1..4096 and seed an unsigned u64")
    binary, llvm = args.binary.resolve(strict=True), args.llvm_mc.resolve(strict=True)
    rng = random.Random(args.seed)

    def run(command, **kwargs):
        result = subprocess.run(command, capture_output=True, timeout=60, **kwargs)
        if result.returncode:
            raise RuntimeError(f"{command}: {result.stderr!r}")
        return result

    for arch, machine, call, code in [("aarch64",183,"bl",283),("riscv64",243,"jal ra,",17),("loongarch64",258,"bl",66)]:
        fixtures = []
        expected = bytearray()
        for line in (ROOT / "assembler" / arch / "tests/fixtures/encodings.tsv").read_text().splitlines():
            if line and not line.startswith("#"):
                hex_bytes, source = line.split("\t")
                fixtures.append(source)
                expected.extend(bytes.fromhex(hex_bytes))
        all_sources = [*fixtures, *generated(arch, args.cases, rng)]
        source = "\n".join(all_sources) + "\n"
        response = run([str(llvm), "--triple=" + arch, "--show-encoding"], input=source, text=True)
        encodings = re.findall(r"encoding:\s*\[([^\]]+)\]", response.stdout)
        if len(encodings) != len(all_sources):
            raise AssertionError("LLVM did not emit one instruction per source statement")
        oracle = b"".join(bytes(int(n.strip(), 16) for n in item.split(",")) for item in encodings)
        if oracle[:len(expected)] != expected:
            raise AssertionError(f"{arch}: retained fixtures disagree with LLVM")
        with tempfile.TemporaryDirectory(prefix="whale-isa-oracle-") as directory:
            root = Path(directory)
            input_path, output_path = root / "input.s", root / "output.o"
            input_path.write_text(source)
            run([str(binary), "asm", "--" + arch, str(input_path), "-o", str(output_path)])
            actual = output_path.read_bytes()
            if struct.unpack_from("<H", actual, 18)[0] != machine:
                raise AssertionError(f"{arch}: wrong ELF machine")
            text = sections(actual)[".text"]
            if len(text) != len(oracle):
                raise AssertionError(f"{arch}: instruction length mismatch")
            if text != oracle:
                first = next(i for i in range(min(len(text),len(oracle))) if text[i] != oracle[i]) // 4
                raise AssertionError(f"{arch}: bytes differ at {all_sources[first]!r}")
            symbolic = f".text\n.extern external\n{call} external\n.data\n.quad external+8\n"
            input_path.write_text(symbolic)
            run([str(binary), "asm", "--" + arch, str(input_path), "-o", str(output_path)])
            actual_sections = sections(output_path.read_bytes())
            llvm_path = root / "llvm.o"
            run([str(llvm), "--triple=" + arch, "--filetype=obj", str(input_path), "-o", str(llvm_path)])
            oracle_sections = sections(llvm_path.read_bytes())
            for name, expected_code in [(".rela.text", code),(".rela.data",257 if arch == "aarch64" else 2)]:
                actual_rela, oracle_rela = actual_sections[name], oracle_sections[name]
                if len(actual_rela) != 24 or len(oracle_rela) != 24:
                    raise AssertionError("expected exactly one RELA entry")
                for rela in (actual_rela, oracle_rela):
                    if struct.unpack_from("<Q",rela,8)[0] & 0xffffffff != expected_code:
                        raise AssertionError("relocation code mismatch")
                if actual_rela[:8] != oracle_rela[:8] or actual_rela[16:] != oracle_rela[16:]:
                    raise AssertionError("relocation offset/addend mismatch")
            if args.execute:
                lld = shutil.which("ld.lld")
                qemu = shutil.which("qemu-" + arch)
                if not lld or not qemu:
                    raise RuntimeError("--execute requires ld.lld and qemu-ARCH on PATH")
                finish, helper_body = {
                    "aarch64": ("mov x8,#93\nsvc #0", "mov x0,#42"),
                    "riscv64": ("addi a7,zero,93\necall", "addi a0,zero,42"),
                    "loongarch64": ("addi.d $a7,$zero,93\nsyscall 0", "addi.d $a0,$zero,42"),
                }[arch]
                input_path.write_text(f".text\n.global _start\n.extern helper\n_start: {call} helper+4\n{finish}\n.data\n.quad helper+8\n.word helper+4\n")
                run([str(binary), "asm", "--" + arch, str(input_path), "-o", str(output_path)])
                helper_source, helper_object, executable = root / "helper.s", root / "helper.o", root / "program"
                helper_source.write_text(f".text\n.global helper\nhelper: nop\n{helper_body}\nret\n")
                run([str(binary), "asm", "--" + arch, str(helper_source), "-o", str(helper_object)])
                run([lld, str(output_path), str(helper_object), "-o", str(executable)])
                data = sections(executable.read_bytes())[".data"]
                if struct.unpack_from("<Q",data)[0] - struct.unpack_from("<I",data,8)[0] != 4:
                    raise AssertionError("linked absolute relocations lost their addends")
                execution = subprocess.run([qemu, str(executable)], capture_output=True, timeout=30)
                if execution.returncode != 42:
                    raise AssertionError(f"{arch}: QEMU exit {execution.returncode}: {execution.stderr!r}")
                print(f"{arch}: LLD two-object call/addend/data relocations + QEMU exit 42", flush=True)
        print(f"{arch}: {len(fixtures)} retained + {args.cases} random instructions and ELF RELA match LLVM", flush=True)


if __name__ == "__main__":
    main()
