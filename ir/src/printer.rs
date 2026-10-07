// SPDX-License-Identifier: MPL-2.0

use crate::{
    BasicBlock, BinOp, Callee, CmpOp, ConstValue, Endian, Function, ICmpPred, Instruction, Module,
    Terminator, Type,
};

pub fn print_module(m: &Module) -> String {
    let mut out = String::new();
    out.push_str("module {\n");
    out.push_str(&format!(
        "  format_version {}\n  semantics_version {}\n",
        crate::IR_FORMAT_VERSION,
        crate::SEMANTICS_VERSION
    ));
    out.push_str(&format!("  target \"{}\"\n", escape(&m.target)));
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
            "  global @g{} \"{}\": {} = const {} {}, align {}, init_expr {}\n",
            g.id.0,
            escape(&g.name),
            g.ty,
            g.ty,
            fmt_const(&g.init),
            g.align,
            print_const_expr(&g.init_expr)
        ));
    }
    if !m.globals.is_empty() {
        out.push('\n');
    }

    for d in &m.declarations {
        out.push_str(&format!(
            "  declare @f{} \"{}\": {}, linkage {}",
            d.id.0,
            escape(&d.name),
            d.signature,
            match d.linkage {
                crate::Linkage::Internal => "internal",
                crate::Linkage::External => "external",
            }
        ));
        if let Some(link) = &d.link_name {
            out.push_str(&format!(", link_name \"{}\"", escape(link)));
        }
        out.push('\n');
    }
    if !m.declarations.is_empty() {
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
    out.push_str(&format!("  fn @f{} \"{}\"(", f.id.0, escape(&f.name)));
    for (i, p) in f.params.iter().enumerate() {
        if i != 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{} \"{}\": {}", p.id, escape(&p.name), p.ty));
    }
    out.push_str(&format!(") -> {}, entry {} {{\n", f.ret_ty, f.entry));

    for b in &f.blocks {
        out.push_str(&print_block(b));
    }

    out.push_str("  }\n");
    out
}

fn print_block(b: &BasicBlock) -> String {
    let mut out = String::new();
    out.push_str(&format!("  {} \"{}\":\n", b.id, escape(&b.name)));

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
        ConstDecl {
            dst,
            name,
            expression,
            value,
        } => format!(
            "{dst}: {} = const_decl \"{}\" {} => const {} {}",
            expression.ty,
            escape(name),
            print_const_expr(expression),
            expression.ty,
            fmt_const(value)
        ),
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

        Cmp {
            dst,
            op,
            ty,
            lhs,
            rhs,
        } => {
            format!("{dst}: bool = cmp {} {ty} {lhs}, {rhs}", fmt_cmpop(op))
        }

        ICmp {
            dst,
            pred,
            ty,
            lhs,
            rhs,
        } => format!("{dst}: bool = icmp {} {ty} {lhs}, {rhs}", fmt_icmp(pred)),
        FCmp {
            dst,
            pred,
            ty,
            lhs,
            rhs,
        } => format!("{dst}: bool = fcmp {} {ty} {lhs}, {rhs}", fmt_fcmp(pred)),

        Select {
            dst,
            ty,
            cond,
            on_true,
            on_false,
        } => format!("{dst}: {ty} = select bool {cond}, {ty} {on_true}, {ty} {on_false}"),

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
                s.push_str(&format!("[ {v}, {bb} ]"));
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
            "{dst}: tuple<{ty}, bool> = {}_chk {ty} {lhs}, {rhs}",
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

        NullFunction { dst, signature } => format!("{dst}: fnptr<{signature}> = null_function"),
        FunctionAddr {
            dst,
            function,
            signature,
        } => format!("{dst}: fnptr<{signature}> = function_addr @f{}", function.0),
        Call {
            convention,
            dst,
            ret_ty,
            callee,
            args,
        } => {
            let mut s = String::new();
            if let Some(v) = dst {
                s.push_str(&format!("{v}: {ret_ty} = "));
            }
            s.push_str(&format!(
                "call {} {ret_ty} {}(",
                match convention {
                    crate::CallingConvention::Whale => "whale",
                    crate::CallingConvention::SysV64 => "sysv64",
                },
                fmt_callee(callee)
            ));
            for (i, a) in args.iter().enumerate() {
                if i != 0 {
                    s.push_str(", ");
                }
                s.push_str(&format!("{a}"));
            }
            s.push(')');
            s
        }

        TrapIf { cond, reason } => format!("trap_if bool {cond}, reason=\"{}\"", escape(reason)),
    }
}

pub fn print_const_expr(expr: &crate::ConstExpr) -> String {
    use crate::{ConstBinaryOp as B, ConstCompareOp as C, ConstExprKind as K, ConstRef};
    match &expr.kind {
        K::Literal(value) => format!("{} {}", expr.ty, fmt_const(value)),
        K::Reference(reference) => match reference {
            ConstRef::Global(id) => format!("{} @g{}", expr.ty, id.0),
            ConstRef::Local(id) => format!("{} {id}", expr.ty),
        },
        K::Binary { op, left, right } => format!(
            "{}({}, {})",
            match op {
                B::Add => "add",
                B::Sub => "sub",
                B::Mul => "mul",
            },
            print_const_expr(left),
            print_const_expr(right)
        ),
        K::Compare { op, left, right } => format!(
            "{}({}, {})",
            match op {
                C::Eq => "eq",
                C::Ne => "ne",
                C::Lt => "lt",
                C::Le => "le",
                C::Gt => "gt",
                C::Ge => "ge",
            },
            print_const_expr(left),
            print_const_expr(right)
        ),
    }
}

fn print_term(t: &Terminator) -> String {
    use Terminator::*;
    match t {
        Br { target } => format!("br label {target}"),
        CBr {
            cond,
            then_bb,
            else_bb,
        } => format!("cbr bool {cond}, label {then_bb}, label {else_bb}"),
        Switch {
            ty,
            value,
            default_bb,
            cases,
        } => {
            let mut s = format!("switch {ty} {value}, label {default_bb} [");
            for (i, (c, bb)) in cases.iter().enumerate() {
                if i != 0 {
                    s.push(',');
                }
                s.push_str(&format!(" {}: {bb}", fmt_const(c)));
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
        Callee::Direct(id) => format!("@f{}", id.0),
        Callee::Indirect(value) => format!("indirect {value}"),
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
        ConstValue::F(x) => x.to_string(),
    }
}

// String fields use one syntax, independent of Rust's Debug formatting.
// Printable Unicode is preserved; control and line-separator characters are escaped.
fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                use std::fmt::Write;
                write!(&mut out, "\\u{{{:x}}}", ch as u32).expect("writing to a String");
            }
            ch => out.push(ch),
        }
    }
    out
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
