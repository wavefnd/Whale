// SPDX-License-Identifier: MPL-2.0
use crate::{CallError, Callee, FunctionDecl, FunctionId, Instruction, Module, Type, VerifyError};

use std::cell::Cell;

use std::collections::HashMap;

#[cfg(not(whale_wave_ir))]
use std::collections::HashSet;

pub(super) struct DeclarationIndex<'a> {
    by_id: HashMap<FunctionId, &'a FunctionDecl>,
    work: Cell<usize>,
}

impl<'a> DeclarationIndex<'a> {
    fn get(&self, id: FunctionId) -> Option<&'a FunctionDecl> {
        self.work.set(self.work.get().saturating_add(1));
        self.by_id.get(&id).copied()
    }

    pub(super) fn work(&self) -> usize {
        self.work.get()
    }
}

fn error(name: &str, reason: CallError) -> VerifyError {
    VerifyError::Call { func: name.into(), reason }
}

pub(super) fn verify_declarations<'a>(
    m: &'a Module,
    limits: crate::IrLimits,
    analysis: &super::AnalysisBudget,
) -> Result<DeclarationIndex<'a>, VerifyError> {
    #[cfg(not(whale_wave_ir))]
    let _ = analysis;
    for f in &m.functions {
        for (_, ty) in &f.value_types {
            #[cfg(whale_wave_ir)]
            super::wave_semantics::pointer_signatures(ty, analysis)
                .map_err(|e| with_name(e, &f.name))?;
            #[cfg(not(whale_wave_ir))]
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
    #[cfg(whale_wave_ir)]
    let mut ids = super::wave_index::Index::new(m.declarations.len(), analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut ids = HashSet::new();
    #[cfg(whale_wave_ir)]
    let mut names = super::wave_index::Index::new(m.declarations.len(), analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut names = HashSet::new();
    #[cfg(whale_wave_ir)]
    let mut links = super::wave_index::Index::new(m.declarations.len(), analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut links = HashSet::new();
    #[cfg(whale_wave_ir)]
    let mut body_ids = {
        let mut index = super::wave_index::Index::new(m.functions.len(), analysis)?;
        for function in &m.functions {
            index.id(function.id.0, analysis)?;
        }
        index
    };
    #[cfg(not(whale_wave_ir))]
    let body_ids: HashSet<_> = m.functions.iter().map(|f| f.id).collect();
    let mut by_id = HashMap::new();
    for decl in &m.declarations {
        #[cfg(whale_wave_ir)]
        {
            super::wave_semantics::signature(&decl.signature, limits, analysis)
                .map_err(|e| with_name(e, &decl.name))?;
            if !super::wave_semantics::valid_link(
                decl.linkage,
                decl.link_name.as_deref(),
                analysis,
            )? {
                return Err(error(&decl.name, CallError::InvalidLinkName(decl.name.clone())));
            }
        }
        #[cfg(not(whale_wave_ir))]
        crate::function::validate_decl_with_limits(decl, limits)
            .map_err(|e| error(&decl.name, e))?;
        #[cfg(whale_wave_ir)]
        let conflicting = !ids.id(decl.id.0, analysis)? || !names.name(&decl.name, analysis)?;
        #[cfg(not(whale_wave_ir))]
        let conflicting = !ids.insert(decl.id) || !names.insert(&decl.name);
        if conflicting {
            return Err(error(&decl.name, CallError::ConflictingDeclaration(decl.name.clone())));
        }
        if let Some(link) = &decl.link_name {
            #[cfg(whale_wave_ir)]
            let inserted = links.name(link, analysis)?;
            #[cfg(not(whale_wave_ir))]
            let inserted = links.insert(link);
            if !inserted {
                return Err(error(&decl.name, CallError::LinkNameCollision(link.clone())));
            }
        }
        #[cfg(whale_wave_ir)]
        let missing_body = !body_ids.contains_id(decl.id.0, analysis)?;
        #[cfg(not(whale_wave_ir))]
        let missing_body = !body_ids.contains(&decl.id);
        if decl.linkage == crate::Linkage::Internal && missing_body {
            return Err(error(&decl.name, CallError::MissingDefinition(decl.id)));
        }
        by_id.insert(decl.id, decl);
    }
    let index = DeclarationIndex {
        by_id,
        work: Cell::new(m.functions.len().saturating_add(m.declarations.len().saturating_mul(2))),
    };
    #[cfg(whale_wave_ir)]
    let mut bodies = super::wave_index::Index::new(m.functions.len(), analysis)?;
    #[cfg(not(whale_wave_ir))]
    let mut bodies = HashSet::new();
    for f in &m.functions {
        #[cfg(whale_wave_ir)]
        let inserted = bodies.id(f.id.0, analysis)?;
        #[cfg(not(whale_wave_ir))]
        let inserted = bodies.insert(f.id);
        if !inserted {
            return Err(error(&f.name, CallError::DuplicateDefinition(f.id)));
        }
        let decl =
            index.get(f.id).ok_or_else(|| error(&f.name, CallError::UnknownFunction(f.id)))?;
        #[cfg(whale_wave_ir)]
        let mismatch = {
            let actual = crate::FunctionSignature {
                params: f.params.iter().map(|param| param.ty.clone()).collect(),
                ret: f.ret_ty.clone(),
                convention: decl.signature.convention,
                variadic: decl.signature.variadic,
            };
            decl.name != f.name
                || !super::wave_semantics::signature_equal(&decl.signature, &actual, analysis)?
        };
        #[cfg(not(whale_wave_ir))]
        let mismatch = decl.name != f.name
            || decl.signature.ret != f.ret_ty
            || decl.signature.params != f.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>();
        if mismatch {
            return Err(error(&f.name, CallError::DefinitionMismatch(f.id)));
        }
    }
    Ok(index)
}

#[cfg(whale_wave_ir)]
fn with_name(error: VerifyError, name: &str) -> VerifyError {
    match error {
        VerifyError::Call { reason, .. } => VerifyError::Call { func: name.into(), reason },
        other => other,
    }
}

#[cfg(whale_wave_ir)]
pub(super) fn verify_instruction(
    declarations: &DeclarationIndex<'_>,
    f: &super::IndexedFunction<'_>,
    ins: &Instruction,
    limits: crate::IrLimits,
    analysis: &super::AnalysisBudget,
) -> Result<(), VerifyError> {
    match ins {
        Instruction::NullFunction { signature, .. } => {
            super::wave_semantics::signature(signature, limits, analysis)
                .map_err(|e| with_name(e, &f.name))
        },
        Instruction::FunctionAddr { function, signature, .. } => {
            let decl = declarations
                .get(*function)
                .ok_or_else(|| error(&f.name, CallError::UnknownFunction(*function)))?;
            if super::wave_semantics::signature_equal(&decl.signature, signature, analysis)? {
                Ok(())
            } else {
                Err(error(&f.name, CallError::AddressTypeMismatch))
            }
        },
        Instruction::Call { callee, .. } => {
            let signature;
            let ty = match callee {
                Callee::Direct(id) => {
                    let declaration = declarations
                        .get(*id)
                        .ok_or_else(|| error(&f.name, CallError::UnknownFunction(*id)))?;
                    signature = Type::FnPtr(Box::new(declaration.signature.clone()));
                    &signature
                },
                Callee::Indirect(value) => f
                    .value_type(*value)
                    .ok_or_else(|| error(&f.name, CallError::UnknownValue(*value)))?,
            };
            super::wave_semantics::call(f, ty, ins, analysis)
        },
        Instruction::Cast { src, src_ty, dst_ty, op, .. } => {
            super::wave_semantics::function_pointer_cast(f, *src, src_ty, dst_ty, op, analysis)
        },
        _ => Ok(()),
    }
}

#[cfg(not(whale_wave_ir))]
pub(super) fn verify_instruction(
    declarations: &DeclarationIndex<'_>,
    f: &super::IndexedFunction<'_>,
    ins: &Instruction,
    limits: crate::IrLimits,
    _analysis: &super::AnalysisBudget,
) -> Result<(), VerifyError> {
    let check = || -> Result<(), CallError> {
        match ins {
            Instruction::NullFunction { signature, .. } => {
                crate::function::validate_signature_with_limits(signature, limits)?
            },
            Instruction::FunctionAddr { function, signature, .. } => {
                let decl =
                    declarations.get(*function).ok_or(CallError::UnknownFunction(*function))?;
                if &decl.signature != signature {
                    return Err(CallError::AddressTypeMismatch);
                }
            },
            Instruction::Call { dst, ret_ty, convention, callee, args } => {
                let sig = match callee {
                    Callee::Direct(id) => {
                        &declarations.get(*id).ok_or(CallError::UnknownFunction(*id))?.signature
                    },
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
                    let got = f.value_type(*value).ok_or(CallError::UnknownValue(*value))?;
                    if got != expected {
                        return Err(CallError::ArgumentType {
                            index,
                            expected: expected.clone(),
                            got: got.clone(),
                        });
                    }
                }
            },
            Instruction::Cast { src, src_ty, dst_ty, op, .. }
                if !(matches!(op, crate::CastOp::Bitcast)
                    && matches!(src_ty, Type::Ptr(_))
                    && matches!(dst_ty, Type::Ptr(_))
                    && matches!(f.value_type(*src), Some(Type::Ptr(_))))
                    && (contains_function_pointer(src_ty)
                        || contains_function_pointer(dst_ty)
                        || f.value_type(*src).is_some_and(contains_function_pointer)) =>
            {
                return Err(CallError::ForbiddenFunctionPointerCast)
            },
            _ => {},
        }
        Ok(())
    };
    check().map_err(|e| error(&f.name, e))
}

#[cfg(not(whale_wave_ir))]
fn contains_function_pointer(ty: &Type) -> bool {
    match ty {
        Type::FnPtr(_) => true,
        Type::Ptr(t) | Type::Array(t, _) => contains_function_pointer(t),
        Type::Struct(ts) | Type::Tuple(ts) => ts.iter().any(contains_function_pointer),
        _ => false,
    }
}

#[cfg(not(whale_wave_ir))]
fn validate_pointer_types(ty: &Type, limits: crate::IrLimits) -> Result<(), CallError> {
    match ty {
        Type::FnPtr(sig) => crate::function::validate_signature_with_limits(sig, limits),
        Type::Ptr(t) | Type::Array(t, _) => validate_pointer_types(t, limits),
        Type::Struct(ts) | Type::Tuple(ts) => {
            for t in ts {
                validate_pointer_types(t, limits)?;
            }
            Ok(())
        },
        _ => Ok(()),
    }
}
