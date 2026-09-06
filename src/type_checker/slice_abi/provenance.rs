// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Proof of the Vec-layout owner or borrowed capability carried by a value.

use super::{
    Env, HandleKind, call_signature, confident_scalar_class, declared_kind, scalar_class_of_ann,
    value_contains_borrow,
};
use crate::ast::{Literal, Node, TypeAnn};

pub(super) fn expr_kind(node: &Node, env: &Env) -> Option<HandleKind> {
    match node {
        Node::ArrayLit { .. } => None,
        Node::Lit(Literal::Ident(name), _) => env.handles.get(name).cloned(),
        Node::Paren(inner, _) => expr_kind(inner, env),
        // `check_fn` validates a declared collection result before it authorizes callers.
        Node::Call { callee, .. } => call_signature(callee)
            .and_then(|(_, ret)| ret)
            .as_ref()
            .and_then(declared_kind),
        // Only Vec::push returns a replacement owner handle. In particular,
        // Vec::set returns a scalar status and must never acquire this proof.
        Node::MethodCall {
            receiver,
            method,
            args,
            ..
        } if method == "push" && args.len() == 1 => {
            expr_kind(receiver, env).filter(|kind| matches!(kind, HandleKind::Array(_)))
        }
        _ => None,
    }
}

fn compatible_array_literal(node: &Node, target_element: &TypeAnn, env: &Env) -> bool {
    let Node::ArrayLit { elements, .. } = node else {
        return false;
    };
    if elements.is_empty() {
        return true;
    }
    if let Some(target_class) = scalar_class_of_ann(target_element) {
        return elements
            .iter()
            .all(|element| confident_scalar_class(element, &env.classes) == Some(target_class));
    }
    elements.iter().all(|element| {
        expr_type(element, env)
            .as_ref()
            .is_some_and(|actual| same_type(actual, target_element))
    })
}

pub(super) fn expr_type(node: &Node, env: &Env) -> Option<TypeAnn> {
    match node {
        Node::Lit(Literal::Ident(name), _) => env.types.get(name).cloned(),
        Node::Lit(Literal::Str(_), _) => Some(TypeAnn::Named("string".to_string())),
        Node::StructLit { name, .. } => Some(TypeAnn::Named(name.clone())),
        Node::Paren(inner, _) => expr_type(inner, env),
        Node::Call { callee, .. } => call_signature(callee).and_then(|(_, ret)| ret),
        _ => None,
    }
}

pub(super) fn same_type(left: &TypeAnn, right: &TypeAnn) -> bool {
    left == right
        || matches!(
            (left, right),
            (TypeAnn::Named(a), TypeAnn::Named(b))
                if matches!(a.as_str(), "string" | "String")
                    && matches!(b.as_str(), "string" | "String")
        )
}

pub(super) fn compatible_source(target: &HandleKind, value: &Node, env: &Env) -> bool {
    compatible_array_literal(value, target.element(), env)
        || expr_kind(value, env).as_ref().is_some_and(|source| {
            same_type(source.element(), target.element())
                && match target {
                    HandleKind::Array(_) => matches!(source, HandleKind::Array(_)),
                    HandleKind::Slice { mutable: false, .. } => true,
                    HandleKind::Slice { mutable: true, .. } => matches!(
                        source,
                        HandleKind::Array(_) | HandleKind::Slice { mutable: true, .. }
                    ),
                }
        })
        // Existing public raw-handle interop explicitly retypes an opaque i64
        // identifier/call as an owned array. Borrow flow is checked first, so a
        // slice can never reach this compatibility lane by erasing its type.
        || (matches!(target, HandleKind::Array(_))
            && !value_contains_borrow(value, env)
            && matches!(value, Node::Lit(Literal::Ident(_), _) | Node::Call { .. }))
}
