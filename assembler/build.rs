// SPDX-License-Identifier: MPL-2.0
const CFG: &str = "whale_wave_assembler";

const ENV: &str = "WHALE_WAVE_ASSEMBLER_DIR";

const LIBRARY: &str = "whale_wave_assembler";

const METADATA: &str = "wave-assembler.meta";

const ROOT_UP: usize = 1;

const SOURCES: &[&str] = &[
    "assembler/model.wave",
    "assembler/unicode.wave",
    "assembler/numeric.wave",
    "assembler/output.wave",
    "assembler/lexer.wave",
    "assembler/fixed_model.wave",
    "assembler/fixed_text.wave",
    "assembler/fixed_numbers.wave",
    "assembler/fixed_operands.wave",
    "assembler/fixed_namespace.wave",
    "assembler/fixed_relative.wave",
    "assembler/fixed_scalar.wave",
    "assembler/fixed_source.wave",
    "assembler/fixed_driver.wave",
];

const ABI_SOURCES: &[&str] = &[
    "assembler/src/tokens.rs",
    "assembler/src/tokens_wave.rs",
    "assembler/src/fixed.rs",
    "assembler/src/fixed_wave.rs",
    "assembler/src/lib.rs",
];
include!("build_support.rs");
