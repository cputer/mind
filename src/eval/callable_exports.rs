// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Classify the callable subset of source exports for optional C ABI emission.

use crate::{ast, ir::IRModule};

pub(super) fn retain_c_abi_exports(items: &[ast::Node], ir: &mut IRModule) {
    if ir.exports.is_empty() {
        return;
    }
    for name in source_non_callable_exports(items) {
        ir.exports.remove(&name);
    }
}

/// Return source-exported declarations that are known not to be callable.
///
/// The language export surface includes type declarations for cross-module
/// type resolution, while the C ABI export surface is function-only. Keep the
/// distinction at AST→IR lowering so `IRModule::exports` contains no wrappers
/// for aliases/structs/constants; unknown names remain in the set and are
/// rejected by the C-export emitter instead of disappearing silently.
fn source_non_callable_exports(items: &[ast::Node]) -> std::collections::BTreeSet<String> {
    let mut explicit = std::collections::BTreeSet::new();
    let mut callable = std::collections::BTreeSet::new();
    let mut non_callable = std::collections::BTreeSet::new();

    fn collect(
        items: &[ast::Node],
        explicit: &mut std::collections::BTreeSet<String>,
        callable: &mut std::collections::BTreeSet<String>,
        non_callable: &mut std::collections::BTreeSet<String>,
    ) {
        for item in items {
            match item {
                ast::Node::Export { names, .. } => explicit.extend(names.iter().cloned()),
                ast::Node::FnDef(fd, _) => {
                    callable.insert(fd.name.clone());
                }
                ast::Node::StructDef { name, .. }
                | ast::Node::EnumDef { name, .. }
                | ast::Node::TypeAlias { name, .. }
                | ast::Node::Const { name, .. }
                | ast::Node::ExternConst { name, .. } => {
                    non_callable.insert(name.clone());
                }
                ast::Node::Block { stmts, .. } => {
                    collect(stmts, explicit, callable, non_callable);
                }
                _ => {}
            }
        }
    }

    collect(items, &mut explicit, &mut callable, &mut non_callable);
    explicit
        .intersection(&non_callable)
        .filter(|name| !callable.contains(*name))
        .cloned()
        .collect()
}
