// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;

use crate::{BlockId, ConstValue, DataLayout, Module, ModuleBuilder, Type, ValueId};

use super::{
    binding::Binding,
    frontend,
    support::{align_of, map_binop, socket_type_to_whale},
    LowerError,
};

#[derive(Clone, Copy, Debug)]
struct LoopCtx {
    cond_bb: BlockId,
    exit_bb: BlockId,
}

type ConstMap = HashMap<String, (Type, ConstValue)>;

pub fn lower_o0(
    program: &frontend::Program,
    target: &str,
    datalayout: DataLayout,
) -> Result<Module, LowerError> {
    let ptr_bits = datalayout.ptr_bits;
    let mut mb = ModuleBuilder::new(target, datalayout);

    let mut gconsts: ConstMap = HashMap::new();
    for g in &program.globals {
        if gconsts.contains_key(&g.name) {
            return Err(LowerError::DuplicateGlobal(g.name.clone()));
        }

        let decl_ty = socket_type_to_whale(&g.ty)?;
        let (vty, v) = eval_const_expr(
            &g.init,
            &ConstEvalCtx {
                local_consts: None,
                global_consts: &gconsts,
            },
        )?;

        if vty != decl_ty {
            return Err(LowerError::TypeMismatch {
                expected: decl_ty,
                got: vty,
            });
        }

        let align = align_of(&vty, ptr_bits);
        mb.add_global(g.name.clone(), vty.clone(), v.clone(), align);
        gconsts.insert(g.name.clone(), (vty, v));
    }

    for f in &program.functions {
        lower_function_o0(&mut mb, f, &gconsts, ptr_bits)?;
    }

    Ok(mb.finish())
}

fn lower_function_o0(
    mb: &mut ModuleBuilder,
    f: &frontend::Function,
    global_consts: &ConstMap,
    ptr_bits: u32,
) -> Result<(), LowerError> {
    let ret_ty = socket_type_to_whale(&f.return_type)?;

    let params: Vec<(String, Type)> = f
        .parameters
        .iter()
        .map(|p| Ok((p.name.clone(), socket_type_to_whale(&p.ty)?)))
        .collect::<Result<_, LowerError>>()?;

    let mut fb = mb.begin_function(f.name.clone(), params, ret_ty.clone());

    let mut env: HashMap<String, Binding> = HashMap::new();
    let mut loop_stack: Vec<LoopCtx> = Vec::new();

    // params -> stack slot (-O0)
    for (i, p) in f.parameters.iter().enumerate() {
        let param_val = fb.param_value(i);
        let ty = socket_type_to_whale(&p.ty)?;
        let align = align_of(&ty, ptr_bits);

        let slot = fb.alloca_in_entry(ty.clone(), align);
        fb.store(ty.clone(), param_val, slot, align);

        env.insert(p.name.clone(), Binding::Addr { ptr: slot, ty });
    }

    for s in &f.body {
        lower_stmt_o0(
            &mut fb,
            &mut env,
            global_consts,
            &mut loop_stack,
            s,
            &ret_ty,
            ptr_bits,
        )?;
    }

    if !fb.is_current_block_terminated() {
        if ret_ty == Type::Void {
            fb.ret(None);
        } else {
            fb.trap("missing return");
        }
    }

    fb.finish();
    Ok(())
}

fn lower_stmt_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    loop_stack: &mut Vec<LoopCtx>,
    stmt: &frontend::Stmt,
    func_ret_ty: &Type,
    ptr_bits: u32,
) -> Result<(), LowerError> {
    if fb.is_current_block_terminated() {
        return Ok(());
    }

    match stmt {
        frontend::Stmt::Return(opt) => {
            if *func_ret_ty == Type::Void {
                fb.ret(None);
                return Ok(());
            }

            let e = opt.as_ref().ok_or(LowerError::UnsupportedStmt)?;
            let (v, ty) = lower_expr_o0(fb, env, global_consts, e, ptr_bits)?;

            if ty != *func_ret_ty {
                return Err(LowerError::TypeMismatch {
                    expected: func_ret_ty.clone(),
                    got: ty,
                });
            }

            fb.ret(Some(v));
            Ok(())
        }

        frontend::Stmt::ConstDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;

            let (cty, cv) = eval_const_expr(
                init,
                &ConstEvalCtx {
                    local_consts: Some(env),
                    global_consts,
                },
            )?;

            if cty != decl_ty {
                return Err(LowerError::TypeMismatch {
                    expected: decl_ty,
                    got: cty,
                });
            }

            env.insert(name.clone(), Binding::Const { ty: cty, value: cv });
            Ok(())
        }

        frontend::Stmt::VarDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;
            let align = align_of(&decl_ty, ptr_bits);

            let (v, vty) = if let Some(init_expr) = init.as_ref() {
                lower_expr_o0(fb, env, global_consts, init_expr, ptr_bits)?
            } else {
                let v = fb.undef(decl_ty.clone());
                (v, decl_ty.clone())
            };

            if vty != decl_ty {
                return Err(LowerError::TypeMismatch {
                    expected: decl_ty,
                    got: vty,
                });
            }

            let slot = fb.alloca_in_entry(decl_ty.clone(), align);
            fb.store(decl_ty.clone(), v, slot, align);

            env.insert(
                name.clone(),
                Binding::Addr {
                    ptr: slot,
                    ty: decl_ty,
                },
            );
            Ok(())
        }

        frontend::Stmt::Assign { name, value } => {
            let (v, vty) = lower_expr_o0(fb, env, global_consts, value, ptr_bits)?;

            let b = env
                .get(name)
                .cloned()
                .ok_or_else(|| LowerError::UnknownVariable(name.clone()))?;

            match b {
                Binding::Const { .. } => Err(LowerError::AssignToConst(name.clone())),
                Binding::Addr { ptr, ty } => {
                    if vty != ty {
                        return Err(LowerError::TypeMismatch {
                            expected: ty,
                            got: vty,
                        });
                    }
                    let align = align_of(&ty, ptr_bits);
                    fb.store(ty, v, ptr, align);
                    Ok(())
                }
            }
        }

        frontend::Stmt::ExprStmt(e) => {
            let _ = lower_expr_o0(fb, env, global_consts, e, ptr_bits)?;
            Ok(())
        }

        frontend::Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, ptr_bits)?;
            if cty != Type::I1 {
                return Err(LowerError::TypeMismatch {
                    expected: Type::I1,
                    got: cty,
                });
            }

            let then_bb = fb.create_block("if.then");
            let else_bb = fb.create_block("if.else");
            let cont_bb = fb.create_block("if.cont");

            fb.cbr(cv, then_bb, else_bb);

            // then
            fb.set_insert_point(then_bb);
            let mut then_env = env.clone();
            for s in then_body {
                lower_stmt_o0(
                    fb,
                    &mut then_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // else
            fb.set_insert_point(else_bb);
            let mut else_env = env.clone();
            for s in else_body {
                lower_stmt_o0(
                    fb,
                    &mut else_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // cont
            fb.set_insert_point(cont_bb);
            Ok(())
        }

        frontend::Stmt::While { cond, body } => {
            let cond_bb = fb.create_block("while.cond");
            let body_bb = fb.create_block("while.body");
            let exit_bb = fb.create_block("while.exit");

            fb.br(cond_bb);

            // cond
            fb.set_insert_point(cond_bb);
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, ptr_bits)?;
            if cty != Type::I1 {
                return Err(LowerError::TypeMismatch {
                    expected: Type::I1,
                    got: cty,
                });
            }
            fb.cbr(cv, body_bb, exit_bb);

            fb.set_insert_point(body_bb);

            loop_stack.push(LoopCtx { cond_bb, exit_bb });

            let mut body_env = env.clone();
            for s in body {
                lower_stmt_o0(
                    fb,
                    &mut body_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cond_bb);
            }

            loop_stack.pop();

            // exit
            fb.set_insert_point(exit_bb);
            Ok(())
        }

        frontend::Stmt::Break => {
            let ctx = loop_stack
                .last()
                .cloned()
                .ok_or(LowerError::BreakOutsideLoop)?;
            fb.br(ctx.exit_bb);
            Ok(())
        }

        frontend::Stmt::Continue => {
            let ctx = loop_stack
                .last()
                .cloned()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            fb.br(ctx.cond_bb);
            Ok(())
        }
    }
}

fn lower_expr_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    expr: &frontend::Expr,
    ptr_bits: u32,
) -> Result<(ValueId, Type), LowerError> {
    match expr {
        frontend::Expr::Var(name) => {
            if let Some(b) = env.get(name).cloned() {
                return match b {
                    Binding::Addr { ptr, ty } => {
                        let align = align_of(&ty, ptr_bits);
                        let v = fb.load(ty.clone(), ptr, align);
                        Ok((v, ty))
                    }
                    Binding::Const { ty, value } => {
                        let v = emit_const_value(fb, &ty, &value);
                        Ok((v, ty))
                    }
                };
            }

            if let Some((ty, value)) = global_consts.get(name).cloned() {
                let v = emit_const_value(fb, &ty, &value);
                return Ok((v, ty));
            }

            Err(LowerError::UnknownVariable(name.clone()))
        }

        frontend::Expr::Lit(lit) => lower_lit_o0(fb, lit),

        frontend::Expr::Binary { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, ptr_bits)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, ptr_bits)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch {
                    expected: lty,
                    got: rty,
                });
            }

            let binop = map_binop(*op, &lty)?;
            let out = fb.bin(binop, lty.clone(), lv, rv);
            Ok((out, lty))
        }

        frontend::Expr::Cmp { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, ptr_bits)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, ptr_bits)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch {
                    expected: lty,
                    got: rty,
                });
            }

            let cmp = super::support::map_cmp(*op, &lty)?;
            let out = fb.cmp(cmp, lty.clone(), lv, rv);
            Ok((out, Type::I1))
        }
    }
}

fn emit_const_value(fb: &mut crate::FunctionBuilder<'_>, ty: &Type, v: &ConstValue) -> ValueId {
    match v {
        ConstValue::Bool(b) => fb.const_bool(*b),
        ConstValue::I(i) => fb.const_int(ty.clone(), *i),
        ConstValue::U(u) => fb.const_uint(ty.clone(), *u),
        ConstValue::F(x) => fb.const_float(ty.clone(), *x),
    }
}

// -------------------------
// const expr evaluator
// -------------------------
struct ConstEvalCtx<'a> {
    local_consts: Option<&'a HashMap<String, Binding>>,
    global_consts: &'a ConstMap,
}

fn eval_const_expr(expr: &frontend::Expr, ctx: &ConstEvalCtx<'_>) -> Result<(Type, ConstValue), LowerError> {
    match expr {
        frontend::Expr::Lit(l) => lit_to_const(l),

        frontend::Expr::Var(name) => {
            // local const 우선
            if let Some(env) = ctx.local_consts {
                if let Some(Binding::Const { ty, value }) = env.get(name) {
                    return Ok((ty.clone(), value.clone()));
                }
                // Addr면 const-eval에서 못 씀
                if env.contains_key(name) {
                    return Err(LowerError::NonConstExpr);
                }
            }

            // global const
            if let Some((ty, v)) = ctx.global_consts.get(name) {
                return Ok((ty.clone(), v.clone()));
            }

            Err(LowerError::UnknownVariable(name.clone()))
        }

        frontend::Expr::Binary { left, op, right } => {
            let (lv_ty, lv) = eval_const_expr(left, ctx)?;
            let (rv_ty, rv) = eval_const_expr(right, ctx)?;

            if lv_ty != rv_ty {
                return Err(LowerError::TypeMismatch {
                    expected: lv_ty,
                    got: rv_ty,
                });
            }

            let out = const_bin(*op, &lv_ty, &lv, &rv)?;
            Ok((lv_ty, out))
        }

        frontend::Expr::Cmp { left, op, right } => {
            let (lt, lv) = eval_const_expr(left, ctx)?;
            let (rt, rv) = eval_const_expr(right, ctx)?;

            if lt != rt {
                return Err(LowerError::TypeMismatch {
                    expected: lt,
                    got: rt,
                });
            }

            let b = const_cmp(*op, &lt, &lv, &rv)?;
            Ok((Type::I1, ConstValue::Bool(b)))
        }
    }
}

fn lit_to_const(l: &frontend::Lit) -> Result<(Type, ConstValue), LowerError> {
    Ok(match l {
        frontend::Lit::Bool(b) => (Type::I1, ConstValue::Bool(*b)),
        frontend::Lit::Int { bits, signed, value } => {
            let ty = super::support::int_type(*bits, *signed)?;
            if *signed {
                (ty.clone(), ConstValue::I(wrap_i(*value, &ty)))
            } else {
                if *value < 0 {
                    return Err(LowerError::UnsupportedExpr);
                }
                (ty.clone(), ConstValue::U(wrap_u(*value as u128, &ty)))
            }
        }
        frontend::Lit::Float { bits, value } => {
            let ty = super::support::float_type(*bits)?;
            (ty, ConstValue::F(*value))
        }
    })
}

fn ty_int_bits(ty: &Type) -> Option<u32> {
    Some(match ty {
        Type::I1 => 1,
        Type::I8 | Type::U8 => 8,
        Type::I16 | Type::U16 => 16,
        Type::I32 | Type::U32 => 32,
        Type::I64 | Type::U64 => 64,
        Type::I128 | Type::U128 => 128,
        _ => return None,
    })
}

fn is_signed_int(ty: &Type) -> bool {
    matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
}

fn wrap_u(v: u128, ty: &Type) -> u128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    v & mask
}

fn wrap_i(v: i128, ty: &Type) -> i128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    let u = (v as u128) & mask;
    let sign = 1u128 << (bits - 1);
    if (u & sign) != 0 {
        // sign-extend
        (u | (!mask)) as i128
    } else {
        u as i128
    }
}

fn const_bin(
    op: frontend::BinOpRef,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<ConstValue, LowerError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(LowerError::UnsupportedExpr);
        };
        let out = match op {
            frontend::BinOpRef::Add => a + b,
            frontend::BinOpRef::Sub => a - b,
            frontend::BinOpRef::Mul => a * b,
        };
        return Ok(ConstValue::F(out));
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::I1) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(LowerError::UnsupportedExpr);
            };
            let raw = match op {
                frontend::BinOpRef::Add => a.wrapping_add(*b),
                frontend::BinOpRef::Sub => a.wrapping_sub(*b),
                frontend::BinOpRef::Mul => a.wrapping_mul(*b),
            };
            return Ok(ConstValue::I(wrap_i(raw, ty)));
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(LowerError::UnsupportedExpr);
            };
            let raw = match op {
                frontend::BinOpRef::Add => a.wrapping_add(*b),
                frontend::BinOpRef::Sub => a.wrapping_sub(*b),
                frontend::BinOpRef::Mul => a.wrapping_mul(*b),
            };
            return Ok(ConstValue::U(wrap_u(raw, ty)));
        }
    }

    Err(LowerError::UnsupportedExpr)
}

fn const_cmp(
    op: frontend::CmpOpRef,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<bool, LowerError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(LowerError::UnsupportedExpr);
        };
        return Ok(match op {
            frontend::CmpOpRef::Eq => a == b,
            frontend::CmpOpRef::Ne => a != b,
            frontend::CmpOpRef::Lt => a < b,
            frontend::CmpOpRef::Le => a <= b,
            frontend::CmpOpRef::Gt => a > b,
            frontend::CmpOpRef::Ge => a >= b,
        });
    }

    // bool(i1)은 eq/ne만
    if matches!(ty, Type::I1) {
        let (ConstValue::Bool(a), ConstValue::Bool(b)) = (l, r) else {
            return Err(LowerError::UnsupportedExpr);
        };
        return Ok(match op {
            frontend::CmpOpRef::Eq => a == b,
            frontend::CmpOpRef::Ne => a != b,
            _ => return Err(LowerError::UnsupportedExpr),
        });
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::I1) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(LowerError::UnsupportedExpr);
            };
            return Ok(match op {
                frontend::CmpOpRef::Eq => a == b,
                frontend::CmpOpRef::Ne => a != b,
                frontend::CmpOpRef::Lt => a < b,
                frontend::CmpOpRef::Le => a <= b,
                frontend::CmpOpRef::Gt => a > b,
                frontend::CmpOpRef::Ge => a >= b,
            });
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(LowerError::UnsupportedExpr);
            };
            return Ok(match op {
                frontend::CmpOpRef::Eq => a == b,
                frontend::CmpOpRef::Ne => a != b,
                frontend::CmpOpRef::Lt => a < b,
                frontend::CmpOpRef::Le => a <= b,
                frontend::CmpOpRef::Gt => a > b,
                frontend::CmpOpRef::Ge => a >= b,
            });
        }
    }

    Err(LowerError::UnsupportedExpr)
}

fn lower_lit_o0(fb: &mut crate::FunctionBuilder<'_>, lit: &frontend::Lit) -> Result<(ValueId, Type), LowerError> {
    match lit {
        frontend::Lit::Bool(b) => Ok((fb.const_bool(*b), Type::I1)),
        frontend::Lit::Int { bits, signed, value } => {
            let ty = super::support::int_type(*bits, *signed)?;
            if *signed {
                Ok((fb.const_int(ty.clone(), wrap_i(*value, &ty)), ty))
            } else {
                if *value < 0 {
                    return Err(LowerError::UnsupportedExpr);
                }
                Ok((fb.const_uint(ty.clone(), wrap_u(*value as u128, &ty)), ty))
            }
        }
        frontend::Lit::Float { bits, value } => {
            let ty = super::support::float_type(*bits)?;
            Ok((fb.const_float(ty.clone(), *value), ty))
        }
    }
}