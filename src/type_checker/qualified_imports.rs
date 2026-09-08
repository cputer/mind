//! Feature-neutral span-scoped qualified import predicates for name resolution.

#[cfg(feature = "cross-module-imports")]
pub(super) fn current_symbol(name: &str) -> bool {
    crate::project::active_module_table::current_symbol_exported(name)
}

#[cfg(not(feature = "cross-module-imports"))]
pub(super) fn current_symbol(_name: &str) -> bool {
    false
}

#[cfg(feature = "cross-module-imports")]
pub(super) fn value(span: crate::ast::Span, name: &str) -> bool {
    crate::project::active_module_table::qualified_import_value(span, name)
}

#[cfg(not(feature = "cross-module-imports"))]
pub(super) fn value(_span: crate::ast::Span, _name: &str) -> bool {
    false
}

#[cfg(feature = "cross-module-imports")]
pub(super) fn call(span: crate::ast::Span, name: &str) -> bool {
    crate::project::active_module_table::qualified_import_fn(span, name).is_some()
        || crate::project::active_module_table::qualified_import_value(span, name)
}

#[cfg(not(feature = "cross-module-imports"))]
pub(super) fn call(_span: crate::ast::Span, _name: &str) -> bool {
    false
}
