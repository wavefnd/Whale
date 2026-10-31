// SPDX-License-Identifier: MPL-2.0
const CFG: &str = "whale_wave_riscv64";

const ENV: &str = "WHALE_WAVE_RISCV64_DIR";

const LIBRARY: &str = "whale_wave_riscv64";

const METADATA: &str = "wave-riscv64.meta";

const ROOT_UP: usize = 2;

const SOURCES: &[&str] = &[
    "assembler/model.wave",
    "assembler/declarations.wave",
    "assembler/fixed_model.wave",
    "assembler/fixed_declarations.wave",
    "assembler/riscv64/registers.wave",
    "assembler/riscv64/operands.wave",
    "assembler/riscv64/encoding.wave",
    "assembler/riscv64/encoder.wave",
    "assembler/riscv64/parser.wave",
    "assembler/riscv64/entry.wave",
];

const ABI_SOURCES: &[&str] = &[
    "assembler/riscv64/src/wave.rs",
    "assembler/riscv64/src/lib.rs",
    "assembler/riscv64/src/encoder.rs",
    "assembler/riscv64/src/parser.rs",
];
include!("../build_support.rs");
