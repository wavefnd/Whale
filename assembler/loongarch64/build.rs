// SPDX-License-Identifier: MPL-2.0
const CFG: &str = "whale_wave_loongarch64";

const ENV: &str = "WHALE_WAVE_LOONGARCH64_DIR";

const LIBRARY: &str = "whale_wave_loongarch64";

const METADATA: &str = "wave-loongarch64.meta";

const ROOT_UP: usize = 2;

const SOURCES: &[&str] = &[
    "assembler/model.wave",
    "assembler/declarations.wave",
    "assembler/fixed_model.wave",
    "assembler/fixed_declarations.wave",
    "assembler/loongarch64/model.wave",
    "assembler/loongarch64/registers.wave",
    "assembler/loongarch64/tables.wave",
    "assembler/loongarch64/operands.wave",
    "assembler/loongarch64/formats.wave",
    "assembler/loongarch64/branch.wave",
    "assembler/loongarch64/encoder.wave",
    "assembler/loongarch64/entry.wave",
];

const ABI_SOURCES: &[&str] = &[
    "assembler/loongarch64/src/wave.rs",
    "assembler/loongarch64/src/lib.rs",
    "assembler/loongarch64/src/encoder.rs",
    "assembler/loongarch64/src/parser.rs",
];
include!("../build_support.rs");
