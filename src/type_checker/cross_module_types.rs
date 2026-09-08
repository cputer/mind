// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Small type adapters shared by the owner-aware imported-call checker.

use super::{ValueType, describe_value_type, valuetype_from_ann};
use crate::ast::TypeAnn;

/// Seed names visible to the legacy explicit multi-file check scope.
pub(super) fn cm_inject_visible_symbols(tenv: &mut super::TypeEnv) {
    if !crate::project::active_module_table::legacy_visible_scope() {
        return;
    }
    let owner = crate::qualified_enums::current_module_path();
    for sym in crate::project::active_module_table::visible_symbols(owner.as_deref()) {
        tenv.entry(sym).or_insert(ValueType::ScalarI64);
    }
    for function in crate::project::active_module_table::visible_fns(owner.as_deref()) {
        tenv.entry(function.name).or_insert(ValueType::ScalarI64);
    }
}

/// Map a declared imported-call type to the loose call-site `ValueType`.
/// Aggregates and named heap records retain the established i64 fallback.
pub(super) fn cm_typeann_to_valuetype(ann: &TypeAnn) -> ValueType {
    valuetype_from_ann(ann).unwrap_or(ValueType::ScalarI64)
}

/// Render a declared parameter while preserving a named heap-record identity.
pub(super) fn describe_param_type(ann: &TypeAnn) -> String {
    match ann {
        TypeAnn::Named(name) => format!("{name} (heap-record i64 addr)"),
        _ => describe_value_type(&cm_typeann_to_valuetype(ann)),
    }
}

/// Accept exact call-site classes plus the existing i32/i64 literal widening.
pub(super) fn cm_arg_compatible(expected: &ValueType, actual: &ValueType) -> bool {
    if expected == actual {
        return true;
    }
    matches!(
        (expected, actual),
        (ValueType::ScalarI64, ValueType::ScalarI32) | (ValueType::ScalarI32, ValueType::ScalarI64)
    )
}
