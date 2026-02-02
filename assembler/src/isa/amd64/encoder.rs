use crate::ast::*;
use crate::error::AsmError;
use crate::assembler::{AssemblerOutput, AsmSection, AsmSymbol, Relocation, RelocKind};
use crate::isa::amd64::encoding::{ModRM, REX, encode_address, EncodedAddress, DispKind};
use crate::isa::amd64::tables::*;

pub fn encode(ast: &AST) -> Result<AssemblerOutput, AsmError> {
    let mut sections = Vec::new();
    let mut symbols = Vec::new();

    // Default .text section
    sections.push(AsmSection {
        name: ".text".to_string(),
        data: Vec::new(),
        relocs: Vec::new(),
    });

    let mut current_section_idx = 0;
    let mut global_symbols = Vec::new();

    for node in &ast.items {
        match node {
            ASTNode::Section(name) => {
                if let Some(idx) = sections.iter().position(|s| s.name == *name) {
                    current_section_idx = idx;
                } else {
                    sections.push(AsmSection {
                        name: name.clone(),
                        data: Vec::new(),
                        relocs: Vec::new(),
                    });
                    current_section_idx = sections.len() - 1;
                }
            }

            ASTNode::Global(name) => {
                global_symbols.push(name.clone());
            }

            ASTNode::Extern(name) => {
                symbols.push(AsmSymbol {
                    name: name.clone(),
                    section_index: None,
                    offset: 0,
                    is_global: true,
                });
            }

            ASTNode::Label(name) => {
                symbols.push(AsmSymbol {
                    name: name.clone(),
                    section_index: Some(current_section_idx),
                    offset: sections[current_section_idx].data.len(),
                    is_global: global_symbols.contains(name),
                });
            }

            ASTNode::Instruction(ins) => {
                let sec = &mut sections[current_section_idx];
                encode_instruction(ins, &mut sec.data, &mut sec.relocs)?;
            }

            ASTNode::Directive(dir) => {
                let sec = &mut sections[current_section_idx];
                encode_directive(dir, &mut sec.data)?;
            }
        }
    }

    Ok(AssemblerOutput { sections, symbols })
}

struct RegInfo { code: u8, width: u8 }

fn lookup_reg(name: &str) -> Option<RegInfo> {
    if let Some((_, code)) = REGISTERS_64.iter().find(|(n, _)| *n == name) { return Some(RegInfo { code: *code, width: 64 }); }
    if let Some((_, code)) = REGISTERS_32.iter().find(|(n, _)| *n == name) { return Some(RegInfo { code: *code, width: 32 }); }
    if let Some((_, code)) = REGISTERS_16.iter().find(|(n, _)| *n == name) { return Some(RegInfo { code: *code, width: 16 }); }
    if let Some((_, code)) = REGISTERS_8.iter().find(|(n, _)| *n == name) { return Some(RegInfo { code: *code, width: 8 }); }
    None
}

fn encode_instruction(ins: &Instruction, bytes: &mut Vec<u8>, relocs: &mut Vec<Relocation>) -> Result<(), AsmError> {
    match ins.mnemonic.as_str() {
        "mov" => encode_mov(ins, bytes, relocs),
        "add" => encode_binop(ins, 0x01, 0x03, 0, bytes, relocs),
        "sub" => encode_binop(ins, 0x29, 0x2B, 5, bytes, relocs),
        "and" => encode_binop(ins, 0x21, 0x23, 4, bytes, relocs),
        "or"  => encode_binop(ins, 0x09, 0x0B, 1, bytes, relocs),
        "xor" => encode_binop(ins, 0x31, 0x33, 6, bytes, relocs),
        "cmp" => encode_binop(ins, 0x39, 0x3B, 7, bytes, relocs),
        "push" => encode_push_pop(ins, 0x50, bytes),
        "pop"  => encode_push_pop(ins, 0x58, bytes),
        "jmp"  => encode_jump(ins, 0xE9, bytes, relocs),
        "call" => encode_jump(ins, 0xE8, bytes, relocs),
        "ret"  => { bytes.push(0xC3); Ok(()) },
        "nop"  => { bytes.push(0x90); Ok(()) },
        "syscall" => { bytes.push(0x0F); bytes.push(0x05); Ok(()) },
        "int3" => { bytes.push(0xCC); Ok(()) },
        _ => Err(AsmError::EncodeError(format!("Unknown mnemonic {}", ins.mnemonic))),
    }
}

fn write_rex_modrm_addr(bytes: &mut Vec<u8>, opcode: u8, reg_code: u8, reg_width: u8, addr: EncodedAddress) {
    let mut rex = REX::new();
    rex.w = reg_width == 64; rex.r = reg_code >= 8; rex.b = addr.rex_b; rex.x = addr.rex_x;
    if rex.w || rex.r || rex.b || rex.x { bytes.push(rex.encode()); }
    bytes.push(opcode);
    bytes.push(ModRM::new(addr.mod_bits, reg_code, addr.rm_bits).encode());
    if let Some(sib) = addr.sib { bytes.push((sib.0 << 6) | (sib.1 << 3) | sib.2); }
    if let Some(disp) = addr.disp {
        match disp { DispKind::Disp8(d) => bytes.push(d as u8), DispKind::Disp32(d) => bytes.extend_from_slice(&d.to_le_bytes()) }
    }
}

fn encode_mov(ins: &Instruction, bytes: &mut Vec<u8>, relocs: &mut Vec<Relocation>) -> Result<(), AsmError> {
    if ins.operands.len() != 2 { return Err(AsmError::EncodeError("mov expects 2 operands".into())); }
    let dst = &ins.operands[0];
    let src = &ins.operands[1];

    match (dst, src) {
        (Operand::Register(r_name), Operand::Immediate(imm)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            if reg.width == 64 {
                let mut rex = REX::new(); rex.w = true; rex.b = reg.code >= 8;
                bytes.push(rex.encode()); bytes.push(0xB8 + (reg.code & 7));
                bytes.extend_from_slice(&imm.to_le_bytes());
            } else if reg.width == 32 {
                if reg.code >= 8 { let mut rex = REX::new(); rex.b = true; bytes.push(rex.encode()); }
                bytes.push(0xB8 + (reg.code & 7)); bytes.extend_from_slice(&(*imm as i32).to_le_bytes());
            } else { return Err(AsmError::EncodeError("Unsupported mov width for imm".into())); }
            Ok(())
        }
        (Operand::Register(r_name), Operand::Label(label)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            if reg.width == 64 {
                let mut rex = REX::new(); rex.w = true; rex.b = reg.code >= 8;
                bytes.push(rex.encode()); bytes.push(0xB8 + (reg.code & 7));
                relocs.push(Relocation { offset: bytes.len(), symbol: label.clone(), kind: RelocKind::Absolute64, addend: 0 });
                bytes.extend_from_slice(&0i64.to_le_bytes());
                Ok(())
            } else { Err(AsmError::EncodeError("Label move only supported for r64".into())) }
        }
        (Operand::Register(dst_name), Operand::Register(src_name)) => {
            let dst_reg = lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            let src_reg = lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid src register".into()))?;
            if dst_reg.width != src_reg.width { return Err(AsmError::EncodeError("Register width mismatch".into())); }
            let mut rex = REX::new(); rex.w = dst_reg.width == 64; rex.r = src_reg.code >= 8; rex.b = dst_reg.code >= 8;
            if rex.w || rex.r || rex.b { bytes.push(rex.encode()); }
            bytes.push(0x89); bytes.push(ModRM::new(0b11, src_reg.code, dst_reg.code).encode());
            Ok(())
        }
        (Operand::Register(r_name), Operand::Memory(mem)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let addr = encode_address(mem, 64)?;
            write_rex_modrm_addr(bytes, 0x8B, reg.code, reg.width, addr);
            Ok(())
        }
        (Operand::Memory(mem), Operand::Register(r_name)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let addr = encode_address(mem, 64)?;
            write_rex_modrm_addr(bytes, 0x89, reg.code, reg.width, addr);
            Ok(())
        }
        _ => Err(AsmError::EncodeError("Unsupported mov form".into())),
    }
}

fn encode_binop(ins: &Instruction, opcode_rm_r: u8, opcode_r_rm: u8, imm_op_ext: u8, bytes: &mut Vec<u8>, _relocs: &mut Vec<Relocation>) -> Result<(), AsmError> {
    if ins.operands.len() != 2 { return Err(AsmError::EncodeError(format!("{} expects 2 operands", ins.mnemonic))); }
    let dst = &ins.operands[0];
    let src = &ins.operands[1];
    match (dst, src) {
        (Operand::Register(dst_name), Operand::Register(src_name)) => {
            let dst_reg = lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            let src_reg = lookup_reg(src_name).ok_or(AsmError::EncodeError("Invalid src register".into()))?;
            let mut rex = REX::new(); rex.w = dst_reg.width == 64; rex.r = src_reg.code >= 8; rex.b = dst_reg.code >= 8;
            if rex.w || rex.r || rex.b { bytes.push(rex.encode()); }
            bytes.push(opcode_rm_r); bytes.push(ModRM::new(0b11, src_reg.code, dst_reg.code).encode());
            Ok(())
        }
        (Operand::Register(dst_name), Operand::Immediate(imm)) => {
            let dst_reg = lookup_reg(dst_name).ok_or(AsmError::EncodeError("Invalid dst register".into()))?;
            let mut rex = REX::new(); rex.w = dst_reg.width == 64; rex.b = dst_reg.code >= 8;
            if rex.w || rex.b { bytes.push(rex.encode()); }
            if (-128..=127).contains(imm) {
                bytes.push(0x83); bytes.push(ModRM::new(0b11, imm_op_ext, dst_reg.code).encode()); bytes.push(*imm as u8);
            } else {
                bytes.push(0x81); bytes.push(ModRM::new(0b11, imm_op_ext, dst_reg.code).encode()); bytes.extend_from_slice(&(*imm as i32).to_le_bytes());
            }
            Ok(())
        }
        (Operand::Register(r_name), Operand::Memory(mem)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let addr = encode_address(mem, 64)?;
            write_rex_modrm_addr(bytes, opcode_r_rm, reg.code, reg.width, addr);
            Ok(())
        }
        (Operand::Memory(mem), Operand::Register(r_name)) => {
            let reg = lookup_reg(r_name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
            let addr = encode_address(mem, 64)?;
            write_rex_modrm_addr(bytes, opcode_rm_r, reg.code, reg.width, addr);
            Ok(())
        }
        _ => Err(AsmError::EncodeError(format!("Unsupported {} form", ins.mnemonic))),
    }
}

fn encode_push_pop(ins: &Instruction, base_opcode: u8, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    if ins.operands.len() != 1 { return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic))); }
    if let Operand::Register(name) = &ins.operands[0] {
        let reg = lookup_reg(name).ok_or(AsmError::EncodeError("Invalid register".into()))?;
        if reg.code >= 8 { let mut rex = REX::new(); rex.b = true; bytes.push(rex.encode()); }
        bytes.push(base_opcode + (reg.code & 7));
        Ok(())
    } else { Err(AsmError::EncodeError(format!("{} only supports registers for now", ins.mnemonic))) }
}

fn encode_jump(ins: &Instruction, opcode: u8, bytes: &mut Vec<u8>, relocs: &mut Vec<Relocation>) -> Result<(), AsmError> {
    if ins.operands.len() != 1 { return Err(AsmError::EncodeError(format!("{} expects 1 operand", ins.mnemonic))); }
    match &ins.operands[0] {
        Operand::Label(label) => {
            bytes.push(opcode);
            relocs.push(Relocation { offset: bytes.len(), symbol: label.clone(), kind: RelocKind::Relative32, addend: -4 });
            bytes.extend_from_slice(&0i32.to_le_bytes());
            Ok(())
        }
        _ => Err(AsmError::EncodeError(format!("{} only supports labels for now", ins.mnemonic))),
    }
}

fn encode_directive(dir: &Directive, bytes: &mut Vec<u8>) -> Result<(), AsmError> {
    match dir.name.as_str() {
        "db" => { for v in &dir.values { match v { DirectiveValue::Number(n) => bytes.push(*n as u8), DirectiveValue::StringLiteral(s) => bytes.extend_from_slice(s.as_bytes()), _ => return Err(AsmError::EncodeError("Unsupported db value".into())) } } Ok(()) }
        "dw" => { for v in &dir.values { if let DirectiveValue::Number(n) = v { bytes.extend_from_slice(&(*n as i16).to_le_bytes()); } else { return Err(AsmError::EncodeError("dw only supports numbers".into())); } } Ok(()) }
        "dd" => { for v in &dir.values { if let DirectiveValue::Number(n) = v { bytes.extend_from_slice(&(*n as i32).to_le_bytes()); } else { return Err(AsmError::EncodeError("dd only supports numbers".into())); } } Ok(()) }
        "dq" => { for v in &dir.values { if let DirectiveValue::Number(n) = v { bytes.extend_from_slice(&(*n as i64).to_le_bytes()); } else { return Err(AsmError::EncodeError("dq only supports numbers".into())); } } Ok(()) }
        _ => Err(AsmError::EncodeError(format!("Unknown directive {}", dir.name))),
    }
}
