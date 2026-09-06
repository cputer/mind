// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Cached return metadata for the bundled pure-MIND standard library.

use std::sync::OnceLock;

/// The single-file resolver exposes bundled std names through its cached
/// name-only surface, but it deliberately has no active project module table.
/// Keep the return metadata on parsed declarations so an unannotated
/// `let s = string_push_byte(...);` can retain String ownership for a later
/// method call. The cache avoids reparsing bundled modules for every source
/// compiled on this thread. Under `cross-module-imports`, the existing
/// `ExportedFn` collector supplies the declarations; the std-surface-only build
/// walks the same parsed `FnDef`s because its module table is cfg-gated off.
pub(crate) fn bundled_std_fn_signatures() -> &'static Vec<(
    String,
    Vec<crate::ast::TypeAnn>,
    Option<crate::ast::TypeAnn>,
)> {
    type CachedSignature = (
        String,
        Vec<crate::ast::TypeAnn>,
        Option<crate::ast::TypeAnn>,
    );
    static CACHE: OnceLock<Vec<CachedSignature>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut out = Vec::new();
        for (path, module) in crate::project::stdlib::parsed_stdlib_modules() {
            #[cfg(feature = "cross-module-imports")]
            out.extend(
                crate::project::module_table::collect_module_exports(&path, &module)
                    .exported_fns
                    .into_iter()
                    .map(|f| (f.name, f.param_types, f.ret_type)),
            );
            #[cfg(not(feature = "cross-module-imports"))]
            {
                let _ = path;
                collect_std_fn_signatures(&module.items, &mut out);
            }
        }
        out
    })
}

#[cfg(not(feature = "cross-module-imports"))]
fn collect_std_fn_signatures(
    items: &[crate::ast::Node],
    out: &mut Vec<(
        String,
        Vec<crate::ast::TypeAnn>,
        Option<crate::ast::TypeAnn>,
    )>,
) {
    for item in items {
        match item {
            crate::ast::Node::FnDef(fd, _) => out.push((
                fd.name.clone(),
                fd.params.iter().map(|param| param.ty.clone()).collect(),
                fd.ret_type.clone(),
            )),
            crate::ast::Node::Block { stmts, .. } => collect_std_fn_signatures(stmts, out),
            _ => {}
        }
    }
}
