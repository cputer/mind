// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Small type adapters shared by the owner-aware imported-call checker.

use super::{ValueType, describe_value_type, valuetype_from_ann};
use crate::ast::TypeAnn;

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
