use crate::ast::*;
use crate::error::AsmError;
use crate::assembler::{AssemblerOutput, Relocation, RelocKind};
use crate::error::AsmError::ParserError;
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

            ASTNode::Directive(dir) => {
                encode_directive(dir, &mut bytes)?;
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
    if let Operand::Label(regname) = dst {
        if let Some(reg) = lookup_reg(regname, 64) {
            match src {
                Operand::Immediate(val) => {
                    // REX.W + mov rax, imm64 = 0x48 B8 + imm64
                    bytes.push(0x48);
                    bytes.push(0xB8 + reg);

                    let imm = *val as i64;
                    bytes.extend_from_slice(&imm.to_le_bytes());
                    return Ok(());
                }

                Operand::Label(label) => {
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

fn lookup_reg(name: &str, mode: u8) -> Option<u8> {
    let regs = match mode {
        64 => REGISTERS_64,
        32 => REGISTERS_32,
        _ => return None,
    };

    regs.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, code)| *code)
}

fn encode_directive(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    match dir.name.as_str() {
        "db" => encode_db(dir, bytes),
        "dw" => encode_dw(dir, bytes),
        "dd" => encode_dd(dir, bytes),
        "dq" => encode_dq(dir, bytes),
        _ => Err(AsmError::EncodeError(format!("Unknown directive {}", dir.name))),
    }
}

fn encode_db(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    for v in &dir.values {
        match v {
            DirectiveValue::Number(n) => {
                bytes.push(*n as u8);
            }
            DirectiveValue::StringLiteral(s) => {
                for ch in s.bytes() {
                    bytes.push(ch);
                }
            }
            DirectiveValue::Identifier(_) => {
                return Err(AsmError::EncodeError(
                    "db does not support identifier".into(),
                ));
            }
        }
    }
    Ok(())
}


fn encode_dw(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    for v in &dir.values {
        match v {
            DirectiveValue::Number(n) => {
                let val = *n as i16;
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            _ => return Err(AsmError::EncodeError("dw supports only numbers".into())),
        }
    }
    Ok(())
}

fn encode_dd(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    for v in &dir.values {
        match v {
            DirectiveValue::Number(n) => {
                let val = *n as i32;
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            DirectiveValue::Identifier(name) => {
                // relocatable data
                // dd label
                // → reserve 4 bytes and add reloc
                // but for now: not implemented
                return Err(AsmError::EncodeError("dd identifier reloc not implemented yet".into()));
            }
            _ => return Err(AsmError::EncodeError("dd supports only numbers".into())),
        }
    }
    Ok(())
}

fn encode_dq(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    for v in &dir.values {
        match v {
            DirectiveValue::Number(n) => {
                let val = *n as i64;
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            DirectiveValue::Identifier(name) => {
                // relocatable data 8 bytes
                // dd label → reloc 4 bytes, dq label → reloc 8 bytes
                return Err(AsmError::EncodeError("dq identifier reloc not implemented yet".into()));
            }
            _ => return Err(AsmError::EncodeError("dq supports only numbers".into())),
        }
    }
    Ok(())
}
