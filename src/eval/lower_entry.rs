// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Thin lowering entry wrapper used to keep the legacy lowerer under its
//! established source-size ceiling.

#[cfg(feature = "cross-module-imports")]
use super::canonical_producers;
use super::{HashMap, IRModule, LoweringContext, ValueId, ast};

#[inline]
pub(super) fn lower_expr(
    node: &ast::Node,
    ir: &mut IRModule,
    env: &HashMap<String, ValueId>,
    struct_env: &HashMap<String, String>,
    receiver_types: &HashMap<crate::ast::Span, String>,
    context: &mut LoweringContext,
) -> ValueId {
    let id = super::lower_expr_inner(node, ir, env, struct_env, receiver_types, context);
    #[cfg(feature = "cross-module-imports")]
    if context.canonical_active() && !context.canonical_failed() {
        canonical_producers::record_checked_expr(node, id, context);
    }
    id
}

#[inline]
pub(super) fn validated_value(
    _context: &mut LoweringContext,
    _source: &ast::Node,
    value: ValueId,
) -> ValueId {
    #[cfg(feature = "cross-module-imports")]
    _context.canonical_validate_return(_source.span(), value);
    value
}

#[inline]
pub(super) fn ret(
    context: &mut LoweringContext,
    source: Option<&ast::Node>,
    value: Option<ValueId>,
) -> Option<ValueId> {
    if let (Some(source), Some(value)) = (source, value) {
        Some(validated_value(context, source, value))
    } else {
        value
    }
}
