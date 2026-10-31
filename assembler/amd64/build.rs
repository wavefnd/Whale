// SPDX-License-Identifier: MPL-2.0
const CFG: &str = "whale_wave_amd64";

const ENV: &str = "WHALE_WAVE_AMD64_DIR";

const LIBRARY: &str = "whale_wave_amd64";

const METADATA: &str = "wave-amd64.meta";

const ROOT_UP: usize = 2;

const SOURCES: &[&str] = &[
    "assembler/model.wave",
    "assembler/declarations.wave",
    "assembler/ast_model.wave",
    "assembler/amd64/parser_model.wave",
    "assembler/amd64/registers.wave",
    "assembler/amd64/expressions.wave",
    "assembler/amd64/operands.wave",
    "assembler/amd64/directives.wave",
    "assembler/amd64/parser.wave",
    "assembler/amd64/encoder_model.wave",
    "assembler/amd64/namespace.wave",
    "assembler/amd64/layout.wave",
    "assembler/amd64/address.wave",
    "assembler/amd64/emit.wave",
    "assembler/amd64/integer.wave",
    "assembler/amd64/sse.wave",
    "assembler/amd64/branch.wave",
    "assembler/amd64/directives_encode.wave",
    "assembler/amd64/encoder.wave",
];

const ABI_SOURCES: &[&str] = &[
    "assembler/amd64/src/ast_model.rs",
    "assembler/amd64/src/parser_wave.rs",
    "assembler/amd64/src/encoder_wave.rs",
    "assembler/amd64/src/encoder_namespace_errors.rs",
    "assembler/amd64/src/tables.rs",
    "assembler/amd64/src/encoder.rs",
    "assembler/amd64/src/encoding/address.rs",
    "assembler/amd64/src/encoding/modrm.rs",
    "assembler/amd64/src/encoding/rex.rs",
    "assembler/amd64/src/encoding/sib.rs",
    "assembler/amd64/src/lib.rs",
];
include!("../build_support.rs");
