// SPDX-License-Identifier: MPL-2.0
// Built against the pinned Wave frontend only, never a backend or mutable checkout.
use hir::{HirExpressionType, TypedProgram};

use parser::ast::*;

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn ty(value: &WaveType) -> String {
    match value {
        WaveType::Void => quote("Void"),
        WaveType::Bool => quote("Bool"),
        WaveType::Int(bits) => format!("{{\"Integer\":{{\"bits\":{bits},\"signed\":true}}}}"),
        WaveType::Uint(bits) => format!("{{\"Integer\":{{\"bits\":{bits},\"signed\":false}}}}"),
        other => format!("{{\"Unsupported\":{}}}", quote(&format!("{other:?}"))),
    }
}

fn expression(program: &TypedProgram, value: &Expression) -> String {
    let id = program.expression_id(value).expect("owned typed expression").index();
    let resolved = program
        .numeric_expression_of(value)
        .map(|info| &info.result_type)
        .or_else(|| match program.type_of(value) {
            Some(HirExpressionType::Resolved(ty)) => Some(ty),
            _ => None,
        })
        .expect("resolved scalar HIR fact");
    let kind = match value.unspanned() {
        Expression::Literal(Literal::Int(value)) => format!("{{\"Integer\":{}}}", quote(value)),
        Expression::Literal(Literal::Bool(value)) => format!("{{\"Bool\":{value}}}"),
        Expression::Variable(name) => format!("{{\"Local\":{}}}", quote(name)),
        Expression::BinaryExpression { left, operator, right } => {
            let operator = match operator {
                Operator::Add => "Add",
                Operator::Subtract => "Subtract",
                Operator::Multiply => "Multiply",
                Operator::Divide => "Divide",
                Operator::Remainder => "Remainder",
                Operator::GreaterEqual => "GreaterEqual",
                Operator::LessEqual => "LessEqual",
                Operator::Greater => "Greater",
                Operator::Less => "Less",
                Operator::Equal => "Equal",
                Operator::NotEqual => "NotEqual",
                Operator::LogicalAnd => "LogicalAnd",
                Operator::LogicalOr => "LogicalOr",
                Operator::BitwiseAnd => "BitwiseAnd",
                Operator::BitwiseOr => "BitwiseOr",
                Operator::BitwiseXor => "BitwiseXor",
                Operator::ShiftLeft => "ShiftLeft",
                Operator::ShiftRight => "ShiftRight",
                _ => panic!("fixture has unsupported operator"),
            };
            format!(
                "{{\"Binary\":{{\"left\":{},\"operator\":{},\"right\":{}}}}}",
                expression(program, left),
                quote(operator),
                expression(program, right)
            )
        },
        _ => panic!("fixture has unsupported expression"),
    };
    format!("{{\"id\":{id},\"resolved_type\":{},\"kind\":{kind}}}", ty(resolved))
}

fn statements(program: &TypedProgram, body: &[ASTNode]) -> String {
    let values: Vec<String> = body
        .iter()
        .map(|node| match node.unspanned() {
            ASTNode::Variable(value) => {
                assert!(matches!(value.mutability, Mutability::Var));
                format!(
                    "{{\"Variable\":{{\"name\":{},\"ty\":{},\"init\":{}}}}}",
                    quote(&value.name),
                    ty(&value.type_name),
                    value
                        .initial_value
                        .as_ref()
                        .map(|value| expression(program, value))
                        .unwrap_or_else(|| "null".into())
                )
            },
            ASTNode::Statement(StatementNode::Assign { variable, value }) => format!(
                "{{\"Assign\":{{\"name\":{},\"value\":{}}}}}",
                quote(variable),
                expression(program, value)
            ),
            ASTNode::Statement(StatementNode::While { condition, body }) => format!(
                "{{\"While\":{{\"condition\":{},\"body\":{}}}}}",
                expression(program, condition),
                statements(program, body)
            ),
            ASTNode::Statement(StatementNode::If {
                condition,
                body,
                else_if_blocks,
                else_block,
            }) => {
                assert!(else_if_blocks.is_none());
                format!(
                    "{{\"If\":{{\"condition\":{},\"body\":{},\"else_body\":{}}}}}",
                    expression(program, condition),
                    statements(program, body),
                    statements(program, else_block.as_deref().map(Vec::as_slice).unwrap_or(&[]))
                )
            },
            ASTNode::Statement(StatementNode::Return(value)) => format!(
                "{{\"Return\":{}}}",
                value
                    .as_ref()
                    .map(|value| expression(program, value))
                    .unwrap_or_else(|| "null".into())
            ),
            _ => panic!("fixture has unsupported statement"),
        })
        .collect();
    format!("[{}]", values.join(","))
}

fn variable(name: &str) -> Expression {
    Expression::Variable(name.into())
}

fn binary(left: Expression, operator: Operator, right: Expression) -> Expression {
    Expression::BinaryExpression { left: Box::new(left), operator, right: Box::new(right) }
}

fn main() {
    // This is the final syntax passed to TypedProgram::lower after imports and
    // monomorphization: one concrete scalar function, no frontend/backend state.
    let program = TypedProgram::lower(vec![ASTNode::Function(FunctionNode {
        is_async: false,
        span: None,
        name: "hir_scalar".into(),
        generic_params: vec![],
        parameters: vec![
            ParameterNode {
                span: None,
                name: "x".into(),
                param_type: WaveType::Int(32),
                initial_value: None,
            },
            ParameterNode {
                span: None,
                name: "n".into(),
                param_type: WaveType::Int(32),
                initial_value: None,
            },
            ParameterNode {
                span: None,
                name: "flag".into(),
                param_type: WaveType::Bool,
                initial_value: None,
            },
        ],
        return_type: Some(WaveType::Int(32)),
        return_type_span: None,
        body: vec![
            ASTNode::Variable(VariableNode {
                name: "acc".into(),
                type_name: WaveType::Int(32),
                initial_value: Some(variable("x")),
                mutability: Mutability::Var,
                visibility: Visibility::Private,
            }),
            ASTNode::Statement(StatementNode::While {
                condition: binary(variable("acc"), Operator::Less, variable("n")),
                body: vec![ASTNode::Statement(StatementNode::Assign {
                    variable: "acc".into(),
                    value: binary(
                        variable("acc"),
                        Operator::Add,
                        Expression::Literal(Literal::Int("1".into())),
                    ),
                })],
            }),
            ASTNode::Statement(StatementNode::If {
                condition: variable("flag"),
                body: vec![ASTNode::Statement(StatementNode::Return(Some(variable("acc"))))],
                else_if_blocks: None,
                else_block: Some(Box::new(vec![ASTNode::Statement(StatementNode::Return(Some(
                    variable("x"),
                )))])),
            }),
        ],
        export: None,
        visibility: Visibility::Private,
    })])
    .expect("pinned HIR semantic validation");
    program.verify_conversions().expect("pinned HIR conversion facts");
    let functions:Vec<_>=program.syntax().iter().map(|node|{
        let ASTNode::Function(function)=node.unspanned() else {panic!("only scalar functions")};
        let params:Vec<_>=function.parameters.iter().map(|p|format!("{{\"name\":{},\"ty\":{}}}",quote(&p.name),ty(&p.param_type))).collect();
        format!("{{\"name\":{},\"parameters\":[{}],\"return_type\":{},\"body\":{},\"is_async\":{}}}",quote(&function.name),params.join(","),ty(function.return_type.as_ref().unwrap_or(&WaveType::Void)),statements(&program,&function.body),function.is_async)
    }).collect();
    println!(
        "{{\"wave_revision\":\"e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b\",\"functions\":[{}]}}",
        functions.join(",")
    );
}
