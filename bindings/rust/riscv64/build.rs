// SPDX-License-Identifier: MPL-2.0
const MODULE: &str = "riscv64";

const CFG: &str = "whale_wave_riscv64";

const ENV: &str = "WHALE_WAVE_RISCV64_DIR";

const LIBRARY: &str = "whale_wave_riscv64";

const SOURCE_ROOTS: &[&str] = &["assembler/riscv64", "assembler"];
include!("../build_support.rs");
