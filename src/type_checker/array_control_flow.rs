// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Branch-tail checks for the narrow fixed-array consistency pass.

use super::{Pretty, StructFieldChecker, is_exact_i64, unwrap_parens};
use crate::ast::{Literal, Node, TypeAnn};
use std::collections::BTreeMap;

impl StructFieldChecker<'_> {
    /// Check an array element when its enclosing schema proves that i64 is
    /// required. Unsupported expressions remain deferred; this structural
    /// walk only exposes known facts in control-flow tails.
    pub(super) fn check_i64_element(
        &mut self,
        node: &Node,
        env: &BTreeMap<String, TypeAnn>,
        struct_name: &str,
        field_name: &str,
    ) {
        let node = unwrap_parens(node);
        match node {
            Node::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                let mut cond_env = env.clone();
                self.visit_expr(cond, &mut cond_env);
                self.check_i64_branch_tail(then_branch, env, struct_name, field_name);
                if let Some(else_branch) = else_branch {
                    self.check_i64_branch_tail(else_branch, env, struct_name, field_name);
                }
            }
            Node::Block { stmts, .. } => {
                self.check_i64_branch_tail(stmts, env, struct_name, field_name);
            }
            Node::Return {
                value: Some(value), ..
            } => {
                self.check_i64_element(value, env, struct_name, field_name);
            }
            _ => {
                if self
                    .expr_type(node, env)
                    .is_some_and(|ty| !is_exact_i64(&ty))
                {
                    self.push_error(
                        format!(
                            "array element type mismatch for `{struct_name}.{field_name}`: expected i64"
                        ),
                        node.span(),
                    );
                }
            }
        }
    }

    fn check_i64_branch_tail(
        &mut self,
        stmts: &[Node],
        env: &BTreeMap<String, TypeAnn>,
        struct_name: &str,
        field_name: &str,
    ) {
        let mut branch_env = env.clone();
        let Some((tail, prefix)) = stmts.split_last() else {
            return;
        };
        for stmt in prefix {
            self.visit_stmt(stmt, &mut branch_env);
        }
        self.check_i64_element(tail, &branch_env, struct_name, field_name);
    }
}

pub(super) fn check_integer_return_value(
    value: &Node,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let mut value = value;
    while let Node::Paren(inner, _) = value {
        value = inner;
    }
    match value {
        Node::If {
            then_branch,
            else_branch,
            ..
        } => {
            check_integer_return_tail(then_branch, fn_name, src, file, errors);
            if let Some(else_branch) = else_branch {
                check_integer_return_tail(else_branch, fn_name, src, file, errors);
            }
        }
        Node::Block { stmts, .. } => {
            check_integer_return_tail(stmts, fn_name, src, file, errors);
        }
        _ if is_float_literal(value) => {
            errors.push(super::super::diag_from_span(
                src,
                file,
                format!(
                    "array element type mismatch for return from `{fn_name}`: integer element cannot use a float literal"
                ),
                value.span(),
                super::super::TYPE_ERR_CODE,
            ));
        }
        _ => {}
    }
}

fn check_integer_return_tail(
    stmts: &[Node],
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let Some((tail, _)) = stmts.split_last() else {
        return;
    };
    match tail {
        Node::Let { .. } | Node::LetTuple { .. } | Node::Assign { .. } => {}
        Node::Return { .. } => {}
        _ => check_integer_return_value(tail, fn_name, src, file, errors),
    }
}

fn is_float_literal(node: &Node) -> bool {
    match node {
        Node::Lit(Literal::Float(_), _) => true,
        Node::Paren(inner, _) | Node::Neg { operand: inner, .. } => is_float_literal(inner),
        _ => false,
    }
}
