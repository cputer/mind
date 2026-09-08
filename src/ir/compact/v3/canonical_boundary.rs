// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Checked boundary for canonical metadata that MIC@3 v0x03 cannot encode.

use crate::ir::canonical_verify::instruction_metadata_present;
use crate::ir::{IRModule, verify_canonical_metadata};

use super::{Mic3EncodeError, emit::emit_mic3_legacy};

/// Legacy v0x02/v0x03 emission; populated B1 metadata must use the checked boundary.
pub fn emit_mic3(module: &IRModule) -> Vec<u8> {
    if module
        .canonical_types
        .as_ref()
        .is_some_and(|b| !b.is_empty())
        || instruction_metadata_present(&module.instrs)
    {
        panic!("emit_mic3 cannot encode populated canonical metadata; use emit_mic3_checked");
    }
    emit_mic3_legacy(module)
}

/// Emit legacy bytes only when no canonical payload would be discarded.
pub fn emit_mic3_checked(module: &IRModule) -> Result<Vec<u8>, Mic3EncodeError> {
    if module.canonical_types.is_none() && instruction_metadata_present(&module.instrs) {
        return Err(Mic3EncodeError::InvalidCanonicalMetadata(
            "instruction metadata has no canonical module bundle".to_string(),
        ));
    }
    if module.canonical_types.is_some() {
        verify_canonical_metadata(module)
            .map_err(|error| Mic3EncodeError::InvalidCanonicalMetadata(error.to_string()))?;
        if module
            .canonical_types
            .as_ref()
            .is_some_and(|bundle| !bundle.is_empty())
            || instruction_metadata_present(&module.instrs)
        {
            return Err(Mic3EncodeError::UnsupportedCanonicalMetadata);
        }
    }
    Ok(emit_mic3_legacy(module))
}
