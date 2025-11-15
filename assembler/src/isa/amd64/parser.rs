use crate::tokens::{Token, TokenKind};
use crate::ast::*;
use crate::error::AsmError;

pub fn parse(tokens: &[Token]) -> Result<AST, AsmError> {
    let mut pos = 0;
    let mut items = Vec::new();

    while pos < tokens.len() {
        let token = &tokens[pos];

        match &token.kind {
            // label: identifier :
            TokenKind::Identifier(name) => {
                if pos + 1 < tokens.len() {
                    if let TokenKind::Colon = tokens[pos + 1].kind {
                        items.push(ASTNode::Label(name.clone()));
                        pos += 2;
                        continue;
                    }
                }

                // instruction start
                items.push(parse_instruction(tokens, &mut pos)?);
            }

            // skip newlines or unknown tokens for now
            _ => { pos += 1; }
        }
    }

    Ok(AST { items })
}

fn parse_instruction(tokens: &[Token], pos: &mut usize) -> Result<ASTNode, AsmError> {
    let mnemonic = match &tokens[*pos].kind {
        TokenKind::Identifier(s) => s.clone(),
        _ => return Err(AsmError::ParserError("Expected mnemonic".into())),
    };
    *pos += 1;

    // parse operands (simple: reg, imm, label)
    let mut operands = Vec::new();

    loop {
        if *pos >= tokens.len() { break; }

        match &tokens[*pos].kind {
            TokenKind::Newline => break,
            TokenKind::Number(n) => {
                operands.push(Operand::Immediate(*n));
                *pos += 1;
            }

            TokenKind::Identifier(name) => {
                operands.push(Operand::LabelRef(name.clone()));
                *pos += 1;
            }

            TokenKind::Comma => {
                *pos += 1;
                continue;
            }

            TokenKind::Colon => break,
            _ => break,
        }
    }
    
    while *pos < tokens.len() {
        match tokens[*pos].kind {
            TokenKind::Newline => { *pos += 1; }
            _ => break
        }
    }

    Ok(ASTNode::Instruction(Instruction {
        mnemonic,
        operands,
    }))
}
