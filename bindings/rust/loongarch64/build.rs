// SPDX-License-Identifier: MPL-2.0
const MODULE: &str = "loongarch64";

const CFG: &str = "whale_wave_loongarch64";

const ENV: &str = "WHALE_WAVE_LOONGARCH64_DIR";

const LIBRARY: &str = "whale_wave_loongarch64";

const SOURCE_ROOTS: &[&str] = &["assembler/loongarch64", "assembler"];
include!("../build_support.rs");
