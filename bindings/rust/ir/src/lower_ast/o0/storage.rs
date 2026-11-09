// SPDX-License-Identifier: MPL-2.0
//! Frontend aggregate values are addresses; scalar values remain SSA values.
//! Aggregate ABI is deliberately outside this storage lowering boundary.
use super::*;

pub(super) fn scalar_signature(sig: &crate::FunctionSignature) -> Result<(), LowerError> {
    // Legacy array parameters remain representable for layout-only clients.
    // They still fail interpreter scalar-signature preflight.
    if sig.params.iter().any(|ty| matches!(ty, Type::Struct(_) | Type::Tuple(_)))
        || aggregate(&sig.ret)
    {
        return Err(LowerError::Aggregate("aggregate by-value signatures are unsupported".into()));
    }
    Ok(())
}

fn aggregate(ty: &Type) -> bool {
    matches!(ty, Type::Array(..) | Type::Struct(_))
}

pub(super) fn read(
    fb: &mut crate::FunctionBuilder<'_>,
    ptr: ValueId,
    ty: Type,
    target: crate::Target,
) -> Result<(ValueId, Type), LowerError> {
    if aggregate(&ty) {
        return Ok((ptr, ty));
    }
    let align = crate::allocation_align(&ty, target).map_err(LowerError::Layout)?;
    Ok((fb.load(ty.clone(), ptr, align), ty))
}

pub(super) fn write(
    fb: &mut crate::FunctionBuilder<'_>,
    ptr: ValueId,
    value: ValueId,
    ty: &Type,
    target: crate::Target,
) -> Result<(), LowerError> {
    fb.charge_lower_type(ty)?;
    let layout = crate::layout_of(ty, target).map_err(LowerError::Layout)?;
    if aggregate(ty) {
        if ptr == value {
            return Ok(());
        }
        // A frontend assignment may alias through an address expression.
        // Snapshot the RHS before writing the destination; explicit IR memcpy
        // keeps its overlap trap.
        let temporary = fb.alloca_in_entry(ty.clone(), layout.align);
        let byte_ptr = Type::ptr_to(Type::U8);
        let dst = fb.cast(crate::CastOp::Bitcast, Type::ptr_to(ty.clone()), byte_ptr.clone(), ptr);
        let src =
            fb.cast(crate::CastOp::Bitcast, Type::ptr_to(ty.clone()), byte_ptr.clone(), value);
        let snapshot =
            fb.cast(crate::CastOp::Bitcast, Type::ptr_to(ty.clone()), byte_ptr, temporary);
        let size = fb.const_uint(Type::U64, layout.size.into());
        fb.memcpy(snapshot, src, size, layout.align);
        fb.memcpy(dst, snapshot, size, layout.align);
    } else {
        fb.store(ty.clone(), value, ptr, layout.align);
    }
    Ok(())
}

fn field_ptr(fb: &mut crate::FunctionBuilder<'_>, base: ValueId, index: u64, ty: Type) -> ValueId {
    let zero = fb.const_uint(Type::U64, 0);
    let index = fb.const_uint(Type::U64, index.into());
    fb.gep(base, vec![zero, index], Type::ptr_to(ty))
}

pub(super) fn place(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    globals: &ConstMap,
    expr: &frontend::Expr,
    target: crate::Target,
) -> Result<(ValueId, Type), LowerError> {
    use frontend::Expr as E;
    match expr {
        E::Located { annotation, expression } => {
            let checkpoint = fb.source_checkpoint()?;
            let result =
                place(fb, env, globals, expression, target).map_err(|e| e.at(annotation))?;
            fb.annotate_source(annotation, checkpoint)?;
            Ok(result)
        },
        E::Var(name) => match env.get(name) {
            Some(Binding::Addr { ptr, ty }) => {
                fb.charge_lower_type(ty)?;
                Ok((*ptr, ty.clone()))
            },
            Some(Binding::Const { .. }) => Err(LowerError::AssignToConst(name.clone())),
            None => Err(LowerError::UnknownVariable(name.clone())),
        },
        E::Deref(value) => {
            let (ptr, ty) = lower_expr_o0(fb, env, globals, value, target)?;
            let Type::Ptr(ty) = ty else { return Err(LowerError::UnsupportedExpr) };
            if *ty == Type::Void {
                return Err(LowerError::UnsupportedExpr);
            }
            Ok((ptr, *ty))
        },
        E::Field { base, layout, name } => {
            let expected = socket_type_to_whale(layout)?;
            let frontend::TypeRef::Struct { fields } = layout else {
                return Err(LowerError::UnsupportedExpr);
            };
            let ordinal = fields
                .iter()
                .position(|f| f.name == *name)
                .ok_or_else(|| LowerError::Aggregate(format!("unknown field {name:?}")))?;
            let Type::Struct(types) = &expected else { unreachable!() };
            let ty = types[ordinal].clone();
            let (ptr, actual) = lower_expr_o0(fb, env, globals, base, target)?;
            if actual != expected {
                return Err(LowerError::TypeMismatch { expected, got: actual });
            }
            Ok((field_ptr(fb, ptr, ordinal as u64, ty.clone()), ty))
        },
        E::Index { base, index } => {
            let (ptr, ty) = lower_expr_o0(fb, env, globals, base, target)?;
            let Type::Array(element, len) = ty else { return Err(LowerError::UnsupportedExpr) };
            let (index, ity) = lower_expr_o0(fb, env, globals, index, target)?;
            let (_, signed) =
                crate::interpreter::integer::shape(&ity).ok_or(LowerError::UnsupportedExpr)?;
            if signed {
                let zero = fb.const_int(ity.clone(), 0);
                let negative = fb.icmp(crate::ICmpPred::Slt, ity.clone(), index, zero);
                fb.trap_if(negative, "array index out of bounds");
            }
            let index = fb.cast(crate::CastOp::CheckedInt, ity, Type::U128, index);
            let limit = fb.const_uint(Type::U128, len.into());
            let bad = fb.icmp(crate::ICmpPred::Uge, Type::U128, index, limit);
            fb.trap_if(bad, "array index out of bounds");
            let zero = fb.const_uint(Type::U64, 0);
            let dst = fb.gep(ptr, vec![zero, index], Type::ptr_to((*element).clone()));
            Ok((dst, *element))
        },
        _ => Err(LowerError::UnsupportedExpr),
    }
}

pub(super) fn literal(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    globals: &ConstMap,
    expr: &frontend::Expr,
    target: crate::Target,
) -> Result<(ValueId, Type), LowerError> {
    use frontend::Expr as E;
    let source_ty = match expr {
        E::Struct { ty, .. } | E::Array { ty, .. } => ty,
        _ => unreachable!(),
    };
    let ty = socket_type_to_whale(source_ty)?;
    fb.charge_lower_type(&ty)?;
    let layout = crate::layout_of(&ty, target).map_err(LowerError::Layout)?;
    let ptr = fb.alloca_in_entry(ty.clone(), layout.align);
    // Allocation is hoisted; initialization is repeated at the expression site.
    fb.uninit(ty.clone(), ptr, layout.align);
    match (expr, &ty, source_ty) {
        (
            E::Struct { fields: values, .. },
            Type::Struct(types),
            frontend::TypeRef::Struct { fields },
        ) => {
            if values.len() != fields.len() {
                return Err(LowerError::Aggregate(
                    "struct initializer field count mismatch".into(),
                ));
            }
            let ordinals: HashMap<_, _> =
                fields.iter().enumerate().map(|(i, f)| (&f.name, i)).collect();
            let mut used = HashSet::new();
            for f in values {
                if !used.insert(&f.name) {
                    return Err(LowerError::Aggregate("duplicate field initializer".into()));
                }
                let ordinal = *ordinals.get(&f.name).ok_or_else(|| {
                    LowerError::Aggregate(format!("unknown initializer field {:?}", f.name))
                })?;
                let (value, actual) = lower_expr_o0(fb, env, globals, &f.value, target)?;
                if actual != types[ordinal] {
                    return Err(LowerError::TypeMismatch {
                        expected: types[ordinal].clone(),
                        got: actual,
                    });
                }
                let dest = field_ptr(fb, ptr, ordinal as u64, actual.clone());
                write(fb, dest, value, &actual, target)?;
            }
        },
        (E::Array { elements, .. }, Type::Array(element, len), _) => {
            if !super::super::support::scalar_storage(element) {
                return Err(LowerError::Aggregate(
                    "initial array literals require scalar elements".into(),
                ));
            }
            if elements.len() as u64 != *len {
                return Err(LowerError::Aggregate("array literal length mismatch".into()));
            }
            for (ordinal, expression) in elements.iter().enumerate() {
                let (value, actual) = lower_expr_o0(fb, env, globals, expression, target)?;
                if actual != **element {
                    return Err(LowerError::TypeMismatch {
                        expected: (**element).clone(),
                        got: actual,
                    });
                }
                let dest = field_ptr(fb, ptr, ordinal as u64, actual.clone());
                write(fb, dest, value, &actual, target)?;
            }
        },
        _ => return Err(LowerError::UnsupportedExpr),
    }
    Ok((ptr, ty))
}

pub(super) fn charge_env(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &HashMap<String, Binding>,
) -> Result<(), LowerError> {
    for binding in env.values() {
        let ty = match binding {
            Binding::Addr { ty, .. } | Binding::Const { ty, .. } => ty,
        };
        fb.charge_lower_type(ty)?;
    }
    Ok(())
}
