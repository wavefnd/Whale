// SPDX-License-Identifier: MPL-2.0
use super::{syntax::Parser, ParseError};
use crate::*;
type ParsedInstruction = (Instruction, Option<(ValueId, Type)>);
impl Parser<'_> {
    pub(super) fn instruction(&mut self) -> Result<ParsedInstruction, ParseError> {
        let result = if self.peek().starts_with("%v") {
            let id = self.value()?;
            self.expect(":")?;
            let ty = self.ty(0)?;
            self.expect("=")?;
            Some((id, ty))
        } else {
            None
        };
        let op_location = self.location();
        let op = self.word()?;
        // Every value-producing instruction requires an explicit result. Stores,
        // memory copies and void calls cannot silently acquire a destination.
        let dst = result.as_ref().map(|r| r.0);
        let required = || dst.ok_or_else(|| self.error("instruction requires a result"));
        let ins = match op {
            "const_decl" => {
                let dst = required()?;
                let name = self.string()?;
                let expression = self.expression(0)?;
                self.expect("=>")?;
                self.expect("const")?;
                let ty = self.ty(0)?;
                self.equal_type(&ty, &expression.ty)?;
                let value = self.literal(&ty)?;
                Instruction::ConstDecl {
                    dst,
                    name,
                    expression,
                    value,
                }
            }
            "const" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let value = self.literal(&ty)?;
                Instruction::Const { dst, ty, value }
            }
            "undef" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                Instruction::Undef { dst, ty }
            }
            "mov" | "not" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let src = self.value()?;
                if op == "mov" {
                    Instruction::Mov { dst, ty, src }
                } else {
                    Instruction::Not { dst, ty, src }
                }
            }
            "add" | "sub" | "mul" | "udiv" | "sdiv" | "urem" | "srem" | "fadd" | "fsub"
            | "fmul" | "fdiv" | "frem" | "and" | "or" | "xor" | "shl" | "lshr" | "ashr" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let lhs = self.value()?;
                self.comma()?;
                let rhs = self.value()?;
                let op = match op {
                    "add" => BinOp::Add,
                    "sub" => BinOp::Sub,
                    "mul" => BinOp::Mul,
                    "udiv" => BinOp::UDiv,
                    "sdiv" => BinOp::SDiv,
                    "urem" => BinOp::URem,
                    "srem" => BinOp::SRem,
                    "fadd" => BinOp::FAdd,
                    "fsub" => BinOp::FSub,
                    "fmul" => BinOp::FMul,
                    "fdiv" => BinOp::FDiv,
                    "frem" => BinOp::FRem,
                    "and" => BinOp::And,
                    "or" => BinOp::Or,
                    "xor" => BinOp::Xor,
                    "shl" => BinOp::Shl,
                    "lshr" => BinOp::LShr,
                    _ => BinOp::AShr,
                };
                Instruction::Bin {
                    dst,
                    op,
                    ty,
                    lhs,
                    rhs,
                }
            }
            "cmp" | "icmp" | "fcmp" => {
                let dst = required()?;
                let pred = self.word()?;
                let ty = self.ty(0)?;
                let lhs = self.value()?;
                self.comma()?;
                let rhs = self.value()?;
                match op {
                    "cmp" => Instruction::Cmp {
                        dst,
                        op: match pred {
                            "eq" => CmpOp::Eq,
                            "ne" => CmpOp::Ne,
                            "slt" => CmpOp::SLt,
                            "sle" => CmpOp::SLe,
                            "sgt" => CmpOp::SGt,
                            "sge" => CmpOp::SGe,
                            "ult" => CmpOp::ULt,
                            "ule" => CmpOp::ULe,
                            "ugt" => CmpOp::UGt,
                            "uge" => CmpOp::UGe,
                            "feq" => CmpOp::FEq,
                            "fne" => CmpOp::FNe,
                            "flt" => CmpOp::FLt,
                            "fle" => CmpOp::FLe,
                            "fgt" => CmpOp::FGt,
                            "fge" => CmpOp::FGe,
                            _ => return Err(self.error("unknown comparison predicate")),
                        },
                        ty,
                        lhs,
                        rhs,
                    },
                    "icmp" => Instruction::ICmp {
                        dst,
                        pred: match pred {
                            "eq" => ICmpPred::Eq,
                            "ne" => ICmpPred::Ne,
                            "slt" => ICmpPred::Slt,
                            "sle" => ICmpPred::Sle,
                            "sgt" => ICmpPred::Sgt,
                            "sge" => ICmpPred::Sge,
                            "ult" => ICmpPred::Ult,
                            "ule" => ICmpPred::Ule,
                            "ugt" => ICmpPred::Ugt,
                            "uge" => ICmpPred::Uge,
                            _ => return Err(self.error("unknown integer predicate")),
                        },
                        ty,
                        lhs,
                        rhs,
                    },
                    _ => Instruction::FCmp {
                        dst,
                        pred: match pred {
                            "oeq" => FCmpPred::Oeq,
                            "one" => FCmpPred::One,
                            "olt" => FCmpPred::Olt,
                            "ole" => FCmpPred::Ole,
                            "ogt" => FCmpPred::Ogt,
                            "oge" => FCmpPred::Oge,
                            "ord" => FCmpPred::Ord,
                            "uno" => FCmpPred::Uno,
                            "ueq" => FCmpPred::Ueq,
                            "une" => FCmpPred::Une,
                            "ult" => FCmpPred::Ult,
                            "ule" => FCmpPred::Ule,
                            "ugt" => FCmpPred::Ugt,
                            "uge" => FCmpPred::Uge,
                            _ => return Err(self.error("unknown float predicate")),
                        },
                        ty,
                        lhs,
                        rhs,
                    },
                }
            }
            "select" => {
                let dst = required()?;
                self.expect("bool")?;
                let cond = self.value()?;
                self.comma()?;
                let ty = self.ty(0)?;
                let on_true = self.value()?;
                self.comma()?;
                let other = self.ty(0)?;
                self.equal_type(&ty, &other)?;
                let on_false = self.value()?;
                Instruction::Select {
                    dst,
                    ty,
                    cond,
                    on_true,
                    on_false,
                }
            }
            "zext" | "sext" | "trunc" | "fext" | "ftrunc" | "itof_s" | "itof_u" | "ftoi_s"
            | "ftoi_u" | "bitcast" | "ptrtoint" | "inttoptr" => {
                let dst = required()?;
                let src_ty = self.ty(0)?;
                let src = self.value()?;
                self.expect("to")?;
                let dst_ty = self.ty(0)?;
                let op = match op {
                    "zext" => CastOp::ZExt,
                    "sext" => CastOp::SExt,
                    "trunc" => CastOp::Trunc,
                    "fext" => CastOp::FExt,
                    "ftrunc" => CastOp::FTrunc,
                    "itof_s" => CastOp::IToF_S,
                    "itof_u" => CastOp::IToF_U,
                    "ftoi_s" => CastOp::FToI_S,
                    "ftoi_u" => CastOp::FToI_U,
                    "bitcast" => CastOp::Bitcast,
                    "ptrtoint" => CastOp::PtrToInt,
                    _ => CastOp::IntToPtr,
                };
                Instruction::Cast {
                    dst,
                    op,
                    dst_ty,
                    src_ty,
                    src,
                }
            }
            "phi" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let mut incomings = Vec::new();
                if self.peek() == "[" {
                    loop {
                        self.node()?;
                        self.expect("[")?;
                        let v = self.value()?;
                        self.comma()?;
                        let b = self.block_id()?;
                        self.expect("]")?;
                        incomings.push((v, b));
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                Instruction::Phi { dst, ty, incomings }
            }
            "extract" => {
                let dst = required()?;
                let dst_ty = result.as_ref().unwrap().1.clone();
                let tuple = self.value()?;
                self.comma()?;
                let index = self.number()?;
                Instruction::Extract {
                    dst,
                    dst_ty,
                    tuple,
                    index,
                }
            }
            "uadd_chk" | "usub_chk" | "umul_chk" | "sadd_chk" | "ssub_chk" | "smul_chk" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let lhs = self.value()?;
                self.comma()?;
                let rhs = self.value()?;
                let op = match op {
                    "uadd_chk" => CheckedOp::UAdd,
                    "usub_chk" => CheckedOp::USub,
                    "umul_chk" => CheckedOp::UMul,
                    "sadd_chk" => CheckedOp::SAdd,
                    "ssub_chk" => CheckedOp::SSub,
                    _ => CheckedOp::SMul,
                };
                Instruction::Checked {
                    dst,
                    op,
                    ty,
                    lhs,
                    rhs,
                }
            }
            "alloca" => {
                let dst = required()?;
                let ty = self.ty(0)?;
                let align = self.align()?;
                Instruction::Alloca { dst, ty, align }
            }
            "load" | "store" => {
                let ty = self.ty(0)?;
                let value = if op == "store" {
                    Some(self.value()?)
                } else {
                    None
                };
                self.comma()?;
                let ptr_ty = self.ty(0)?;
                self.equal_type(&Type::ptr_to(ty.clone()), &ptr_ty)?;
                let ptr = self.value()?;
                let align = self.align()?;
                if let Some(value) = value {
                    Instruction::Store {
                        ty,
                        value,
                        ptr,
                        align,
                    }
                } else {
                    Instruction::Load {
                        dst: dst.ok_or_else(|| self.error("load requires a result"))?,
                        ty,
                        ptr,
                        align,
                    }
                }
            }
            "gep" => {
                let dst = required()?;
                let dst_ty = result.as_ref().unwrap().1.clone();
                let base_ptr = self.value()?;
                let mut indices = Vec::new();
                while self.eat(",") {
                    self.node()?;
                    indices.push(self.value()?);
                }
                Instruction::Gep {
                    dst,
                    dst_ty,
                    base_ptr,
                    indices,
                }
            }
            "memcpy" | "memset" => {
                self.expect("ptr")?;
                self.expect("<")?;
                self.expect("u8")?;
                self.expect(">")?;
                let dst = self.value()?;
                self.comma()?;
                if op == "memcpy" {
                    self.expect("ptr")?;
                    self.expect("<")?;
                    self.expect("u8")?;
                    self.expect(">")?;
                } else {
                    self.expect("u8")?;
                }
                let src = self.value()?;
                self.comma()?;
                self.expect("u64")?;
                let n = self.value()?;
                let align = self.align()?;
                if op == "memcpy" {
                    Instruction::Memcpy { dst, src, n, align }
                } else {
                    Instruction::Memset {
                        dst,
                        val: src,
                        n,
                        align,
                    }
                }
            }
            "null_function" | "function_addr" => {
                let dst = required()?;
                let Type::FnPtr(signature) = &result.as_ref().unwrap().1 else {
                    return Err(self.error("function address requires fnptr type"));
                };
                let signature = (**signature).clone();
                if op == "null_function" {
                    Instruction::NullFunction { dst, signature }
                } else {
                    let function = FunctionId(self.id("@f")?);
                    Instruction::FunctionAddr {
                        dst,
                        function,
                        signature,
                    }
                }
            }
            "call" => {
                let convention = self.convention()?;
                let ret_ty = self.ty(0)?;
                let callee = if self.eat("indirect") {
                    Callee::Indirect(self.value()?)
                } else {
                    Callee::Direct(FunctionId(self.id("@f")?))
                };
                self.expect("(")?;
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        self.node()?;
                        args.push(self.value()?);
                        if self.eat(")") {
                            break;
                        }
                        self.comma()?;
                    }
                }
                Instruction::Call {
                    dst,
                    convention,
                    ret_ty,
                    callee,
                    args,
                }
            }
            "trap_if" => {
                self.expect("bool")?;
                let cond = self.value()?;
                self.comma()?;
                self.expect("reason")?;
                self.expect("=")?;
                let reason = self.string()?;
                Instruction::TrapIf { cond, reason }
            }
            _ => {
                return Err(super::ParseError {
                    location: op_location,
                    kind: super::ParseErrorKind::Syntax("unknown instruction".into()),
                })
            }
        };
        let expected = match &ins {
            Instruction::ConstDecl { expression, .. } => Some(expression.ty.clone()),
            Instruction::Const { ty, .. }
            | Instruction::Undef { ty, .. }
            | Instruction::Mov { ty, .. }
            | Instruction::Bin { ty, .. }
            | Instruction::Not { ty, .. }
            | Instruction::Select { ty, .. }
            | Instruction::Phi { ty, .. }
            | Instruction::Load { ty, .. } => Some(ty.clone()),
            Instruction::Cmp { .. } | Instruction::ICmp { .. } | Instruction::FCmp { .. } => {
                Some(Type::Bool)
            }
            Instruction::Cast { dst_ty, .. }
            | Instruction::Extract { dst_ty, .. }
            | Instruction::Gep { dst_ty, .. } => Some(dst_ty.clone()),
            Instruction::Checked { ty, .. } => Some(Type::Tuple(vec![ty.clone(), Type::Bool])),
            Instruction::Alloca { ty, .. } => Some(Type::ptr_to(ty.clone())),
            Instruction::FunctionAddr { signature, .. }
            | Instruction::NullFunction { signature, .. } => {
                Some(Type::FnPtr(Box::new(signature.clone())))
            }
            Instruction::Call { ret_ty, .. } if *ret_ty != Type::Void => Some(ret_ty.clone()),
            _ => None,
        };
        match (&result, expected) {
            (Some((_, annotation)), Some(expected)) => self.equal_type(annotation, &expected)?,
            (None, None) => {}
            _ => return Err(self.error("incorrect result presence")),
        }
        Ok((ins, result))
    }
    pub(super) fn terminator(&mut self) -> Result<Terminator, ParseError> {
        Ok(match self.word()? {
            "br" => {
                self.expect("label")?;
                Terminator::Br {
                    target: self.block_id()?,
                }
            }
            "cbr" => {
                self.expect("bool")?;
                let cond = self.value()?;
                self.comma()?;
                self.expect("label")?;
                let then_bb = self.block_id()?;
                self.comma()?;
                self.expect("label")?;
                let else_bb = self.block_id()?;
                Terminator::CBr {
                    cond,
                    then_bb,
                    else_bb,
                }
            }
            "switch" => {
                let ty = self.ty(0)?;
                let value = self.value()?;
                self.comma()?;
                self.expect("label")?;
                let default_bb = self.block_id()?;
                self.expect("[")?;
                let mut cases = Vec::new();
                if !self.eat("]") {
                    loop {
                        self.node()?;
                        let value = self.literal(&ty)?;
                        self.expect(":")?;
                        let block = self.block_id()?;
                        cases.push((value, block));
                        if self.eat("]") {
                            break;
                        }
                        self.comma()?;
                    }
                }
                Terminator::Switch {
                    ty,
                    value,
                    default_bb,
                    cases,
                }
            }
            "ret" => {
                let ty = self.ty(0)?;
                let value = if ty == Type::Void {
                    None
                } else {
                    Some(self.value()?)
                };
                Terminator::Ret { ty, value }
            }
            "trap" => {
                self.expect("reason")?;
                self.expect("=")?;
                Terminator::Trap {
                    reason: self.string()?,
                }
            }
            _ => return Err(self.error("unknown terminator")),
        })
    }
}
