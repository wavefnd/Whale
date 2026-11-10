// SPDX-License-Identifier: MPL-2.0
//! AMD64 assembly encoding. Product semantics execute in the pinned Wave module.
use assembler::{ast::AST, AsmError, AssemblerOutput, AssemblyLimits};

#[cfg(any(test, not(whale_wave_amd64)))]
#[path = "encoder_reference.rs"]
mod reference;

#[cfg(whale_wave_amd64)]
#[path = "encoder_wave.rs"]
pub(crate) mod wave;

#[cfg(whale_wave_amd64)]
#[path = "encoder_namespace_errors.rs"]
mod namespace_errors;

pub fn encode(ast: &AST) -> Result<AssemblerOutput, AsmError> {
    encode_with_limits(ast, &AssemblyLimits::default())
}

pub fn encode_with_limits(ast: &AST, limits: &AssemblyLimits) -> Result<AssemblerOutput, AsmError> {
    #[cfg(whale_wave_amd64)]
    return wave::encode(ast, limits);
    #[cfg(not(whale_wave_amd64))]
    reference::encode_with_limits(ast, limits)
}

#[cfg(all(test, whale_wave_amd64))]
mod tests {
    use super::*;

    fn snapshot(output: AssemblerOutput) -> String {
        let mut value = String::new();
        for section in output.sections {
            value.push_str(&format!(
                "{:?}|{}|{}|{}|{:?}|",
                section.kind, section.name, section.align, section.zero_fill, section.data
            ));
            for relocation in section.relocs {
                value.push_str(&format!(
                    "{}|{}|{:?}|{}|",
                    relocation.offset, relocation.symbol, relocation.kind, relocation.addend
                ));
            }
        }
        for symbol in output.symbols {
            value.push_str(&format!(
                "{}|{:?}|{}|{}|{:?}|{}|{:?}|",
                symbol.name,
                symbol.section_index,
                symbol.offset,
                symbol.is_global,
                symbol.kind,
                symbol.size,
                symbol.visibility
            ));
        }
        value
    }

    #[test]
    fn actual_wave_encoder_matches_retained_reference_for_existing_source_fixtures() {
        for source in [
            include_str!("../test/test1.asm"),
            include_str!("../test/mem_test.asm"),
            include_str!("../test/competitor_test.asm"),
            include_str!("../test/full_test.asm"),
            include_str!("../test/msg.asm"),
        ] {
            let tokens = assembler::tokens::tokenize(source).unwrap();
            let ast = crate::parser::parse(&tokens).unwrap();
            let actual = encode(&ast);
            let expected = reference::encode_with_limits(&ast, &AssemblyLimits::default());
            match (actual, expected) {
                (Ok(actual), Ok(expected)) => assert_eq!(snapshot(actual), snapshot(expected)),
                (Err(_), Err(_)) => {},
                (actual, expected) => panic!(
                    "status differs: Wave={}, reference={}",
                    actual.is_ok(),
                    expected.is_ok()
                ),
            }
        }
    }

    #[test]
    fn full_gpr_sib_and_scalar_sse_matrix_matches_independent_reference() {
        for base in crate::tables::REGISTERS_64.iter().map(|(name, _)| *name) {
            for index in crate::tables::REGISTERS_64.iter().map(|(name, _)| *name) {
                for scale in [1, 2, 4, 8] {
                    for displacement in [-129, -128, 0, 127, 128] {
                        let tail = if displacement < 0 {
                            format!(" - {}", -displacement)
                        } else {
                            format!(" + {displacement}")
                        };
                        let source = format!("mov r9, [{base} + {index}*{scale}{tail}]\nmovsd xmm12, [{base} + {index}*{scale}{tail}]\n");
                        let tokens = assembler::tokens::tokenize(&source).unwrap();
                        let ast = match crate::parser::parse(&tokens) {
                            Ok(ast) => ast,
                            Err(_) => continue,
                        };
                        match (
                            encode(&ast),
                            reference::encode_with_limits(&ast, &AssemblyLimits::default()),
                        ) {
                            (Ok(actual), Ok(expected)) => {
                                assert_eq!(snapshot(actual), snapshot(expected), "{source}")
                            },
                            (Err(_), Err(_)) => {},
                            (actual, expected) => panic!(
                                "{source}: Wave={}, reference={}",
                                actual.is_ok(),
                                expected.is_ok()
                            ),
                        }
                    }
                }
            }
        }
    }
}
