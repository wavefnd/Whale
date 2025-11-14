use crate::ast::*;
use crate::error::AsmError;
use crate::assembler::{AssemblerOutput, Relocation, RelocKind};
use crate::isa::amd64::tables::*;

pub fn encode(ast: &AST) -> Result<AssemblerOutput, AsmError> {
    let mut bytes = Vec::new();
    let mut symbols = Vec::new();
    let mut relocs = Vec::new();

    for node in &ast.items {
        match node {
            ASTNode::Label(name) => {
                // label name → offset
                symbols.push((name.clone(), bytes.len()));
            }

            ASTNode::Instruction(ins) => {
                encode_instruction(ins, &mut bytes, &mut relocs)?;
            }

            ASTNode::Directive(_) => {
                // TODO Add DB, DW, DD Later
            }
        }
    }

    Ok(AssemblerOutput { bytes, symbols, relocations: relocs })
}

fn encode_instruction(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {

    match ins.mnemonic.as_str() {
        "mov" => encode_mov(ins, bytes, relocs),
        "nop" => { bytes.push(0x90); Ok(()) }
        _ => Err(AsmError::EncodeError(format!("Unknown mnemonic {}", ins.mnemonic))),
    }
}

fn encode_mov(
    ins: &Instruction,
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Relocation>,
) -> Result<(), AsmError> {

    if ins.operands.len() != 2 {
        return Err(AsmError::EncodeError("mov expects 2 operands".into()));
    }

    let dst = &ins.operands[0];
    let src = &ins.operands[1];

    // mov r64, imm64
    if let Operand::LabelRef(regname) = dst {
        if let Some(reg) = lookup_reg(regname) {
            match src {
                Operand::Immediate(val) => {
                    // REX.W + mov rax, imm64 = 0x48 B8 + imm64
                    bytes.push(0x48);
                    bytes.push(0xB8 + reg);

                    let imm = *val as i64;
                    bytes.extend_from_slice(&imm.to_le_bytes());
                    return Ok(());
                }

                Operand::LabelRef(label) => {
                    // Requires reloc
                    bytes.push(0x48);
                    bytes.push(0xB8 + reg);
                    relocs.push(Relocation {
                        offset: bytes.len(),
                        symbol: label.clone(),
                        kind: RelocKind::Absolute,
                    });
                    bytes.extend_from_slice(&0i64.to_le_bytes());
                    return Ok(());
                }

                _ => {}
            }
        }
    }

    Err(AsmError::EncodeError("Unsupported mov form".into()))
}

fn lookup_reg(name: &str) -> Option<u8> {
    for (n, code) in REGISTERS_64 {
        if *n == name { return Some(*code); }
    }
    None
}
