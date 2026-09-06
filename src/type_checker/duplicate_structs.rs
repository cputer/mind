// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use super::diag_from_span;
use crate::ast::Node;
use crate::diagnostics::Diagnostic;
use std::collections::BTreeSet;

/// Inline `module NAME { ... }` declarations parse as transparent blocks, so
/// every reachable struct belongs to this source module's single declaration
/// namespace. Keep this pass at the public checker entry point: recursive
/// checker calls for each block item must not re-report the same pair, and
/// separate project files are checked independently by their own entry call.
pub(super) fn check(items: &[Node], src: &str, file: Option<&str>, errs: &mut Vec<Diagnostic>) {
    let mut seen = BTreeSet::new();
    fn visit<'a>(
        items: &'a [Node],
        seen: &mut BTreeSet<&'a str>,
        src: &str,
        file: Option<&str>,
        errs: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            match item {
                Node::StructDef { name, span, .. } => {
                    if !seen.insert(name.as_str()) {
                        errs.push(diag_from_span(
                            src,
                            file,
                            format!(
                                "duplicate struct declaration `{name}` in the transparent module namespace; rename one declaration"
                            ),
                            *span,
                            "E2035",
                        ));
                    }
                }
                Node::Block { stmts, .. } => visit(stmts, seen, src, file, errs),
                _ => {}
            }
        }
    }
    visit(items, &mut seen, src, file, errs);
}
