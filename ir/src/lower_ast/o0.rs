// SPDX-License-Identifier: MPL-2.0

use std::collections::{HashMap, HashSet};

use crate::{
    BlockId, ConstBinaryOp, ConstCompareOp, ConstExpr, ConstExprKind, ConstRef, ConstValue,
    DataLayout, GlobalId, Module, ModuleBuilder, Type, ValueId,
};

use super::{
    binding::Binding,
    frontend,
    support::{map_binop, socket_type_to_whale},
    LowerError,
};

mod storage;

#[derive(Clone, Copy, Debug)]
struct LoopCtx {
    cond_bb: BlockId,
    exit_bb: BlockId,
}

type ConstMap = HashMap<String, (GlobalId, Type, ConstValue)>;

pub fn lower_o0(
    program: &frontend::Program,
    target: &str,
    datalayout: DataLayout,
) -> Result<Module, LowerError> {
    lower_o0_with_limits(program, target, datalayout, super::AstLimits::default())
}

/// Lower a directly constructed AST with explicit resource budgets. Depth is
/// capped at `MAX_AST_DEPTH` regardless of the requested override.
pub fn lower_o0_with_limits(
    program: &frontend::Program,
    target: &str,
    datalayout: DataLayout,
    limits: super::AstLimits,
) -> Result<Module, LowerError> {
    super::limits::inspect_program(program, limits).map_err(LowerError::ResourceLimit)?;
    let module_target = target;
    let selected = if target == "whale-bytecode-v1" {
        // Both built-in contracts use the same tracked 64-bit storage layout.
        crate::Target::X86_64WhaleLinux
    } else {
        crate::Target::lookup(target).map_err(LowerError::Target)?
    };
    selected.validate_layout(datalayout).map_err(LowerError::Target)?;
    let target = selected;
    // Functions and globals have separate namespaces. Validate each before bodies.
    let mut functions = HashSet::new();
    for f in &program.functions {
        if !functions.insert(&f.name) {
            return Err(LowerError::DuplicateFunction(f.name.clone()));
        }
        let mut parameters = HashSet::new();
        for p in &f.parameters {
            if !parameters.insert(&p.name) {
                return Err(LowerError::DuplicateParameter {
                    func: f.name.clone(),
                    param: p.name.clone(),
                });
            }
        }
    }
    let mut mb = ModuleBuilder::new(module_target, datalayout);
    mb.set_source_work_limit(limits.max_source_work);

    for d in &program.declarations {
        let signature = super::support::signature_to_whale(&d.signature)?;
        storage::scalar_signature(&signature)?;
        mb.declare_function(d.name.clone(), signature, d.linkage, d.link_name.clone())
            .map_err(LowerError::Call)?;
    }
    for f in &program.functions {
        let signature = crate::FunctionSignature {
            params: f
                .parameters
                .iter()
                .map(|p| socket_type_to_whale(&p.ty))
                .collect::<Result<_, _>>()?,
            ret: socket_type_to_whale(&f.return_type)?,
            convention: f.convention,
            variadic: false,
        };
        storage::scalar_signature(&signature)?;
        mb.declare_function(f.name.clone(), signature, f.linkage, f.link_name.clone())
            .map_err(LowerError::Call)?;
    }
    for d in &program.declarations {
        if d.linkage == crate::Linkage::Internal && !functions.contains(&d.name) {
            return Err(LowerError::Call(crate::CallError::MissingDefinition(
                mb.function_id(&d.name).map_err(LowerError::Call)?,
            )));
        }
    }
    let mut gconsts: ConstMap = HashMap::new();
    for g in &program.globals {
        if gconsts.contains_key(&g.name) {
            return Err(LowerError::DuplicateGlobal(g.name.clone()));
        }

        let decl_ty = socket_type_to_whale(&g.ty)?;
        let (expression, v) = eval_const_expr(
            &g.init,
            &ConstEvalCtx { local_consts: None, global_consts: &gconsts },
        )?;

        let vty = expression.ty.clone();
        if vty != decl_ty {
            return Err(
                LowerError::TypeMismatch { expected: decl_ty, got: vty }.at_expression(&g.init)
            );
        }

        let align = crate::allocation_align(&vty, target).map_err(LowerError::Layout)?;
        let id = mb.add_global_const(g.name.clone(), expression, v.clone(), align);
        mb.retain_const_sources(&g.init)?;
        if let frontend::Expr::Located { annotation, .. } = &g.init {
            mb.annotate_global_source(id, annotation)?;
        }
        gconsts.insert(g.name.clone(), (id, vty, v));
    }

    for f in &program.functions {
        lower_function_o0(&mut mb, f, &gconsts, target)?;
    }

    Ok(mb.finish())
}

fn lower_function_o0(
    mb: &mut ModuleBuilder,
    f: &frontend::Function,
    global_consts: &ConstMap,
    target: crate::Target,
) -> Result<(), LowerError> {
    let ret_ty = socket_type_to_whale(&f.return_type)?;

    let id = mb.function_id(&f.name).map_err(LowerError::Call)?;
    let mut fb = mb
        .begin_declared_function(id, f.parameters.iter().map(|p| p.name.clone()).collect())
        .map_err(LowerError::Call)?;

    let mut env: HashMap<String, Binding> = HashMap::new();
    let mut loop_stack: Vec<LoopCtx> = Vec::new();

    // params -> stack slot (-O0)
    for (i, p) in f.parameters.iter().enumerate() {
        let param_val = fb.param_value(i);
        let ty = socket_type_to_whale(&p.ty)?;
        let align = crate::allocation_align(&ty, target).map_err(LowerError::Layout)?;

        let slot = fb.alloca_in_entry(ty.clone(), align);
        fb.store(ty.clone(), param_val, slot, align);

        env.insert(p.name.clone(), Binding::Addr { ptr: slot, ty });
    }

    for s in &f.body {
        lower_stmt_o0(&mut fb, &mut env, global_consts, &mut loop_stack, s, &ret_ty, target)?;
    }

    if !fb.is_current_block_terminated() {
        if ret_ty == Type::Void {
            fb.ret(None);
        } else {
            fb.trap("missing return");
        }
    }

    fb.finish_source_mapping()?;
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
    target: crate::Target,
) -> Result<(), LowerError> {
    if fb.is_current_block_terminated() {
        // Keep source statements after return/break/continue visible at O0.
        // A new block has no incoming edge from the terminated block, so the
        // preserved statements cannot change the original execution path.
        let dead = fb.create_block("unreachable.cont");
        fb.set_insert_point(dead);
    }

    match stmt {
        frontend::Stmt::Located { annotation, statement } => {
            let checkpoint = fb.source_checkpoint()?;
            lower_stmt_o0(fb, env, global_consts, loop_stack, statement, func_ret_ty, target)
                .map_err(|e| e.at(annotation))?;
            fb.annotate_source(annotation, checkpoint)?;
            Ok(())
        },
        frontend::Stmt::VolatileStore { pointer, value } => {
            let (ptr, pty) = lower_expr_o0(fb, env, global_consts, pointer, target)?;
            let Type::Ptr(ty) = pty else { return Err(LowerError::UnsupportedExpr) };
            if !super::support::scalar_storage(&ty) {
                return Err(LowerError::UnsupportedExpr);
            }
            let (value, actual) = lower_expr_o0(fb, env, global_consts, value, target)?;
            if actual != *ty {
                return Err(LowerError::TypeMismatch { expected: *ty, got: actual });
            }
            let align = crate::allocation_align(&ty, target).map_err(LowerError::Layout)?;
            fb.volatile_store(*ty, value, ptr, align);
            Ok(())
        },
        frontend::Stmt::Store { place, value } => {
            let (ptr, ty) = storage::place(fb, env, global_consts, place, target)?;
            let (v, vty) = lower_expr_o0(fb, env, global_consts, value, target)?;
            if ty != vty {
                return Err(LowerError::TypeMismatch { expected: ty, got: vty });
            }
            storage::write(fb, ptr, v, &ty, target)
        },
        frontend::Stmt::Return(opt) => {
            if *func_ret_ty == Type::Void {
                if opt.is_some() {
                    return Err(LowerError::ValueReturnedFromVoid);
                }
                fb.ret(None);
                return Ok(());
            }

            let e = opt.as_ref().ok_or(LowerError::UnsupportedStmt)?;
            let (v, ty) = lower_expr_o0(fb, env, global_consts, e, target)?;

            if ty != *func_ret_ty {
                return Err(LowerError::TypeMismatch { expected: func_ret_ty.clone(), got: ty }
                    .at_expression(e));
            }

            fb.ret(Some(v));
            Ok(())
        },

        frontend::Stmt::ConstDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;

            let (expression, cv) =
                eval_const_expr(init, &ConstEvalCtx { local_consts: Some(env), global_consts })?;

            let cty = expression.ty.clone();
            if cty != decl_ty {
                return Err(
                    LowerError::TypeMismatch { expected: decl_ty, got: cty }.at_expression(init)
                );
            }

            fb.retain_const_sources(init)?;
            let checkpoint = if matches!(init, frontend::Expr::Located { .. }) {
                Some(fb.source_checkpoint()?)
            } else {
                None
            };
            let id = fb.const_decl(name.clone(), expression, cv.clone());
            if let (frontend::Expr::Located { annotation, .. }, Some(checkpoint)) =
                (init, checkpoint)
            {
                fb.annotate_source(annotation, checkpoint)?;
            }
            env.insert(name.clone(), Binding::Const { id, ty: cty, value: cv });
            Ok(())
        },

        frontend::Stmt::VarDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;
            let align = crate::allocation_align(&decl_ty, target).map_err(LowerError::Layout)?;

            if let Some(init_expr) = init.as_ref() {
                let (v, vty) = lower_expr_o0(fb, env, global_consts, init_expr, target)?;
                if vty != decl_ty {
                    return Err(LowerError::TypeMismatch { expected: decl_ty, got: vty }
                        .at_expression(init_expr));
                }
                let slot = fb.alloca_in_entry(decl_ty.clone(), align);
                storage::write(fb, slot, v, &decl_ty, target)?;
                env.insert(name.clone(), Binding::Addr { ptr: slot, ty: decl_ty });
                return Ok(());
            }
            let slot = fb.alloca_in_entry(decl_ty.clone(), align);
            // This executes at every declaration, including loop redeclarations.
            fb.uninit(decl_ty.clone(), slot, align);

            env.insert(name.clone(), Binding::Addr { ptr: slot, ty: decl_ty });
            Ok(())
        },

        frontend::Stmt::Assign { name, value } => {
            let (v, vty) = lower_expr_o0(fb, env, global_consts, value, target)?;

            let b =
                env.get(name).cloned().ok_or_else(|| LowerError::UnknownVariable(name.clone()))?;

            match b {
                Binding::Const { .. } => Err(LowerError::AssignToConst(name.clone())),
                Binding::Addr { ptr, ty } => {
                    if vty != ty {
                        return Err(LowerError::TypeMismatch { expected: ty, got: vty }
                            .at_expression(value));
                    }
                    storage::write(fb, ptr, v, &ty, target)?;
                    Ok(())
                },
            }
        },

        frontend::Stmt::ExprStmt(e) => lower_discarded_expr(fb, env, global_consts, e, target),

        frontend::Stmt::If { cond, then_body, else_body } => {
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, target)?;
            if cty != Type::Bool {
                return Err(
                    LowerError::TypeMismatch { expected: Type::Bool, got: cty }.at_expression(cond)
                );
            }

            let then_bb = fb.create_block("if.then");
            let else_bb = fb.create_block("if.else");
            let cont_bb = fb.create_block("if.cont");

            fb.cbr(cv, then_bb, else_bb);

            // then
            fb.set_insert_point(then_bb);
            storage::charge_env(fb, env)?;
            let mut then_env = env.clone();
            for s in then_body {
                lower_stmt_o0(
                    fb,
                    &mut then_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    target,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // else
            fb.set_insert_point(else_bb);
            storage::charge_env(fb, env)?;
            let mut else_env = env.clone();
            for s in else_body {
                lower_stmt_o0(
                    fb,
                    &mut else_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    target,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // cont
            fb.set_insert_point(cont_bb);
            Ok(())
        },

        frontend::Stmt::While { cond, body } => {
            let cond_bb = fb.create_block("while.cond");
            let body_bb = fb.create_block("while.body");
            let exit_bb = fb.create_block("while.exit");

            fb.br(cond_bb);

            // cond
            fb.set_insert_point(cond_bb);
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, target)?;
            if cty != Type::Bool {
                return Err(
                    LowerError::TypeMismatch { expected: Type::Bool, got: cty }.at_expression(cond)
                );
            }
            fb.cbr(cv, body_bb, exit_bb);

            fb.set_insert_point(body_bb);

            loop_stack.push(LoopCtx { cond_bb, exit_bb });

            storage::charge_env(fb, env)?;
            let mut body_env = env.clone();
            for s in body {
                lower_stmt_o0(
                    fb,
                    &mut body_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    target,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cond_bb);
            }

            loop_stack.pop();

            // exit
            fb.set_insert_point(exit_bb);
            Ok(())
        },

        frontend::Stmt::Break => {
            let ctx = loop_stack.last().cloned().ok_or(LowerError::BreakOutsideLoop)?;
            fb.br(ctx.exit_bb);
            Ok(())
        },

        frontend::Stmt::Continue => {
            let ctx = loop_stack.last().cloned().ok_or(LowerError::ContinueOutsideLoop)?;
            fb.br(ctx.cond_bb);
            Ok(())
        },
    }
}

fn lower_discarded_expr(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    expr: &frontend::Expr,
    target: crate::Target,
) -> Result<(), LowerError> {
    match expr {
        frontend::Expr::Located { annotation, expression } => {
            let checkpoint = fb.source_checkpoint()?;
            lower_discarded_expr(fb, env, global_consts, expression, target)
                .map_err(|e| e.at(annotation))?;
            fb.annotate_source(annotation, checkpoint)
        },
        frontend::Expr::Call { callee, args } => {
            lower_call(fb, env, global_consts, callee, args, target).map(|_| ())
        },
        _ => lower_expr_o0(fb, env, global_consts, expr, target).map(|_| ()),
    }
}

fn lower_expr_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    expr: &frontend::Expr,
    target: crate::Target,
) -> Result<(ValueId, Type), LowerError> {
    match expr {
        frontend::Expr::Located { annotation, expression } => {
            let checkpoint = fb.source_checkpoint()?;
            let result = lower_expr_o0(fb, env, global_consts, expression, target)
                .map_err(|e| e.at(annotation))?;
            fb.annotate_source(annotation, checkpoint)?;
            Ok(result)
        },
        frontend::Expr::Struct { .. } | frontend::Expr::Array { .. } => {
            storage::literal(fb, env, global_consts, expr, target)
        },
        frontend::Expr::Field { .. } | frontend::Expr::Index { .. } | frontend::Expr::Deref(_) => {
            let (ptr, ty) = storage::place(fb, env, global_consts, expr, target)?;
            storage::read(fb, ptr, ty, target)
        },
        frontend::Expr::AddressOf(value) => {
            let (ptr, ty) = storage::place(fb, env, global_consts, value, target)?;
            Ok((ptr, Type::ptr_to(ty)))
        },
        frontend::Expr::PointerOffset { base, index } => {
            let (ptr, ty) = lower_expr_o0(fb, env, global_consts, base, target)?;
            let Type::Ptr(pointee) = &ty else { return Err(LowerError::UnsupportedExpr) };
            crate::layout_of(pointee, target).map_err(LowerError::Layout)?;
            let (index, ity) = lower_expr_o0(fb, env, global_consts, index, target)?;
            if !super::support::is_int_like(&ity) {
                return Err(LowerError::UnsupportedExpr);
            }
            Ok((fb.gep(ptr, vec![index], ty.clone()), ty))
        },
        frontend::Expr::VolatileLoad(pointer) => {
            let (ptr, pty) = lower_expr_o0(fb, env, global_consts, pointer, target)?;
            let Type::Ptr(ty) = pty else { return Err(LowerError::UnsupportedExpr) };
            if !super::support::scalar_storage(&ty) {
                return Err(LowerError::UnsupportedExpr);
            }
            let align = crate::allocation_align(&ty, target).map_err(LowerError::Layout)?;
            Ok((fb.volatile_load((*ty).clone(), ptr, align), *ty))
        },
        frontend::Expr::NullFunction(sig) => {
            let signature = super::support::signature_to_whale(sig)?;
            let value = fb.null_function(signature.clone()).map_err(LowerError::Call)?;
            Ok((value, Type::FnPtr(Box::new(signature))))
        },
        frontend::Expr::FunctionRef(name) => {
            let id = fb.function_id(name).map_err(LowerError::Call)?;
            let signature =
                fb.callee_signature(&crate::Callee::Direct(id)).map_err(LowerError::Call)?;
            let value = fb.function_addr(id).map_err(LowerError::Call)?;
            Ok((value, Type::FnPtr(Box::new(signature))))
        },
        frontend::Expr::Call { callee, args } => {
            let (value, ty) = lower_call(fb, env, global_consts, callee, args, target)?;
            Ok((value.ok_or(LowerError::VoidCallUsedAsValue)?, ty))
        },
        frontend::Expr::Var(name) => {
            if let Some(Binding::Addr { ty, .. }) = env.get(name) {
                fb.charge_lower_type(ty)?;
            }
            if let Some(b) = env.get(name).cloned() {
                return match b {
                    Binding::Addr { ptr, ty } => storage::read(fb, ptr, ty, target),
                    Binding::Const { id, ty, .. } => Ok((id, ty)),
                };
            }

            if let Some((_, ty, value)) = global_consts.get(name).cloned() {
                let v = emit_const_value(fb, &ty, &value);
                return Ok((v, ty));
            }

            Err(LowerError::UnknownVariable(name.clone()))
        },

        frontend::Expr::Cast { value, ty, kind } => {
            let dst_ty = super::support::socket_type_to_whale(ty)?;
            // Validate categories before lowering the value or emitting its cast.
            if !super::support::is_int_like(&dst_ty) {
                return Err(LowerError::UnsupportedExpr);
            }
            let (src, src_ty) = lower_expr_o0(fb, env, global_consts, value, target)?;
            let (sb, signed) =
                crate::interpreter::integer::shape(&src_ty).ok_or(LowerError::UnsupportedExpr)?;
            let (db, _) = crate::interpreter::integer::shape(&dst_ty).unwrap();
            let op = match kind {
                frontend::IntegerCast::Checked => crate::CastOp::CheckedInt,
                frontend::IntegerCast::Numeric if sb > db => crate::CastOp::Trunc,
                frontend::IntegerCast::Numeric if sb == db => crate::CastOp::Bitcast,
                frontend::IntegerCast::Numeric if signed => crate::CastOp::SExt,
                frontend::IntegerCast::Numeric => crate::CastOp::ZExt,
            };
            let dst = fb.cast(op, src_ty, dst_ty.clone(), src);
            Ok((dst, dst_ty))
        },
        frontend::Expr::Lit(lit) => lower_lit_o0(fb, lit),

        frontend::Expr::Neg(value) => {
            let (value, ty) = lower_expr_o0(fb, env, global_consts, value, target)?;
            if !super::support::is_int_like(&ty) && !matches!(ty, Type::F16 | Type::F32 | Type::F64)
            {
                return Err(LowerError::UnsupportedExpr);
            }
            Ok((fb.neg(ty.clone(), value), ty))
        },
        frontend::Expr::BitNot(value) => {
            let (value, ty) = lower_expr_o0(fb, env, global_consts, value, target)?;
            if !super::support::is_int_like(&ty) {
                return Err(LowerError::UnsupportedExpr);
            }
            Ok((fb.not(ty.clone(), value), ty))
        },
        frontend::Expr::Binary {
            left,
            op: frontend::BinOpRef::LogicalAnd | frontend::BinOpRef::LogicalOr,
            right,
        } => {
            let frontend::Expr::Binary { op, .. } = expr else { unreachable!() };
            let (left_value, left_ty) = lower_expr_o0(fb, env, global_consts, left, target)?;
            if left_ty != Type::Bool {
                return Err(LowerError::TypeMismatch { expected: Type::Bool, got: left_ty });
            }
            let left_exit = fb.current_block_id();
            let rhs_block = fb.create_block("logical.rhs");
            let cont_block = fb.create_block("logical.cont");
            if matches!(op, frontend::BinOpRef::LogicalAnd) {
                fb.cbr(left_value, rhs_block, cont_block);
            } else {
                fb.cbr(left_value, cont_block, rhs_block);
            }
            // Lower and preserve the right expression even if it will be skipped.
            fb.set_insert_point(rhs_block);
            let (right_value, right_ty) = lower_expr_o0(fb, env, global_consts, right, target)?;
            if right_ty != Type::Bool {
                return Err(LowerError::TypeMismatch { expected: Type::Bool, got: right_ty });
            }
            let right_exit = fb.current_block_id();
            fb.br(cont_block);
            fb.set_insert_point(cont_block);
            let result = fb
                .try_phi(Type::Bool, vec![(left_value, left_exit), (right_value, right_exit)])
                .map_err(|e| LowerError::Call(crate::CallError::Builder(e)))?;
            Ok((result, Type::Bool))
        },
        frontend::Expr::Binary { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, target)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, target)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch { expected: lty, got: rty });
            }

            let binop = map_binop(*op, &lty)?;
            let out = fb.bin(binop, lty.clone(), lv, rv);
            Ok((out, lty))
        },

        frontend::Expr::Cmp { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, target)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, target)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch { expected: lty, got: rty });
            }

            let cmp = super::support::map_cmp(*op, &lty)?;
            let out = fb.cmp(cmp, lty.clone(), lv, rv);
            Ok((out, Type::Bool))
        },
    }
}

fn emit_const_value(fb: &mut crate::FunctionBuilder<'_>, ty: &Type, v: &ConstValue) -> ValueId {
    match v {
        ConstValue::Bool(b) => fb.const_bool(*b),
        ConstValue::I(i) => fb.const_int(ty.clone(), *i),
        ConstValue::U(u) => fb.const_uint(ty.clone(), *u),
        ConstValue::F(x) => fb.const_float_bits(ty.clone(), *x),
    }
}

// -------------------------
// const expr evaluator
// -------------------------
struct ConstEvalCtx<'a> {
    local_consts: Option<&'a HashMap<String, Binding>>,
    global_consts: &'a ConstMap,
}

fn eval_const_expr(
    expr: &frontend::Expr,
    ctx: &ConstEvalCtx<'_>,
) -> Result<(ConstExpr, ConstValue), LowerError> {
    let expression = preserve_const_expr(expr, ctx)?;
    let value = expression
        .evaluate_with_error_site(
            &|reference| match reference {
                ConstRef::Global(id) => ctx
                    .global_consts
                    .values()
                    .find(|(other, _, _)| *other == id)
                    .map(|(_, ty, value)| (ty.clone(), value.clone())),
                ConstRef::Local(id) => ctx.local_consts.and_then(|env| {
                    env.values().find_map(|binding| match binding {
                        Binding::Const { id: other, ty, value } if *other == id => {
                            Some((ty.clone(), value.clone()))
                        },
                        _ => None,
                    })
                }),
            },
            crate::IrLimits::default(),
        )
        .map_err(|(error, failed)| {
            let error = LowerError::ConstantEvaluation(error);
            match const_annotation_at(expr, &expression, failed) {
                Some(annotation) => error.at(annotation),
                None => error,
            }
        })?;
    Ok((expression, value))
}

/// Pair original wrappers with their preserved typed nodes. This runs only on
/// failure; the central evaluator has already validated every branch and visited
/// each evaluated operation once. Wrappers do not create typed expression nodes.
fn const_annotation_at<'a>(
    original: &'a frontend::Expr,
    preserved: &ConstExpr,
    failed: &ConstExpr,
) -> Option<&'a crate::SourceAnnotation> {
    let mut pending = vec![(original, preserved, None)];
    while let Some((original, preserved, inherited)) = pending.pop() {
        if let frontend::Expr::Located { annotation, expression } = original {
            pending.push((expression, preserved, Some(annotation)));
            continue;
        }
        if std::ptr::eq(preserved, failed) {
            return inherited;
        }
        match (original, &preserved.kind) {
            (frontend::Expr::Neg(value), ConstExprKind::Neg(typed))
            | (frontend::Expr::BitNot(value), ConstExprKind::Not(typed)) => {
                pending.push((value, typed, inherited));
            },
            (
                frontend::Expr::Binary { left, right, .. },
                ConstExprKind::Binary { left: l, right: r, .. },
            )
            | (
                frontend::Expr::Cmp { left, right, .. },
                ConstExprKind::Compare { left: l, right: r, .. },
            ) => {
                pending.push((right, r, inherited));
                pending.push((left, l, inherited));
            },
            _ => {},
        }
    }
    None
}

fn preserve_const_expr(
    expr: &frontend::Expr,
    ctx: &ConstEvalCtx<'_>,
) -> Result<ConstExpr, LowerError> {
    match expr {
        frontend::Expr::Located { annotation, expression } => {
            preserve_const_expr(expression, ctx).map_err(|e| e.at(annotation))
        },
        frontend::Expr::VolatileLoad(_)
        | frontend::Expr::Struct { .. }
        | frontend::Expr::Array { .. }
        | frontend::Expr::Field { .. }
        | frontend::Expr::Index { .. }
        | frontend::Expr::AddressOf(_)
        | frontend::Expr::Deref(_)
        | frontend::Expr::PointerOffset { .. }
        | frontend::Expr::Cast { .. }
        | frontend::Expr::NullFunction(_)
        | frontend::Expr::Call { .. }
        | frontend::Expr::FunctionRef(_) => Err(LowerError::NonConstExpr),
        frontend::Expr::Lit(literal) => {
            let (ty, value) = lit_to_const(literal)?;
            Ok(ConstExpr::literal(ty, value))
        },
        frontend::Expr::Var(name) => {
            if let Some(binding) = ctx.local_consts.and_then(|env| env.get(name)) {
                return match binding {
                    Binding::Const { id, ty, .. } => Ok(ConstExpr {
                        ty: ty.clone(),
                        kind: ConstExprKind::Reference(ConstRef::Local(*id)),
                    }),
                    Binding::Addr { .. } => Err(LowerError::NonConstExpr),
                };
            }
            let (id, ty, _) = ctx
                .global_consts
                .get(name)
                .ok_or_else(|| LowerError::UnknownVariable(name.clone()))?;
            Ok(ConstExpr { ty: ty.clone(), kind: ConstExprKind::Reference(ConstRef::Global(*id)) })
        },
        frontend::Expr::Neg(value) => {
            let value = Box::new(preserve_const_expr(value, ctx)?);
            Ok(ConstExpr { ty: value.ty.clone(), kind: ConstExprKind::Neg(value) })
        },
        frontend::Expr::BitNot(value) => {
            let value = Box::new(preserve_const_expr(value, ctx)?);
            if !super::support::is_int_like(&value.ty) {
                return Err(LowerError::UnsupportedExpr);
            }
            Ok(ConstExpr { ty: value.ty.clone(), kind: ConstExprKind::Not(value) })
        },
        frontend::Expr::Binary { left, op, right } => {
            let left = Box::new(preserve_const_expr(left, ctx)?);
            let right = Box::new(preserve_const_expr(right, ctx)?);
            if left.ty != right.ty {
                return Err(LowerError::TypeMismatch { expected: left.ty, got: right.ty });
            }
            if matches!(op, frontend::BinOpRef::LogicalAnd | frontend::BinOpRef::LogicalOr) {
                if left.ty != Type::Bool {
                    return Err(LowerError::TypeMismatch { expected: Type::Bool, got: left.ty });
                }
            } else {
                map_binop(*op, &left.ty)?;
            }
            let op = match op {
                frontend::BinOpRef::Add => ConstBinaryOp::Add,
                frontend::BinOpRef::Sub => ConstBinaryOp::Sub,
                frontend::BinOpRef::Mul => ConstBinaryOp::Mul,
                frontend::BinOpRef::BitAnd => ConstBinaryOp::BitAnd,
                frontend::BinOpRef::BitOr => ConstBinaryOp::BitOr,
                frontend::BinOpRef::BitXor => ConstBinaryOp::BitXor,
                frontend::BinOpRef::Div => ConstBinaryOp::Div,
                frontend::BinOpRef::Rem => ConstBinaryOp::Rem,
                frontend::BinOpRef::ShiftLeft => ConstBinaryOp::ShiftLeft,
                frontend::BinOpRef::ShiftRight => ConstBinaryOp::ShiftRight,
                frontend::BinOpRef::LogicalAnd => ConstBinaryOp::LogicalAnd,
                frontend::BinOpRef::LogicalOr => ConstBinaryOp::LogicalOr,
            };
            Ok(ConstExpr { ty: left.ty.clone(), kind: ConstExprKind::Binary { op, left, right } })
        },
        frontend::Expr::Cmp { left, op, right } => {
            let left = Box::new(preserve_const_expr(left, ctx)?);
            let right = Box::new(preserve_const_expr(right, ctx)?);
            if left.ty != right.ty {
                return Err(LowerError::TypeMismatch { expected: left.ty, got: right.ty });
            }
            let op = match op {
                frontend::CmpOpRef::Eq => ConstCompareOp::Eq,
                frontend::CmpOpRef::Ne => ConstCompareOp::Ne,
                frontend::CmpOpRef::Lt => ConstCompareOp::Lt,
                frontend::CmpOpRef::Le => ConstCompareOp::Le,
                frontend::CmpOpRef::Gt => ConstCompareOp::Gt,
                frontend::CmpOpRef::Ge => ConstCompareOp::Ge,
            };
            Ok(ConstExpr { ty: Type::Bool, kind: ConstExprKind::Compare { op, left, right } })
        },
    }
}

fn lit_to_const(l: &frontend::Lit) -> Result<(Type, ConstValue), LowerError> {
    Ok(match l {
        frontend::Lit::Bool(b) => (Type::Bool, ConstValue::Bool(*b)),
        frontend::Lit::Int { bits, signed, value } => {
            let ty = super::support::int_type(*bits, *signed)?;
            let digits = value.strip_prefix('-').unwrap_or(value);
            if digits.is_empty()
                || !digits.bytes().all(|b| b.is_ascii_digit())
                || (!*signed && value.starts_with('-'))
            {
                return Err(LowerError::NumericLiteral(
                    "expected decimal digits with an optional signed minus".into(),
                ));
            }
            let payload =
                if *signed {
                    ConstValue::I(value.parse().map_err(|_| {
                        LowerError::NumericLiteral("integer exceeds i128 range".into())
                    })?)
                } else {
                    ConstValue::U(value.parse().map_err(|_| {
                        LowerError::NumericLiteral("integer exceeds u128 range".into())
                    })?)
                };
            if !crate::constant::valid_constant(&ty, &payload) {
                return Err(LowerError::InvalidLiteral { ty, value: payload });
            }
            (ty, payload)
        },
        frontend::Lit::Float { bits, value } => {
            let ty = super::support::float_type(*bits)?;
            (
                ty,
                ConstValue::F(
                    crate::FloatBits::parse(*bits, value).map_err(LowerError::NumericLiteral)?,
                ),
            )
        },
    })
}

fn lower_lit_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    lit: &frontend::Lit,
) -> Result<(ValueId, Type), LowerError> {
    let (ty, value) = lit_to_const(lit)?;
    let id = match value {
        ConstValue::Bool(v) => fb.const_bool(v),
        ConstValue::I(v) => fb.const_int(ty.clone(), v),
        ConstValue::U(v) => fb.const_uint(ty.clone(), v),
        ConstValue::F(v) => fb.const_float_bits(ty.clone(), v),
    };
    Ok((id, ty))
}

fn lower_call(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    globals: &ConstMap,
    callee: &frontend::CalleeRef,
    args: &[frontend::Expr],
    target: crate::Target,
) -> Result<(Option<ValueId>, Type), LowerError> {
    // The callee expression is evaluated first, followed by arguments in source order.
    let callee = match callee {
        frontend::CalleeRef::Direct(name) => {
            crate::Callee::Direct(fb.function_id(name).map_err(LowerError::Call)?)
        },
        frontend::CalleeRef::Indirect(expr) => {
            crate::Callee::Indirect(lower_expr_o0(fb, env, globals, expr, target)?.0)
        },
    };
    let signature = fb.callee_signature(&callee).map_err(LowerError::Call)?;
    let mut values = Vec::new();
    for arg in args {
        values.push(lower_expr_o0(fb, env, globals, arg, target)?.0);
    }
    let value = fb.call(callee, values).map_err(LowerError::Call)?;
    Ok((value, signature.ret))
}
