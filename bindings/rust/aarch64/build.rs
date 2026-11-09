// SPDX-License-Identifier: MPL-2.0
const MODULE: &str = "aarch64";

const CFG: &str = "whale_wave_aarch64";

const ENV: &str = "WHALE_WAVE_AARCH64_DIR";

const LIBRARY: &str = "whale_wave_aarch64";

const SOURCE_ROOTS: &[&str] = &["assembler/aarch64", "assembler"];
include!("../build_support.rs");
