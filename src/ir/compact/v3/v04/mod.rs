// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Draft MIC@3 v0x04 canonical-metadata codec.

use crate::ir::{IRModule, Instr, ValueId};

mod decode;
mod encode;

pub(super) use decode::parse_v04_prefix;
pub(super) use encode::emit_v04;

pub(super) const REQUIRED_STD_SURFACE: u64 = 1;
pub(super) const KNOWN_SURFACE_BITS: u64 = REQUIRED_STD_SURFACE;
pub(super) const MAX_INSTR_DEPTH: usize = 256;

pub(super) const BUDGET_MAX: usize = 128 * 1024 * 1024;
pub(super) const BUDGET_BASE: usize = 1024 * 1024;
pub(super) const BUDGET_FACTOR: usize = 32;
pub(super) const STRING_SLOT_CHARGE: usize = 32;
pub(super) const STRING_REFERENCE_MULTIPLIER: usize = 8;
pub(super) const SCHEMA_CHARGE: usize = 256;
pub(super) const FIELD_CHARGE: usize = 192;
pub(super) const FUNCTION_CHARGE: usize = 320;
pub(super) const PARAMETER_CHARGE: usize = 128;
pub(super) const DESCRIPTOR_CHARGE: usize = 160;
pub(super) const VALUE_ROW_CHARGE: usize = 160;
pub(super) const INSTRUCTION_CHARGE: usize = 320;
pub(super) const NAMED_VALUE_CHARGE: usize = 96;
pub(super) const VALUE_ID_CHARGE: usize = 32;

pub(super) fn decoded_budget_limit(input: usize) -> usize {
    input
        .checked_mul(BUDGET_FACTOR)
        .and_then(|value| value.checked_add(BUDGET_BASE))
        .unwrap_or(BUDGET_MAX)
        .min(BUDGET_MAX)
}

pub(super) fn validate_core_value_ids(module: &IRModule) -> Result<(), String> {
    if module.next_id == usize::MAX {
        return Err("v0x04 next_id usize::MAX is not safely incrementable".to_string());
    }
    let mut streams = vec![(&module.instrs[..], Some(module.next_id), 0_usize)];
    while let Some((stream, next_id, depth)) = streams.pop() {
        if depth >= MAX_INSTR_DEPTH && !stream.is_empty() {
            return Err("v0x04 instruction nesting exceeds depth 256".to_string());
        }
        let mut bound = 0;
        for instruction in stream {
            match instruction {
                Instr::ConstI64(dst, _) | Instr::ConstF64(dst, _) => admit_id(*dst, &mut bound)?,
                Instr::BinOp { dst, lhs, rhs, .. } => {
                    for value in [dst, lhs, rhs] {
                        admit_id(*value, &mut bound)?;
                    }
                }
                Instr::Output(value) => admit_id(*value, &mut bound)?,
                Instr::FnDef {
                    params,
                    ret_id,
                    body,
                    semantic_types,
                    ..
                } => {
                    let mut function_bound = 0;
                    for (_, value) in params {
                        admit_id(*value, &mut function_bound)?;
                    }
                    if let Some(value) = ret_id {
                        admit_id(*value, &mut function_bound)?;
                    }
                    if let Some(types) = semantic_types {
                        for value in types.values().keys() {
                            admit_id(*value, &mut function_bound)?;
                        }
                    }
                    streams.push((body, None, depth + 1));
                }
                Instr::Call { dst, args, .. } => {
                    admit_id(*dst, &mut bound)?;
                    for value in args {
                        admit_id(*value, &mut bound)?;
                    }
                }
                Instr::Return { value: Some(value) } => admit_id(*value, &mut bound)?,
                Instr::Return { value: None } => {}
                Instr::Param { dst, .. } => admit_id(*dst, &mut bound)?,
                _ => {}
            }
        }
        if depth == 0 {
            if let Some(bundle) = module.canonical_types.as_ref() {
                for value in bundle.module_values().keys() {
                    admit_id(*value, &mut bound)?;
                }
            }
        }
        if next_id.is_some_and(|next| next < bound) {
            return Err(format!(
                "v0x04 next_id {} is below the enclosing ValueId bound {bound}",
                next_id.unwrap_or_default()
            ));
        }
    }
    Ok(())
}

fn admit_id(value: ValueId, bound: &mut usize) -> Result<(), String> {
    let exclusive = value
        .0
        .checked_add(1)
        .ok_or_else(|| "v0x04 ValueId usize::MAX cannot form an exclusive bound".to_string())?;
    *bound = (*bound).max(exclusive);
    Ok(())
}

pub(super) const TYPE_SCALAR: u8 = 0;
pub(super) const TYPE_RECORD_REF: u8 = 1;
pub(super) const TYPE_FIXED_ARRAY: u8 = 2;
pub(super) const TYPE_DYNAMIC_ARRAY: u8 = 3;

pub(super) const SCALAR_I32: u8 = 0;
pub(super) const SCALAR_I64: u8 = 1;
pub(super) const SCALAR_U32: u8 = 2;
pub(super) const SCALAR_F32: u8 = 3;
pub(super) const SCALAR_F64: u8 = 4;
pub(super) const SCALAR_BOOL: u8 = 5;
pub(super) const SCALAR_BF16: u8 = 6;
pub(super) const SCALAR_F16: u8 = 7;
pub(super) const SCALAR_Q16: u8 = 8;

pub(super) const FUNCTION_LOCAL: u8 = 0;
pub(super) const FUNCTION_EXTERNAL: u8 = 1;
pub(super) const FUNCTION_INTRINSIC: u8 = 2;
