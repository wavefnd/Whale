"""Validate a built Whale CLI in a clean directory without tools on PATH."""

import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile


def text_section(data):
    if len(data) < 64 or data[:7] != b"\x7fELF\x02\x01\x01":
        raise AssertionError("expected a little-endian ELF64 object")
    if struct.unpack_from("<HH", data, 16) != (1, 62):
        raise AssertionError("expected an AMD64 relocatable object")
    offset = struct.unpack_from("<Q", data, 40)[0]
    entry_size, count, strings_index = struct.unpack_from("<HHH", data, 58)
    if entry_size != 64 or not 0 < strings_index < count or offset + count * 64 > len(data):
        raise AssertionError("invalid ELF section table")
    headers = [struct.unpack_from("<IIQQQQIIQQ", data, offset + i * 64) for i in range(count)]

    def contents(header):
        start, length = header[4:6]
        if start + length > len(data):
            raise AssertionError("ELF section extends past file")
        return data[start:start + length]

    names = contents(headers[strings_index])
    for header in headers:
        name_offset = header[0]
        end = names.find(b"\0", name_offset)
        if name_offset >= len(names) or end < 0:
            raise AssertionError("invalid ELF section name")
        if names[name_offset:end] == b".text":
            return contents(header)
    raise AssertionError("missing .text section")


def smoke(binary, socket, artifacts, emulator=None, sysroot=None):
    binary = binary.resolve(strict=True)
    launcher = [str(binary)]
    if emulator:
        launcher = [str(emulator.resolve(strict=True)), "-L", str(sysroot.resolve(strict=True)), *launcher]
    artifacts.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="whale smoke ") as directory:
        root = Path(directory)
        # Copy the binary away from the source/build tree, including spaces in its path.
        copied = root / binary.name
        shutil.copy2(binary, copied)
        launcher[-1] = str(copied)
        empty_path = root / "empty path"
        empty_path.mkdir()
        environment = dict(os.environ, PATH=str(empty_path))
        transcript = []

        def invoke(args, success=True, diagnostic=None):
            result = subprocess.run(
                [*launcher, *map(str, args)], cwd=root, env=environment,
                capture_output=True, timeout=20,
            )
            transcript.append({"args": list(map(str, args)), "status": result.returncode,
                               "stdout": result.stdout.decode("utf-8", errors="replace"),
                               "stderr": result.stderr.decode("utf-8", errors="replace")})
            if (result.returncode == 0) != success:
                raise AssertionError(f"unexpected exit status: {transcript[-1]}")
            if diagnostic and diagnostic not in result.stderr:
                raise AssertionError(f"missing diagnostic {diagnostic!r}: {transcript[-1]}")
            return result

        def rejected(args, output, diagnostic):
            for existing in (False, True):
                if existing:
                    output.write_bytes(b"existing artifact")
                invoke([*args, "-o", output], success=False, diagnostic=diagnostic)
                if existing:
                    if output.read_bytes() != b"existing artifact":
                        raise AssertionError("failure overwrote the existing output")
                    output.unlink()
                elif output.exists():
                    raise AssertionError("failure created an output")

        try:
            source = root / "valid input.asm"
            source.write_text("section .text\nglobal start\nstart:\nmov eax, 42\nret\n", encoding="utf-8")
            outputs = [root / "first.o", root / "second.o"]
            for output in outputs:
                invoke(["asm", "--amd64", source, "-o", output])
                # Hand-specified ISA bytes: MOV EAX, imm32 followed by RET.
                if text_section(output.read_bytes()) != bytes.fromhex("b8 2a 00 00 00 c3"):
                    raise AssertionError("incorrect machine code")
            if outputs[0].read_bytes() != outputs[1].read_bytes():
                raise AssertionError("object output differs across fresh processes")
            multi_section = root / "symbols and relocations.asm"
            multi_section.write_text(
                "section .text\nextern external\nglobal entry\nentry:\ncall external\nret\n"
                "section .data\nglobal pointers\npointers:\ndd entry\ndd external + 4\n",
                encoding="utf-8",
            )
            reference = None
            for attempt in range(3):
                output = root / f"relocations-{attempt}.o"
                invoke(["asm", "--amd64", multi_section, "-o", output])
                data = output.read_bytes()
                text_section(data)
                for section in (b".data\0", b".rela.text\0", b".rela.data\0"):
                    if section not in data:
                        raise AssertionError(f"missing expected section {section!r}")
                if reference is not None and reference != data:
                    raise AssertionError("multi-section object differs across fresh processes")
                reference = data
            bad_asm = root / "invalid input.asm"
            bad_asm.write_text("mov rax, [rbx\n", encoding="utf-8")
            rejected(["asm", "--amd64", bad_asm], root / "rejected.o", b"closing ']'")

            socket_input = root / "socket input.json"
            program = {"declarations": [], "globals": [], "functions": [{"name": "answer", "convention": "Whale", "linkage": "Internal", "link_name": None, "parameters": [],
                       "return_type": {"Int": {"bits": 32, "signed": True}},
                       "body": [{"Return": {"Lit": {"Int": {
                           "bits": 32, "signed": True, "value": "42"}}}}]}]}
            socket_input.write_text(json.dumps({"format_version": 2, "semantics_version": 1, "features": [], "program": program}), encoding="utf-8")
            if socket == "disabled":
                result = invoke(["ir", "lower", socket_input], success=False, diagnostic=b"requires feature 'socket-cli'")
                if result.returncode != 2:
                    raise AssertionError("feature-disabled CLI must exit with status 2")
            else:
                first = invoke(["ir", "lower", socket_input]).stdout
                if b"answer" not in first or b"42" not in first or not first.strip():
                    raise AssertionError("lowered IR is missing the function or constant")
                if first != invoke(["ir", "lower", socket_input]).stdout:
                    raise AssertionError("IR output differs across fresh processes")
                output = root / "output.wir"
                invoke(["ir", "lower", socket_input, "-o", output])
                if output.read_bytes() != first:
                    raise AssertionError("file and stdout IR differ")
                multi_program = json.loads(json.dumps(program))
                multi_program["globals"] = [
                    {"name": "constant", "ty": {"Int": {"bits": 32, "signed": True}},
                     "init": {"Lit": {"Int": {"bits": 32, "signed": True, "value": "7"}}}},
                    {"name": "alias", "ty": {"Int": {"bits": 32, "signed": True}},
                     "init": {"Var": "constant"}},
                ]
                multi_program["functions"].append({
                    "name": "from_global", "convention": "Whale", "linkage": "Internal", "link_name": None, "parameters": [],
                    "return_type": {"Int": {"bits": 32, "signed": True}},
                    "body": [{"Return": {"Var": "alias"}}],
                })
                multi_json = root / "multiple functions.json"
                multi_json.write_text(json.dumps({"format_version": 2, "semantics_version": 1, "features": [], "program": multi_program}), encoding="utf-8")
                reference = None
                for _ in range(3):
                    data = invoke(["ir", "lower", multi_json]).stdout
                    for name in (b"answer", b"from_global", b"constant", b"alias"):
                        if name not in data:
                            raise AssertionError(f"missing IR declaration {name!r}")
                    if reference is not None and reference != data:
                        raise AssertionError("multi-function IR differs across fresh processes")
                    reference = data
                bad_json = root / "malformed.json"
                bad_json.write_text("{", encoding="utf-8")
                rejected(["ir", "lower", bad_json], root / "rejected.wir", b"Failed to parse socket JSON")
                program["functions"][0]["body"][0]["Return"] = {"Lit": {"Bool": True}}
                invalid_program = root / "type mismatch.json"
                invalid_program.write_text(json.dumps({"format_version": 2, "semantics_version": 1, "features": [], "program": program}), encoding="utf-8")
                rejected(["ir", "lower", invalid_program], root / "rejected.wir", b"lower_o0 failed")
        except Exception:
            repro = artifacts / "smoke-repro"
            repro.mkdir(exist_ok=True)
            for path in root.iterdir():
                if path.is_file() and path != copied:
                    shutil.copy2(path, repro / path.name)
            raise
        finally:
            (artifacts / "smoke-transcript.json").write_text(
                json.dumps(transcript, indent=2) + "\n", encoding="utf-8",
            )
    print(f"Standalone CLI smoke passed (socket {socket}, empty PATH).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--socket", choices=["enabled", "disabled"], required=True)
    parser.add_argument("--artifacts", type=Path, default=Path(".ci-artifacts"))
    parser.add_argument("--emulator", type=Path)
    parser.add_argument("--sysroot", type=Path)
    args = parser.parse_args()
    if bool(args.emulator) != bool(args.sysroot):
        parser.error("--emulator and --sysroot must be supplied together")
    smoke(args.binary, args.socket, args.artifacts, args.emulator, args.sysroot)


if __name__ == "__main__":
    main()
