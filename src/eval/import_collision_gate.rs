// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Runnable-artifact preflight for owner collisions in imported functions.

use crate::ast::{Module, Node};
use crate::diagnostics::{Diagnostic, Span};

const HELP: &str = "the shipped backend cannot emit owner-colliding native symbols yet; rename the local or imported function";

/// Refuse a local function that collides with a captured imported function
/// only on runnable-artifact emission. Checking still preserves local
/// declaration precedence; this gate prevents a duplicate bare symbol from
/// reaching the native linker.
pub fn check_imported_local_fn_collisions(
    module: &Module,
    src: &str,
    file: Option<&str>,
) -> Vec<Diagnostic> {
    let owner = crate::qualified_enums::current_module_path();
    module
        .items
        .iter()
        .filter_map(|item| {
            let Node::FnDef(fd, span) = item else {
                return None;
            };
            crate::project::active_module_table::imported_fn_conflicts_with_local(
                owner.as_deref(),
                &fd.name,
            )
            .then(|| {
                Diagnostic::error(
                    "lower",
                    "lower::imported_local_fn_collision",
                    format!(
                        "local function `{}` conflicts with a captured imported function; owner-aware native linkage cannot emit this shadow yet — rename one",
                        fd.name
                    ),
                )
                .with_span(Span::from_offsets(src, span.start(), span.end(), file))
                .with_help(HELP)
            })
        })
        .collect()
}
