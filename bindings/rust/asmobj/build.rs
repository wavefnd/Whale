// SPDX-License-Identifier: MPL-2.0
const MODULE: &str = "asmobj";

const CFG: &str = "whale_asmobj";

const ENV: &str = "WHALE_ASMOBJ_DIR";

const LIBRARY: &str = "whale_asmobj";

const SOURCE_ROOTS: &[&str] = &["assembler/asmobj"];
include!("../build_support.rs");
