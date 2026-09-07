// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Literal cardinality and literal element classes are known without
//! approximating a binding's value type. Check them outside the function-body
//! pass that deliberately filters generic type errors from its approximate env.

use super::Pretty;

use crate::ast::{Literal, Module, Node, TypeAnn};

pub(super) fn check(module: &Module, src: &str, file: Option<&str>, errors: &mut Vec<Pretty>) {
    let aliases = crate::eval::type_aliases::LocalTypeAliases::new(&module.items);
    check_struct_array_fields(&module.items, &aliases, src, file, errors);
    for item in &module.items {
        super::nerve_walk::walk(item, &mut |node| match node {
            Node::Let {
                name,
                ann: Some(annotation),
                value,
                ..
            }
            | Node::Const {
                name,
                ty: Some(annotation),
                value,
                ..
            } => {
                check_literal(name, &aliases.resolve(annotation), value, src, file, errors);
            }
            Node::FnDef(fd, _) => check_fn_returns(fd, &aliases, src, file, errors),
            _ => {}
        });
    }
}

/// The inline scalar-cell ABI has a representation only for fixed-array
/// fields whose elements are the supported i64/f64 scalar cells. Refuse other
/// element types at check time so they cannot reach the lowering backstop as
/// an opaque aggregate and fail with a panic or backend type error.
fn check_struct_array_fields(
    items: &[Node],
    aliases: &crate::eval::type_aliases::LocalTypeAliases,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    for item in items {
        match item {
            Node::StructDef { name, fields, .. } => {
                for field in fields {
                    let TypeAnn::Array { element, .. } = aliases.resolve(&field.ty) else {
                        continue;
                    };
                    if !matches!(element.as_ref(), TypeAnn::ScalarI64 | TypeAnn::ScalarF64) {
                        errors.push(super::diag_from_span(
                            src,
                            file,
                            format!(
                                "fixed struct-array field `{name}.{}` has an unsupported element type",
                                field.name
                            ),
                            field.span,
                            super::TYPE_ERR_CODE,
                        ));
                    }
                }
            }
            Node::Block { stmts, .. } => {
                check_struct_array_fields(stmts, aliases, src, file, errors);
            }
            _ => {}
        }
    }
}

/// Check only functions whose resolved return annotation is a fixed array.
/// Scalar functions stay on the existing type-check path. The surrounding
/// nerve walk invokes this independently for nested function declarations.
fn check_fn_returns(
    fd: &crate::ast::FnDefData,
    aliases: &crate::eval::type_aliases::LocalTypeAliases,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let Some(ret) = fd.ret_type.as_ref() else {
        return;
    };
    let expected = aliases.resolve(ret);
    if !matches!(expected, TypeAnn::Array { .. }) {
        return;
    }
    for stmt in &fd.body {
        check_return_node(stmt, &expected, fd.name.as_str(), src, file, errors);
    }
    if let Some(pos) = fd.body.iter().rposition(|stmt| {
        !matches!(
            stmt,
            Node::Let { .. } | Node::LetTuple { .. } | Node::Assign { .. }
        )
    }) {
        check_tail_node(
            &fd.body[pos],
            &expected,
            fd.name.as_str(),
            src,
            file,
            errors,
        );
    }
}

fn check_return_node(
    node: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    match node {
        Node::Return {
            value: Some(value), ..
        } => check_return_value(value, expected, fn_name, src, file, errors),
        // The outer cardinality walk invokes every nested declaration with its
        // own return annotation; never apply the parent's type here.
        Node::FnDef(..) => {}
        _ => super::nerve_walk::for_each_child(node, &mut |child| {
            check_return_node(child, expected, fn_name, src, file, errors)
        }),
    }
}

fn check_tail_node(
    node: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    match node {
        Node::Return { .. } => {}
        Node::If {
            then_branch,
            else_branch,
            ..
        } => {
            check_tail_sequence(then_branch, expected, fn_name, src, file, errors);
            if let Some(else_branch) = else_branch {
                check_tail_sequence(else_branch, expected, fn_name, src, file, errors);
            }
        }
        Node::Block { stmts, .. } => {
            check_tail_sequence(stmts, expected, fn_name, src, file, errors);
        }
        Node::Match { arms, .. } => {
            for arm in arms {
                check_tail_node(&arm.body, expected, fn_name, src, file, errors);
            }
        }
        _ => check_return_value(node, expected, fn_name, src, file, errors),
    }
}

fn check_tail_sequence(
    stmts: &[Node],
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    if let Some(pos) = stmts.iter().rposition(|stmt| {
        !matches!(
            stmt,
            Node::Let { .. } | Node::LetTuple { .. } | Node::Assign { .. }
        )
    }) {
        check_tail_node(&stmts[pos], expected, fn_name, src, file, errors);
    }
}

fn check_return_value(
    value: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let mut value = value;
    while let Node::Paren(inner, _) = value {
        value = inner;
    }
    match expected {
        TypeAnn::Array { element, length } => {
            if let Node::ArrayLit { elements, .. } = value {
                if elements.len() != *length as usize {
                    errors.push(super::diag_from_span(
                        src,
                        file,
                        format!(
                            "array length mismatch for return from `{fn_name}`: annotation [_; {length}] but value has {} elements",
                            elements.len()
                        ),
                        value.span(),
                        super::TYPE_ERR_CODE,
                    ));
                }
                for item in elements {
                    check_return_value(item, element, fn_name, src, file, errors);
                }
            }
        }
        integer
            if super::scalar_class_of_ann(integer) == Some(super::ScalarClass::Int)
                && is_float_literal(value) =>
        {
            // Fixed-array float construction accepts integer constants; only
            // the reverse literal conversion is rejected here.
            errors.push(super::diag_from_span(
                src,
                file,
                format!(
                    "array element type mismatch for return from `{fn_name}`: integer element cannot use a float literal"
                ),
                value.span(),
                super::TYPE_ERR_CODE,
            ));
        }
        _ => {}
    }
}

fn is_float_literal(node: &Node) -> bool {
    match node {
        Node::Lit(Literal::Float(_), _) => true,
        Node::Paren(inner, _) | Node::Neg { operand: inner, .. } => is_float_literal(inner),
        _ => false,
    }
}

fn check_literal(
    name: &str,
    annotation: &TypeAnn,
    value: &Node,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let TypeAnn::Array { element, length } = annotation else {
        return;
    };
    let mut literal = value;
    while let Node::Paren(inner, _) = literal {
        literal = inner;
    }
    let Node::ArrayLit { elements, .. } = literal else {
        return;
    };
    if elements.len() != *length as usize {
        errors.push(super::diag_from_span(
            src,
            file,
            format!(
                "array length mismatch for `{name}`: annotation [_; {length}] but literal has {} elements",
                elements.len()
            ),
            literal.span(),
            super::TYPE_ERR_CODE,
        ));
    }
    if matches!(element.as_ref(), TypeAnn::Array { .. }) {
        for item in elements {
            check_literal(name, element, item, src, file, errors);
        }
    }
}
