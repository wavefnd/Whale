// SPDX-License-Identifier: MPL-2.0
{
    format = 5,
    workspace = {
        members = [
            "assembler/asmobj", "assembler", "assembler/amd64",
            "assembler/aarch64", "assembler/riscv64", "assembler/loongarch64", "ir"
        ]
    },
    profiles = { dev = { opt_level = 0 }, release = { opt_level = 2 } },
    build = { reproducible = true }
}
