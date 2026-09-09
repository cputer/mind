// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Return and condition checks that need the enclosing function contract.
//! Kept separate from the main checker so the size ratchet does not force
//! contract validation into the already large module.
//!
//! A missing `ValueType` is intentionally not treated as a unit contract here:
//! ordinary MIND functions may omit `-> T` and infer the legacy i64 ABI. The
//! opt-in canonical source bridge applies its stricter unit/implicit-return
//! boundary after resolving the defining declaration.

use crate::ast::Node;

use super::{
    COND_TYPE_MISMATCH_CODE, Pretty, RETURN_TYPE_MISMATCH_CODE, TypeEnv, ValueType,
    cond_is_boolean_intent, describe_value_type, diag_from_span, infer_expr, is_float_scalar,
    is_int_scalar,
};

/// Early check-phase diagnostics that need the enclosing function's declared
/// return type in scope (E2010) or that inspect condition position (E2011).
/// Walks the function body's statement positions, recursing through control
/// flow but stopping at nested `FnDef`s (which carry their own return type).
/// Uses `env` = params + module symbols (NO body-local bindings): a value that
/// can't be resolved yields `Err` from `infer_expr` and is skipped, so this
/// only ever fires on confidently-typed expressions — no false positives on
/// locals. Additive: it only pushes E2010/E2011 on the sound conditions
/// documented at each code constant.
pub(super) fn check_return_and_cond_types(
    stmts: &[Node],
    ret_ty: Option<&ValueType>,
    env: &TypeEnv,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    for stmt in stmts {
        check_return_and_cond_node(stmt, ret_ty, env, src, file, errs);
    }
}

fn check_cond_type(
    cond: &Node,
    env: &TypeEnv,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    if cond_is_boolean_intent(cond) {
        return;
    }
    if let Ok((vt, _)) = infer_expr(cond, env) {
        if is_float_scalar(&vt) {
            errs.push(diag_from_span(
                src,
                file,
                format!(
                    "condition must be a boolean or integer expression, but this is {}",
                    describe_value_type(&vt)
                ),
                cond.span(),
                COND_TYPE_MISMATCH_CODE,
            ));
        }
    }
}

fn check_return_and_cond_node(
    node: &Node,
    ret_ty: Option<&ValueType>,
    env: &TypeEnv,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    match node {
        Node::Return { value, .. } => match (ret_ty, value) {
            (Some(rt), Some(v)) if is_int_scalar(rt) => {
                if let Ok((vt, _)) = infer_expr(v, env) {
                    if is_float_scalar(&vt) {
                        errs.push(diag_from_span(
                            src,
                            file,
                            format!(
                                "return type mismatch: function returns {} but this returns {}",
                                describe_value_type(rt),
                                describe_value_type(&vt)
                            ),
                            v.span(),
                            RETURN_TYPE_MISMATCH_CODE,
                        ));
                    }
                }
            }
            _ => {}
        },
        Node::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            check_cond_type(cond, env, src, file, errs);
            check_return_and_cond_types(then_branch, ret_ty, env, src, file, errs);
            if let Some(eb) = else_branch {
                check_return_and_cond_types(eb, ret_ty, env, src, file, errs);
            }
        }
        #[cfg(feature = "std-surface")]
        Node::While { cond, body, .. } => {
            check_cond_type(cond, env, src, file, errs);
            check_return_and_cond_types(body, ret_ty, env, src, file, errs);
        }
        Node::For { body, .. } | Node::ForEach { body, .. } => {
            check_return_and_cond_types(body, ret_ty, env, src, file, errs);
        }
        Node::Block { stmts, .. } => {
            check_return_and_cond_types(stmts, ret_ty, env, src, file, errs);
        }
        Node::Match { arms, .. } => {
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    check_cond_type(guard, env, src, file, errs);
                }
                check_return_and_cond_node(&arm.body, ret_ty, env, src, file, errs);
            }
        }
        #[cfg(feature = "std-surface")]
        Node::Region { body, .. } => {
            check_return_and_cond_types(body, ret_ty, env, src, file, errs);
        }
        // Nested function definitions carry their own return type; the outer
        // `ret_ty` does not apply, so do not descend into them here.
        _ => {}
    }
}
