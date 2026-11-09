// SPDX-License-Identifier: MPL-2.0
const MODULE: &str = "amd64";

const CFG: &str = "whale_wave_amd64";

const ENV: &str = "WHALE_WAVE_AMD64_DIR";

const LIBRARY: &str = "whale_wave_amd64";

const SOURCE_ROOTS: &[&str] = &["assembler/amd64", "assembler"];
include!("../build_support.rs");
