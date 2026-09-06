// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Literal cardinality is known without approximating a binding's value type.
//! Check it once across all statement depths, outside the function-body pass
//! that deliberately filters generic type errors from its approximate env.

use super::Pretty;
use crate::ast::{Module, Node, TypeAnn};

pub(super) fn check(module: &Module, src: &str, file: Option<&str>, errors: &mut Vec<Pretty>) {
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
                check_literal(name, annotation, value, src, file, errors);
            }
            _ => {}
        });
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
