// SPDX-License-Identifier: MPL-2.0

use crate::{BasicBlock, BinOp, Callee, CmpOp, ConstValue, Endian, Function, ICmpPred, Instruction, Module, Terminator, Type};

pub fn print_module(m: &Module) -> String {
    let mut out = String::new();
    out.push_str("module {\n");
    out.push_str(&format!("  target \"{}\"\n", m.target));
    out.push_str("  datalayout { ");
    out.push_str(&format!(
        "ptr={}, endian={}",
        m.datalayout.ptr_bits,
        match m.datalayout.endian {
            Endian::Little => "little",
            Endian::Big => "big",
        }
    ));
    out.push_str(" }\n\n");

    for g in &m.globals {
        out.push_str(&format!(
            "  global @{}: {} = const {} {}, align {}\n",
            g.name,
            g.ty,
            g.ty,
            fmt_const(&g.init),
            g.align
        ));
    }
    if !m.globals.is_empty() {
        out.push('\n');
    }

    for f in &m.functions {
        out.push_str(&print_function(f));
        out.push('\n');
    }

    out.push_str("}\n");
    out
}

fn print_function(f: &Function) -> String {
    let mut out = String::new();
    out.push_str(&format!("  fn @{}(", f.name));
    for (i, p) in f.params.iter().enumerate() {
        if i != 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{}: {}", p.name, p.ty));
    }
    out.push_str(&format!(") -> {} {{\n", f.ret_ty));

    for b in &f.blocks {
        out.push_str(&print_block(b));
    }

    out.push_str("  }\n");
    out
}

fn print_block(b: &BasicBlock) -> String {
    let mut out = String::new();
    out.push_str(&format!("  {}:\n", b.name));

    for ins in &b.instructions {
        out.push_str("    ");
        out.push_str(&print_instr(ins));
        out.push('\n');
    }

    if let Some(t) = &b.terminator {
        out.push_str("    ");
        out.push_str(&print_term(t));
        out.push('\n');
    }

    out
}

fn print_instr(i: &Instruction) -> String {
    use Instruction::*;
    match i {
        Const { dst, ty, value } => format!("{dst}: {ty} = const {ty} {}", fmt_const(value)),
        Undef { dst, ty } => format!("{dst}: {ty} = undef {ty}"),
        Mov { dst, ty, src } => format!("{dst}: {ty} = mov {ty} {src}"),

        Bin {
            dst,
            op,
            ty,
            lhs,
            rhs,
        } => format!("{dst}: {ty} = {} {ty} {lhs}, {rhs}", fmt_binop(op)),
        Not { dst, ty, src } => format!("{dst}: {ty} = not {ty} {src}"),

        Cmp { dst, op, ty, lhs, rhs } => {
            format!("{dst}: i1 = cmp {} {ty} {lhs}, {rhs}", fmt_cmpop(op))
        }

        ICmp {
            dst,
            pred,
            ty,
            lhs,
            rhs,
        } => format!("{dst}: i1 = icmp {} {ty} {lhs}, {rhs}", fmt_icmp(pred)),
        FCmp {
            dst,
            pred,
            ty,
            lhs,
            rhs,
        } => format!("{dst}: i1 = fcmp {} {ty} {lhs}, {rhs}", fmt_fcmp(pred)),

        Select {
            dst,
            ty,
            cond,
            on_true,
            on_false,
        } => format!("{dst}: {ty} = select i1 {cond}, {ty} {on_true}, {ty} {on_false}"),

        Cast {
            dst,
            op,
            dst_ty,
            src_ty,
            src,
        } => format!(
            "{dst}: {dst_ty} = {} {src_ty} {src} to {dst_ty}",
            fmt_cast(op)
        ),

        Phi { dst, ty, incomings } => {
            let mut s = format!("{dst}: {ty} = phi {ty} ");
            for (i, (v, bb)) in incomings.iter().enumerate() {
                if i != 0 {
                    s.push_str(", ");
                }
                s.push_str(&format!("[ {v}, {} ]", bb.0));
            }
            s
        }

        Extract {
            dst,
            dst_ty,
            tuple,
            index,
        } => format!("{dst}: {dst_ty} = extract {tuple}, {}", index),

        Checked {
            dst,
            op,
            ty,
            lhs,
            rhs,
        } => format!(
            "{dst}: tuple<{ty}, i1> = {}_chk {ty} {lhs}, {rhs}",
            fmt_checked(op)
        ),

        Alloca { dst, ty, align } => format!("{dst}: ptr<{ty}> = alloca {ty}, align {align}"),

        Load {
            dst,
            ty,
            ptr,
            align,
        } => format!("{dst}: {ty} = load {ty}, ptr<{ty}> {ptr}, align {align}"),

        Store {
            ty,
            value,
            ptr,
            align,
        } => format!("store {ty} {value}, ptr<{ty}> {ptr}, align {align}"),

        Gep {
            dst,
            dst_ty,
            base_ptr,
            indices,
        } => {
            let mut s = format!("{dst}: {dst_ty} = gep {base_ptr}");
            for idx in indices {
                s.push_str(&format!(", {idx}"));
            }
            s
        }

        Memcpy { dst, src, n, align } => {
            format!("memcpy ptr<u8> {dst}, ptr<u8> {src}, u64 {n}, align {align}")
        }

        Memset { dst, val, n, align } => {
            format!("memset ptr<u8> {dst}, u8 {val}, u64 {n}, align {align}")
        }

        Call {
            dst,
            ret_ty,
            callee,
            args,
        } => {
            let mut s = String::new();
            if let Some(v) = dst {
                s.push_str(&format!("{v}: {ret_ty} = "));
            }
            s.push_str(&format!("call {ret_ty} {}(", fmt_callee(callee)));
            for (i, a) in args.iter().enumerate() {
                if i != 0 {
                    s.push_str(", ");
                }
                s.push_str(&format!("{a}"));
            }
            s.push(')');
            s
        }

        TrapIf { cond, reason } => format!("trap_if i1 {cond}, reason=\"{}\"", escape(reason)),
    }
}

fn print_term(t: &Terminator) -> String {
    use Terminator::*;
    match t {
        Br { target } => format!("br label {}", target.0),
        CBr {
            cond,
            then_bb,
            else_bb,
        } => format!("cbr i1 {cond}, label {}, label {}", then_bb.0, else_bb.0),
        Switch {
            ty,
            value,
            default_bb,
            cases,
        } => {
            let mut s = format!("switch {ty} {value}, label {} [", default_bb.0);
            for (i, (c, bb)) in cases.iter().enumerate() {
                if i != 0 {
                    s.push_str(",");
                }
                s.push_str(&format!(" {}: {}", fmt_const(c), bb.0));
            }
            s.push_str(" ]");
            s
        }
        Ret { ty, value } => {
            if *ty == Type::Void {
                "ret void".to_string()
            } else if let Some(v) = value {
                format!("ret {ty} {v}")
            } else {
                format!("ret {ty} <missing>")
            }
        }
        Trap { reason } => format!("trap reason=\"{}\"", escape(reason)),
    }
}

fn fmt_binop(op: &BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "add",
        Sub => "sub",
        Mul => "mul",
        UDiv => "udiv",
        SDiv => "sdiv",
        URem => "urem",
        SRem => "srem",
        FAdd => "fadd",
        FSub => "fsub",
        FMul => "fmul",
        FDiv => "fdiv",
        FRem => "frem",
        And => "and",
        Or => "or",
        Xor => "xor",
        Shl => "shl",
        LShr => "lshr",
        AShr => "ashr",
    }
}

fn fmt_checked(op: &crate::CheckedOp) -> &'static str {
    use crate::CheckedOp::*;
    match op {
        UAdd => "uadd",
        USub => "usub",
        UMul => "umul",
        SAdd => "sadd",
        SSub => "ssub",
        SMul => "smul",
    }
}

fn fmt_icmp(p: &ICmpPred) -> &'static str {
    use ICmpPred::*;
    match p {
        Eq => "eq",
        Ne => "ne",
        Ult => "ult",
        Ule => "ule",
        Ugt => "ugt",
        Uge => "uge",
        Slt => "slt",
        Sle => "sle",
        Sgt => "sgt",
        Sge => "sge",
    }
}

fn fmt_fcmp(p: &crate::FCmpPred) -> &'static str {
    use crate::FCmpPred::*;
    match p {
        Oeq => "oeq",
        One => "one",
        Olt => "olt",
        Ole => "ole",
        Ogt => "ogt",
        Oge => "oge",
        Ord => "ord",
        Uno => "uno",
        Ueq => "ueq",
        Une => "une",
        Ult => "ult",
        Ule => "ule",
        Ugt => "ugt",
        Uge => "uge",
    }
}

fn fmt_cast(op: &crate::CastOp) -> &'static str {
    use crate::CastOp::*;
    match op {
        ZExt => "zext",
        SExt => "sext",
        Trunc => "trunc",
        FExt => "fext",
        FTrunc => "ftrunc",
        IToF_S => "itof_s",
        IToF_U => "itof_u",
        FToI_S => "ftoi_s",
        FToI_U => "ftoi_u",
        Bitcast => "bitcast",
        PtrToInt => "ptrtoint",
        IntToPtr => "inttoptr",
    }
}

fn fmt_callee(c: &Callee) -> String {
    match c {
        Callee::Symbol(s) => {
            if s.starts_with('@') {
                s.clone()
            } else {
                format!("@{s}")
            }
        }
    }
}

fn fmt_const(c: &ConstValue) -> String {
    match c {
        ConstValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        ConstValue::I(i) => i.to_string(),
        ConstValue::U(u) => u.to_string(),
        ConstValue::F(x) => {
            if x.is_nan() {
                "nan".into()
            } else if x.is_infinite() && x.is_sign_positive() {
                "+inf".into()
            } else if x.is_infinite() {
                "-inf".into()
            } else {
                x.to_string()
            }
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fmt_cmpop(op: &CmpOp) -> &'static str {
    use CmpOp::*;
    match op {
        Eq => "eq",
        Ne => "ne",

        SLt => "slt",
        SLe => "sle",
        SGt => "sgt",
        SGe => "sge",

        ULt => "ult",
        ULe => "ule",
        UGt => "ugt",
        UGe => "uge",

        FEq => "feq",
        FNe => "fne",
        FLt => "flt",
        FLe => "fle",
        FGt => "fgt",
        FGe => "fge",
    }
}
