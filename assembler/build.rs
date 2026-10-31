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
];

const ABI_SOURCES: &[&str] = &["assembler/src/tokens.rs", "assembler/src/tokens_wave.rs"];
include!("build_support.rs");
