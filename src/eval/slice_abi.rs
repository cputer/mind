// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Collection sentinels for the current Option-C array/slice ABI.

use crate::ast::TypeAnn;

/// `array<T>` is the public std.vec heap record. Slice parameters carry that
/// same handle, while distinct sentinels keep borrowed capabilities visible to
/// lowering so a slice cannot inherit ownership operations such as `push`.
pub(crate) const ARRAY_VEC_SENTINEL: &str = "vec";
pub(crate) const SLICE_VEC_SENTINEL: &str = "slice";
pub(crate) const MUT_SLICE_VEC_SENTINEL: &str = "slice_mut";

pub(crate) fn vec_param_sentinel(ty: &TypeAnn) -> Option<&'static str> {
    match ty {
        TypeAnn::Generic { name, .. } if name == "array" => Some(ARRAY_VEC_SENTINEL),
        TypeAnn::Slice { mutable: true, .. } => Some(MUT_SLICE_VEC_SENTINEL),
        TypeAnn::Slice { mutable: false, .. } => Some(SLICE_VEC_SENTINEL),
        _ => None,
    }
}

pub(crate) fn is_vec_handle_sentinel(name: &str) -> bool {
    matches!(
        name,
        ARRAY_VEC_SENTINEL | SLICE_VEC_SENTINEL | MUT_SLICE_VEC_SENTINEL
    )
}

pub(crate) fn is_mutable_vec_handle_sentinel(name: &str) -> bool {
    matches!(name, ARRAY_VEC_SENTINEL | MUT_SLICE_VEC_SENTINEL)
}
