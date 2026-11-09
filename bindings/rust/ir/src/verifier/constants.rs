use std::collections::HashMap;

#[cfg(any(not(whale_wave_ir), test))]
use std::collections::{HashSet, VecDeque};

use super::VerifyError;

#[cfg(any(not(whale_wave_ir), test))]
use crate::ConstEvalError;

use crate::{ConstExpr, ConstRef, ConstValue, Function, Instruction, Module, Type};

#[cfg(whale_wave_ir)]
#[path = "wave_constants.rs"]
mod wave;

#[derive(Default)]
pub(super) struct Values {
    entries: HashMap<ConstRef, (Type, ConstValue)>,
    #[cfg(whale_wave_ir)]
    index: Option<wave::Index>,
}

impl Values {
    fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for Values {
    type Target = HashMap<ConstRef, (Type, ConstValue)>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl std::ops::DerefMut for Values {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(whale_wave_ir)]
        {
            self.index = None;
        }
        &mut self.entries
    }
}

#[cfg(test)]
impl<const N: usize> From<[(ConstRef, (Type, ConstValue)); N]> for Values {
    fn from(entries: [(ConstRef, (Type, ConstValue)); N]) -> Self {
        Self {
            entries: HashMap::from(entries),
            #[cfg(whale_wave_ir)]
            index: None,
        }
    }
}

fn lookup<'a>(
    locals: &'a Values,
    globals: &'a Values,
    reference: ConstRef,
) -> Option<&'a (Type, ConstValue)> {
    locals.get(&reference).or_else(|| globals.get(&reference))
}

#[derive(Clone, Copy)]
struct Definition<'a> {
    id: ConstRef,
    ty: &'a Type,
    expression: &'a ConstExpr,
    result: &'a ConstValue,
}

fn evaluate(
    scope: &str,
    definitions: Vec<Definition<'_>>,
    values: Values,
    globals: &Values,
    limits: crate::IrLimits,
) -> Result<Values, VerifyError> {
    #[cfg(whale_wave_ir)]
    {
        wave::evaluate(scope, definitions, values, globals, limits)
    }
    #[cfg(not(whale_wave_ir))]
    {
        evaluate_reference(scope, definitions, values, globals, limits)
    }
}

#[cfg(any(not(whale_wave_ir), test))]
fn evaluate_reference(
    scope: &str,
    definitions: Vec<Definition<'_>>,
    mut values: Values,
    globals: &Values,
    limits: crate::IrLimits,
) -> Result<Values, VerifyError> {
    let error = |declaration, reason| VerifyError::InvalidConstExpression {
        scope: scope.into(),
        declaration,
        reason,
    };
    let mut indices = HashMap::new();
    for (i, definition) in definitions.iter().enumerate() {
        if indices.insert(definition.id, i).is_some()
            || lookup(&values, globals, definition.id).is_some()
        {
            return Err(error(definition.id, ConstEvalError::DuplicateDefinition(definition.id)));
        }
        if definition.ty != &definition.expression.ty {
            return Err(error(definition.id, ConstEvalError::TypeMismatch));
        }
    }
    let mut remaining = vec![0; definitions.len()];
    let mut users = vec![Vec::new(); definitions.len()];
    let mut ready = VecDeque::new();
    for (i, definition) in definitions.iter().enumerate() {
        let mut seen = HashSet::new();
        for reference in definition.expression.references() {
            if !seen.insert(reference) || lookup(&values, globals, reference).is_some() {
                continue;
            }
            let &dependency = indices
                .get(&reference)
                .ok_or_else(|| error(definition.id, ConstEvalError::UnknownReference(reference)))?;
            remaining[i] += 1;
            users[dependency].push(i);
        }
        if remaining[i] == 0 {
            ready.push_back(i);
        }
    }
    let mut completed = vec![false; definitions.len()];
    while let Some(i) = ready.pop_front() {
        let definition = &definitions[i];
        let result = definition
            .expression
            .evaluate_with_error_site_reference(
                &|reference| lookup(&values, globals, reference).cloned(),
                limits,
            )
            .map_err(|(reason, _)| error(definition.id, reason))?;
        if !crate::const_expr::same_value(&result, definition.result) {
            return Err(error(definition.id, ConstEvalError::ResultMismatch));
        }
        values.insert(definition.id, (definition.ty.clone(), result));
        completed[i] = true;
        for &user in &users[i] {
            remaining[user] -= 1;
            if remaining[user] == 0 {
                ready.push_back(user);
            }
        }
    }
    if let Some(i) = completed.iter().position(|done| !done) {
        return Err(error(definitions[i].id, ConstEvalError::CyclicReference));
    }
    Ok(values)
}

pub(super) fn verify_globals(m: &Module, limits: crate::IrLimits) -> Result<Values, VerifyError> {
    evaluate(
        "module",
        m.globals
            .iter()
            .map(|g| Definition {
                id: ConstRef::Global(g.id),
                ty: &g.ty,
                expression: &g.init_expr,
                result: &g.init,
            })
            .collect(),
        Values::new(),
        &Values::new(),
        limits,
    )
}

pub(super) fn verify_locals(
    f: &Function,
    globals: &Values,
    limits: crate::IrLimits,
) -> Result<(), VerifyError> {
    let mut values = Values::new();
    let mut definitions = Vec::new();
    for ins in f.blocks.iter().flat_map(|b| &b.instructions) {
        match ins {
            Instruction::Const { dst, ty, value } => {
                values.insert(ConstRef::Local(*dst), (ty.clone(), value.clone()));
            },
            Instruction::ConstDecl { dst, expression, value, .. } => definitions.push(Definition {
                id: ConstRef::Local(*dst),
                ty: &expression.ty,
                expression,
                result: value,
            }),
            _ => {},
        }
    }
    evaluate(&f.name, definitions, values, globals, limits).map(|_| ())
}

#[cfg(test)]
mod sharing_tests {
    use super::*;

    #[test]
    fn overlay_borrows_the_verified_global_entry() {
        let id = ConstRef::Global(crate::GlobalId(7));
        let globals = Values::from([(id, (Type::I32, ConstValue::I(42)))]);
        let mut locals = Values::new();
        locals.insert(ConstRef::Local(crate::ValueId(7)), (Type::I32, ConstValue::I(9)));
        assert!(std::ptr::eq(lookup(&locals, &globals, id).unwrap(), globals.get(&id).unwrap()));
        assert_eq!(
            lookup(&locals, &globals, ConstRef::Local(crate::ValueId(7))).unwrap().1,
            ConstValue::I(9)
        );
    }
}

#[cfg(all(test, whale_wave_ir))]
mod wave_tests {
    use super::*;

    use crate::{ConstBinaryOp, ConstExprKind, DataLayout, Global, GlobalId};

    fn reference(id: u32) -> ConstExpr {
        ConstExpr { ty: Type::I32, kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(id))) }
    }

    fn literal() -> ConstExpr {
        ConstExpr::literal(Type::I32, ConstValue::I(0))
    }

    fn global(id: u32, init: ConstExpr) -> Global {
        Global {
            id: GlobalId(id),
            name: format!("g{id}"),
            ty: Type::I32,
            init: ConstValue::I(0),
            init_expr: init,
            align: 4,
        }
    }

    fn compare(globals: &[Global]) {
        let definitions: Vec<_> = globals
            .iter()
            .map(|g| Definition {
                id: ConstRef::Global(g.id),
                ty: &g.ty,
                expression: &g.init_expr,
                result: &g.init,
            })
            .collect();
        let expected = evaluate_reference(
            "test",
            definitions.clone(),
            Values::new(),
            &Values::new(),
            crate::IrLimits::default(),
        );
        let actual = evaluate(
            "test",
            definitions,
            Values::new(),
            &Values::new(),
            crate::IrLimits::default(),
        );
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual.entries, expected.entries),
            (Err(actual), Err(expected)) => {
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"))
            },
            (actual, expected) => {
                panic!("constant schedule divergence: {:?} / {:?}", actual.err(), expected.err())
            },
        }
    }

    #[test]
    fn wave_dependency_order_matches_reference_unknowns_duplicates_cycles_and_ready_prefixes() {
        let mut seed = 0x3523_5042u64;
        for _ in 0..500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let count = 1 + (seed as usize % 40);
            let mut globals = Vec::new();
            for id in 0..count {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let expression = match seed % 5 {
                    0 => literal(),
                    1 => reference((seed as usize % (count + 1)) as u32),
                    _ => ConstExpr {
                        ty: Type::I32,
                        kind: ConstExprKind::Binary {
                            op: ConstBinaryOp::Add,
                            left: Box::new(reference((seed as usize % (count + 1)) as u32)),
                            right: Box::new(reference(((seed >> 32) as usize % count) as u32)),
                        },
                    },
                };
                globals.push(global(id as u32, expression));
            }
            if seed % 7 == 0 && globals.len() > 1 {
                globals[1].id = globals[0].id;
            }
            if seed % 11 == 0 {
                globals[0].ty = Type::U32;
            }
            compare(&globals);
        }
        let divide = ConstExpr {
            ty: Type::I32,
            kind: ConstExprKind::Binary {
                op: ConstBinaryOp::Div,
                left: Box::new(literal()),
                right: Box::new(literal()),
            },
        };
        compare(&[global(0, reference(1)), global(1, reference(0)), global(2, divide.clone())]);
        compare(&[global(0, divide), global(1, reference(90))]);
        compare(&[
            global(1, literal()),
            global(
                2,
                ConstExpr {
                    ty: Type::I32,
                    kind: ConstExprKind::Binary {
                        op: ConstBinaryOp::Add,
                        left: Box::new(reference(1)),
                        right: Box::new(reference(1)),
                    },
                },
            ),
        ]);
    }

    #[test]
    fn local_scope_borrows_large_global_index_with_a_small_local_work_budget() {
        let mut module = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
        module.globals = (0..2000).map(|id| global(id, literal())).collect();
        let globals = verify_globals(&module, crate::IrLimits::default()).unwrap();
        let expression = reference(1999);
        let expected = ConstValue::I(0);
        for id in 0..100 {
            let definitions = vec![Definition {
                id: ConstRef::Local(crate::ValueId(id)),
                ty: &Type::I32,
                expression: &expression,
                result: &expected,
            }];
            let result = evaluate(
                "local",
                definitions,
                Values::new(),
                &globals,
                crate::IrLimits { max_analysis_work: 64, ..Default::default() },
            )
            .unwrap();
            assert_eq!(result.get(&ConstRef::Local(crate::ValueId(id))).unwrap().1, expected);
        }
    }
}
