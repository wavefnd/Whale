// SPDX-License-Identifier: MPL-2.0
use crate::{CallError, Callee, Function, Instruction, Module, Type, VerifyError};
use std::collections::HashSet;

fn error(name: &str, reason: CallError) -> VerifyError {
    VerifyError::Call {
        func: name.into(),
        reason,
    }
}

pub(super) fn verify_declarations(m: &Module, limits: crate::IrLimits) -> Result<(), VerifyError> {
    for f in &m.functions {
        for (_, ty) in &f.value_types {
            validate_pointer_types(ty, limits).map_err(|e| error(&f.name, e))?;
        }
        for p in &f.params {
            if p.ty == Type::Void {
                return Err(VerifyError::VoidParameter {
                    func: f.name.clone(),
                    param: p.name.clone(),
                });
            }
        }
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut links = HashSet::new();
    for decl in &m.declarations {
        crate::function::validate_decl_with_limits(decl, limits)
            .map_err(|e| error(&decl.name, e))?;
        if !ids.insert(decl.id) || !names.insert(&decl.name) {
            return Err(error(
                &decl.name,
                CallError::ConflictingDeclaration(decl.name.clone()),
            ));
        }
        if let Some(link) = &decl.link_name {
            if !links.insert(link) {
                return Err(error(
                    &decl.name,
                    CallError::LinkNameCollision(link.clone()),
                ));
            }
        }
        if decl.linkage == crate::Linkage::Internal && !m.functions.iter().any(|f| f.id == decl.id)
        {
            return Err(error(&decl.name, CallError::MissingDefinition(decl.id)));
        }
    }
    let mut bodies = HashSet::new();
    for f in &m.functions {
        if !bodies.insert(f.id) {
            return Err(error(&f.name, CallError::DuplicateDefinition(f.id)));
        }
        let decl = m
            .declarations
            .iter()
            .find(|d| d.id == f.id)
            .ok_or_else(|| error(&f.name, CallError::UnknownFunction(f.id)))?;
        if decl.name != f.name
            || decl.signature.ret != f.ret_ty
            || decl.signature.params != f.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>()
        {
            return Err(error(&f.name, CallError::DefinitionMismatch(f.id)));
        }
    }
    Ok(())
}

pub(super) fn verify_instruction(
    m: &Module,
    f: &Function,
    ins: &Instruction,
    limits: crate::IrLimits,
) -> Result<(), VerifyError> {
    let check = || -> Result<(), CallError> {
        match ins {
            Instruction::NullFunction { signature, .. } => {
                crate::function::validate_signature_with_limits(signature, limits)?
            }
            Instruction::FunctionAddr {
                function,
                signature,
                ..
            } => {
                let decl = m
                    .declarations
                    .iter()
                    .find(|d| d.id == *function)
                    .ok_or(CallError::UnknownFunction(*function))?;
                if &decl.signature != signature {
                    return Err(CallError::AddressTypeMismatch);
                }
            }
            Instruction::Call {
                dst,
                ret_ty,
                convention,
                callee,
                args,
            } => {
                let sig = match callee {
                    Callee::Direct(id) => {
                        &m.declarations
                            .iter()
                            .find(|d| d.id == *id)
                            .ok_or(CallError::UnknownFunction(*id))?
                            .signature
                    }
                    Callee::Indirect(value) => match f.value_type(*value) {
                        Some(Type::FnPtr(sig)) => sig,
                        Some(_) => return Err(CallError::NotFunctionPointer(*value)),
                        None => return Err(CallError::UnknownValue(*value)),
                    },
                };
                crate::function::validate_signature_with_limits(sig, limits)?;
                if *convention != sig.convention {
                    return Err(CallError::ConventionMismatch);
                }
                if ret_ty != &sig.ret {
                    return Err(CallError::ReturnType {
                        expected: sig.ret.clone(),
                        got: ret_ty.clone(),
                    });
                }
                if dst.is_some() == (sig.ret == Type::Void) {
                    return Err(CallError::ResultPresence);
                }
                if args.len() != sig.params.len() {
                    return Err(CallError::ArgumentCount {
                        expected: sig.params.len(),
                        got: args.len(),
                    });
                }
                for (index, (value, expected)) in args.iter().zip(&sig.params).enumerate() {
                    let got = f
                        .value_type(*value)
                        .ok_or(CallError::UnknownValue(*value))?;
                    if got != expected {
                        return Err(CallError::ArgumentType {
                            index,
                            expected: expected.clone(),
                            got: got.clone(),
                        });
                    }
                }
            }
            Instruction::Cast {
                src,
                src_ty,
                dst_ty,
                ..
            } if contains_function_pointer(src_ty)
                || contains_function_pointer(dst_ty)
                || f.value_type(*src).is_some_and(contains_function_pointer) =>
            {
                return Err(CallError::ForbiddenFunctionPointerCast)
            }
            _ => {}
        }
        Ok(())
    };
    check().map_err(|e| error(&f.name, e))
}

fn contains_function_pointer(ty: &Type) -> bool {
    match ty {
        Type::FnPtr(_) => true,
        Type::Ptr(t) | Type::Array(t, _) => contains_function_pointer(t),
        Type::Struct(ts) | Type::Tuple(ts) => ts.iter().any(contains_function_pointer),
        _ => false,
    }
}

fn validate_pointer_types(ty: &Type, limits: crate::IrLimits) -> Result<(), CallError> {
    match ty {
        Type::FnPtr(sig) => crate::function::validate_signature_with_limits(sig, limits),
        Type::Ptr(t) | Type::Array(t, _) => validate_pointer_types(t, limits),
        Type::Struct(ts) | Type::Tuple(ts) => {
            for t in ts {
                validate_pointer_types(t, limits)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
