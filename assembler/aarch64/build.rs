// SPDX-License-Identifier: MPL-2.0
const CFG: &str = "whale_wave_aarch64";

const ENV: &str = "WHALE_WAVE_AARCH64_DIR";

const LIBRARY: &str = "whale_wave_aarch64";

const METADATA: &str = "wave-aarch64.meta";

const ROOT_UP: usize = 2;

const SOURCES: &[&str] = &[
    "assembler/model.wave",
    "assembler/declarations.wave",
    "assembler/fixed_model.wave",
    "assembler/fixed_declarations.wave",
    "assembler/aarch64/registers.wave",
    "assembler/aarch64/parser.wave",
    "assembler/aarch64/operands.wave",
    "assembler/aarch64/encoding.wave",
    "assembler/aarch64/encoder.wave",
    "assembler/aarch64/entry.wave",
];

const ABI_SOURCES: &[&str] = &[
    "assembler/aarch64/src/wave.rs",
    "assembler/aarch64/src/lib.rs",
    "assembler/aarch64/src/encoder.rs",
    "assembler/aarch64/src/parser.rs",
];
include!("../build_support.rs");
