// SPDX-License-Identifier: MPL-2.0
{
    format = 5,
    name = "whale_wave_loongarch64",
    version = "0.1.0",
    targets = { lib = { output = "static", mode = "freestanding", relocation = "pic" } },
    dependencies = [{ name = "whale_wave_assembler", path = ".." }],
    target_config = [
        { triple = "x86_64-pc-windows-msvc", lib = { relocation = "default" } },
        { triple = "aarch64-pc-windows-msvc", lib = { relocation = "default" } }
    ]
}
