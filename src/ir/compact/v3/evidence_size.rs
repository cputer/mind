// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Checked size accounting for MIC@3 evidence MAP envelopes.

use super::EvidenceEmitError;
use crate::ir::compact::v2::zigzag_encode;

/// Ephemeral value type used during MAP construction and size accounting.
pub(super) enum MapEntryValue<'a> {
    Str(&'a str),
    Int(i64),
    Bytes(&'a [u8]),
}

pub(super) fn checked_len_add(left: usize, right: usize) -> Result<usize, EvidenceEmitError> {
    left.checked_add(right)
        .ok_or(EvidenceEmitError::ArtifactSizeOverflow)
}

fn uleb128_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

fn map_value_encoded_len(val: &MapEntryValue<'_>) -> Result<usize, EvidenceEmitError> {
    let payload_len = match val {
        MapEntryValue::Str(value) => value.len(),
        MapEntryValue::Bytes(value) => value.len(),
        MapEntryValue::Int(value) => uleb128_len(zigzag_encode(*value)),
    };
    let encoded_payload_len = match val {
        MapEntryValue::Int(_) => payload_len,
        MapEntryValue::Str(_) | MapEntryValue::Bytes(_) => {
            checked_len_add(uleb128_len(payload_len as u64), payload_len)?
        }
    };
    checked_len_add(1, encoded_payload_len)
}

pub(super) fn map_encoded_len(
    entries: &[(&str, MapEntryValue<'_>)],
) -> Result<usize, EvidenceEmitError> {
    let mut len = 1usize; // MAP_SENTINEL
    len = checked_len_add(len, uleb128_len(entries.len() as u64))?;
    for (key, value) in entries {
        len = checked_len_add(len, uleb128_len(key.len() as u64))?;
        len = checked_len_add(len, key.len())?;
        len = checked_len_add(len, map_value_encoded_len(value)?)?;
    }
    Ok(len)
}
