// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Preserve known struct identity separately from the loose scalar/tensor ABI.

use super::{
    ClassCtx, Pretty, SCALAR_INTO_STRUCT_CODE, confident_scalar_class, diag_from_span,
    intra_lookup_fn,
};
use crate::ast::{Literal, Node, TypeAnn};
use std::collections::BTreeSet;

fn struct_value_name(node: &Node, ctx: &ClassCtx) -> Option<String> {
    match node {
        Node::StructLit { name, .. } if struct_name_in_scope(name) => Some(name.clone()),
        Node::Paren(inner, _) => struct_value_name(inner, ctx),
        Node::Lit(Literal::Ident(name), _) => ctx.structs.get(name).cloned(),
        Node::Call { callee, .. } => intra_lookup_fn(callee).and_then(|sig| match sig.ret_type {
            Some(TypeAnn::Named(name)) if struct_name_in_scope(&name) => Some(name),
            _ => None,
        }),
        _ => None,
    }
}

/// Inline modules are transparent declaration blocks. Do not descend into
/// function bodies: their local declarations belong to a separate scope.
pub(super) fn collect_struct_names(items: &[Node], names: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Node::StructDef { name, .. } => {
                names.insert(name.clone());
            }
            Node::Block { stmts, .. } => collect_struct_names(stmts, names),
            _ => {}
        }
    }
}

pub(super) fn seed_let(name: &str, ann: &Option<TypeAnn>, value: &Node, ctx: &mut ClassCtx) {
    let struct_name = match ann {
        Some(TypeAnn::Named(ty)) if struct_name_in_scope(ty) => Some(ty.clone()),
        Some(_) => None,
        None => struct_value_name(value, ctx),
    };
    if let Some(ty) = struct_name {
        ctx.structs.insert(name.to_string(), ty);
    } else {
        ctx.structs.remove(name);
    }
}

pub(super) fn check_assignment(
    name: &str,
    value: &Node,
    ctx: &ClassCtx,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    if let Some(struct_name) = ctx
        .structs
        .get(name)
        .filter(|_| confident_scalar_class(value, ctx).is_some())
    {
        let message = format!(
            "cannot replace struct binding `{name}` (`{struct_name}`) with a scalar; construct a struct value"
        );
        errs.push(diag_from_span(
            src,
            file,
            message,
            value.span(),
            SCALAR_INTO_STRUCT_CODE,
        ));
    }
}

// ── E2026 — locally-declared struct names ─────────────────────────────
//
// The names of every module-level `struct` in the file being checked, so the
// `let` arm can recognise a struct-typed annotation (`let v: Value = …`) even
// inside the FnDef-body mini-module recursion (whose sub-module holds only the
// body statements, NO `StructDef` items). Same merge-on-install / restore-on-
// drop discipline as `INTRA_FN_SIGS` / `FIXED_BYTES_LOCALS`.
thread_local! {
    static STRUCT_NAMES: std::cell::RefCell<Option<BTreeSet<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// True iff `name` is a `struct` declared in the module currently being
/// checked. `false` when the side-table is unpopulated (no module context).
pub(super) fn struct_name_in_scope(name: &str) -> bool {
    STRUCT_NAMES.with(|cell| cell.borrow().as_ref().is_some_and(|set| set.contains(name)))
}

/// RAII guard for `STRUCT_NAMES`; merges onto any parent table (so the fn-body
/// recursion keeps the enclosing module's structs visible) and restores the
/// previous table on drop.
pub(super) struct StructNamesGuard {
    prev: Option<BTreeSet<String>>,
}

impl StructNamesGuard {
    pub(super) fn install(names: BTreeSet<String>) -> Self {
        let prev = STRUCT_NAMES.with(|cell| {
            let mut slot = cell.borrow_mut();
            let prev = slot.clone();
            match slot.as_mut() {
                Some(existing) => existing.extend(names),
                None => *slot = Some(names),
            }
            prev
        });
        StructNamesGuard { prev }
    }
}

impl Drop for StructNamesGuard {
    fn drop(&mut self) {
        STRUCT_NAMES.with(|cell| *cell.borrow_mut() = self.prev.take());
    }
}
