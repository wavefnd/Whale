#![cfg(all(whale_wave_ir, feature = "socket"))]
// SPDX-License-Identifier: MPL-2.0
use ir::lower_ast::{wave_hir as h, AstLimits};

use ir::*;

fn fixture() -> h::Program {
    serde_json::from_str(include_str!("fixtures/wave_typed_hir_scalar.json")).unwrap()
}

fn int() -> h::ResolvedType {
    h::ResolvedType::Integer { bits: 32, signed: true }
}

fn literal(id: u64, value: &str) -> h::Expression {
    h::Expression { id, resolved_type: int(), kind: h::ExpressionKind::Integer(value.into()) }
}

fn program(expression: h::Expression) -> h::Program {
    h::Program {
        wave_revision: h::WAVE_HIR_REVISION.into(),
        functions: vec![h::Function {
            name: "test".into(),
            parameters: vec![],
            return_type: expression.resolved_type.clone(),
            body: vec![h::Statement::Return(Some(expression))],
            is_async: false,
        }],
    }
}

#[test]
fn actual_pinned_typed_hir_fixture_lowers_verifies_executes_and_preserves_identity() {
    let program = fixture();
    let module = h::lower(
        &program,
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
        AstLimits::default(),
    )
    .unwrap();
    verify_module(&module).unwrap();
    let function = module.functions.iter().find(|f| f.name == "hir_scalar").unwrap().id;
    for (x, n, flag, expected) in [(2, 5, true, 5), (2, 5, false, 2), (10, 5, true, 10)] {
        let result = interpret(
            &module,
            function,
            &[ConstValue::I(x), ConstValue::I(n), ConstValue::Bool(flag)],
        )
        .unwrap();
        assert_eq!(result.value, Some(ConstValue::I(expected)));
    }
    fn expression(expr: &h::Expression, ids: &mut Vec<u64>) {
        ids.push(expr.id);
        if let h::ExpressionKind::Binary { left, right, .. } = &expr.kind {
            expression(left, ids);
            expression(right, ids);
        }
    }

    fn statements(values: &[h::Statement], ids: &mut Vec<u64>) {
        for statement in values {
            match statement {
                h::Statement::Variable { init: Some(value), .. }
                | h::Statement::Assign { value, .. }
                | h::Statement::Return(Some(value))
                | h::Statement::Expression(value) => expression(value, ids),
                h::Statement::If { condition, body, else_body } => {
                    expression(condition, ids);
                    statements(body, ids);
                    statements(else_body, ids);
                },
                h::Statement::While { condition, body } => {
                    expression(condition, ids);
                    statements(body, ids);
                },
                _ => {},
            }
        }
    }
    let mut expected = vec![];
    statements(&program.functions[0].body, &mut expected);
    expected.sort_unstable();
    let mut actual: Vec<_> = module.sources.originals.iter().map(|original| original.id).collect();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn resolved_type_mismatches_unknown_bindings_and_duplicate_ids_fail_transactionally() {
    let mut input = program(literal(7, "2"));
    let before = serde_json::to_value(&input).unwrap();
    input.functions[0].return_type = h::ResolvedType::Bool;
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::ResolvedType { .. })
    ));
    input = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(serde_json::to_value(&input).unwrap(), before);
    input.functions[0].body = vec![h::Statement::Return(Some(h::Expression {
        id: 2,
        resolved_type: int(),
        kind: h::ExpressionKind::Local("absent".into()),
    }))];
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::UnknownBinding { .. })
    ));
    input = program(h::Expression {
        id: 7,
        resolved_type: int(),
        kind: h::ExpressionKind::Binary {
            left: Box::new(literal(7, "1")),
            operator: h::Operator::Add,
            right: Box::new(literal(9, "2")),
        },
    });
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::DuplicateExpression { id: 7 })
    ));
    input = program(literal(1, "2147483648"));
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::NumericLiteral { .. })
    ));
    input = program(literal(1, "1"));
    input.wave_revision = "other".into();
    assert!(matches!(h::adapt(&input, AstLimits::default()), Err(h::AdapterError::Revision)));
}

#[test]
fn calls_aggregates_async_and_inline_assembly_are_explicit_whole_program_errors() {
    for kind in [
        h::Unsupported::Call,
        h::Unsupported::Aggregate,
        h::Unsupported::Async,
        h::Unsupported::InlineAssembly,
    ] {
        let mut input = fixture();
        input.functions[0].body.push(h::Statement::Unsupported(kind));
        let before = serde_json::to_value(&input).unwrap();
        assert!(
            matches!(h::adapt(&input,AstLimits::default()),Err(h::AdapterError::Unsupported{category,..}) if category==kind as u64)
        );
        assert_eq!(serde_json::to_value(&input).unwrap(), before);
    }
    let mut input = fixture();
    input.functions[0].is_async = true;
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::Unsupported { category: 3, .. })
    ));
}

#[test]
fn all_scalar_operators_use_resolved_facts_and_reference_execution() {
    for operator in [
        h::Operator::Add,
        h::Operator::Subtract,
        h::Operator::Multiply,
        h::Operator::Divide,
        h::Operator::Remainder,
        h::Operator::BitwiseAnd,
        h::Operator::BitwiseOr,
        h::Operator::BitwiseXor,
        h::Operator::ShiftLeft,
        h::Operator::ShiftRight,
        h::Operator::Equal,
        h::Operator::NotEqual,
        h::Operator::Less,
        h::Operator::LessEqual,
        h::Operator::Greater,
        h::Operator::GreaterEqual,
    ] {
        let compare = operator as u64 >= 12;
        let input = program(h::Expression {
            id: 2,
            resolved_type: if compare { h::ResolvedType::Bool } else { int() },
            kind: h::ExpressionKind::Binary {
                left: Box::new(literal(0, "7")),
                operator,
                right: Box::new(literal(1, "2")),
            },
        });
        let module = h::lower(
            &input,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
            AstLimits::default(),
        )
        .unwrap();
        verify_module(&module).unwrap();
        let result = interpret(&module, module.functions[0].id, &[]).unwrap().value.unwrap();
        let expected = match operator {
            h::Operator::Add => ConstValue::I(9),
            h::Operator::Subtract => ConstValue::I(5),
            h::Operator::Multiply => ConstValue::I(14),
            h::Operator::Divide => ConstValue::I(3),
            h::Operator::Remainder => ConstValue::I(1),
            h::Operator::BitwiseAnd => ConstValue::I(2),
            h::Operator::BitwiseOr => ConstValue::I(7),
            h::Operator::BitwiseXor => ConstValue::I(5),
            h::Operator::ShiftLeft => ConstValue::I(28),
            h::Operator::ShiftRight => ConstValue::I(1),
            h::Operator::Equal => ConstValue::Bool(false),
            h::Operator::NotEqual => ConstValue::Bool(true),
            h::Operator::Less | h::Operator::LessEqual => ConstValue::Bool(false),
            h::Operator::Greater | h::Operator::GreaterEqual => ConstValue::Bool(true),
            _ => unreachable!(),
        };
        assert_eq!(result, expected, "{operator:?}");
    }
    for operator in [h::Operator::LogicalAnd, h::Operator::LogicalOr] {
        let input = program(h::Expression {
            id: 2,
            resolved_type: h::ResolvedType::Bool,
            kind: h::ExpressionKind::Binary {
                left: Box::new(h::Expression {
                    id: 0,
                    resolved_type: h::ResolvedType::Bool,
                    kind: h::ExpressionKind::Bool(false),
                }),
                operator,
                right: Box::new(h::Expression {
                    id: 1,
                    resolved_type: h::ResolvedType::Bool,
                    kind: h::ExpressionKind::Bool(true),
                }),
            },
        });
        let module = h::lower(
            &input,
            "x86_64-whale-linux",
            DataLayout::default_64bit_le(),
            AstLimits::default(),
        )
        .unwrap();
        assert_eq!(
            interpret(&module, module.functions[0].id, &[]).unwrap().value,
            Some(ConstValue::Bool(matches!(operator, h::Operator::LogicalOr)))
        );
    }
}

#[test]
fn typed_hir_adapter_limits_and_lexical_branch_bindings_are_checked() {
    let input = fixture();
    for limits in [
        AstLimits { max_nodes: 1, ..AstLimits::default() },
        AstLimits { max_depth: 1, ..AstLimits::default() },
        AstLimits { max_string_bytes: 1, ..AstLimits::default() },
        AstLimits { max_source_work: 1, ..AstLimits::default() },
    ] {
        assert!(matches!(h::adapt(&input, limits), Err(h::AdapterError::Resource)));
    }
    let mut input = program(h::Expression {
        id: 2,
        resolved_type: int(),
        kind: h::ExpressionKind::Local("branch_local".into()),
    });
    input.functions[0].body.insert(
        0,
        h::Statement::If {
            condition: h::Expression {
                id: 0,
                resolved_type: h::ResolvedType::Bool,
                kind: h::ExpressionKind::Bool(true),
            },
            body: vec![h::Statement::Variable {
                name: "branch_local".into(),
                ty: int(),
                init: Some(literal(1, "1")),
            }],
            else_body: vec![],
        },
    );
    assert!(matches!(
        h::adapt(&input, AstLimits::default()),
        Err(h::AdapterError::UnknownBinding { .. })
    ));
}
