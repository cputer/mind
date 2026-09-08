// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use crate::ir::IRModule;

use super::evidence::{MAP_SENTINEL, ParseMapError, ParsedEntry, parse_map_epilogue};

/// One decoded MIC@3 body prefix and the exact cursor position after it.
#[derive(Debug)]
pub struct ParsedMic3Prefix {
    pub module: IRModule,
    pub consumed: usize,
}

/// Parse a plain MIC@3 body and reject every trailing byte.
pub fn parse_mic3_body(data: &[u8]) -> Result<IRModule, super::Mic3Error> {
    let parsed = super::parse_mic3_prefix(data)?;
    if parsed.consumed != data.len() {
        return Err(super::Mic3Error {
            message: format!(
                "trailing data after mic@3 body at byte {} ({} bytes remain)",
                parsed.consumed,
                data.len() - parsed.consumed
            ),
        });
    }
    Ok(parsed.module)
}

/// Legacy suffix-tolerant body parser; new callers should choose a strict API.
pub fn parse_mic3(data: &[u8]) -> Result<IRModule, super::Mic3Error> {
    super::parse_mic3_prefix(data).map(|parsed| parsed.module)
}

/// Why a strict mic@3 artifact envelope could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mic3EnvelopeError {
    /// The MIC@3 body prefix is malformed.
    Body(super::Mic3Error),
    /// Bytes follow the body but do not start a MAP epilogue.
    TrailingData { offset: usize },
    /// A MAP sentinel follows the body, but the epilogue is malformed.
    MalformedMap,
}

impl std::fmt::Display for Mic3EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(error) => write!(f, "mic@3 body is malformed: {error}"),
            Self::TrailingData { offset } => {
                write!(f, "non-MAP data follows the mic@3 body at byte {offset}")
            }
            Self::MalformedMap => write!(f, "MAP epilogue is malformed"),
        }
    }
}

impl std::error::Error for Mic3EnvelopeError {}

pub(super) struct ArtifactParts {
    pub(super) module: IRModule,
    pub(super) body_end: usize,
    pub(super) entries: Option<Vec<ParsedEntry>>,
}

pub(super) enum ArtifactPartsError {
    Body(super::Mic3Error),
    Trailing { offset: usize },
    Map(ParseMapError),
}

/// Decode exactly one MIC@3 body with an optional MAP epilogue.
///
/// Unlike [`super::parse_mic3`], this consumes the whole artifact and rejects
/// both arbitrary suffixes and malformed MAP bytes. The body/MAP boundary is
/// the parser cursor position, never a re-emitted body length.
pub fn parse_mic3_envelope(bytes: &[u8]) -> Result<IRModule, Mic3EnvelopeError> {
    parse_artifact_parts(bytes)
        .map(|parts| parts.module)
        .map_err(|error| match error {
            ArtifactPartsError::Body(error) => Mic3EnvelopeError::Body(error),
            ArtifactPartsError::Trailing { offset } => Mic3EnvelopeError::TrailingData { offset },
            ArtifactPartsError::Map(_) => Mic3EnvelopeError::MalformedMap,
        })
}

pub(super) fn parse_artifact_parts(bytes: &[u8]) -> Result<ArtifactParts, ArtifactPartsError> {
    let parsed = super::parse_mic3_prefix(bytes).map_err(ArtifactPartsError::Body)?;
    let rest = &bytes[parsed.consumed..];
    let entries = if rest.is_empty() {
        None
    } else if rest[0] != MAP_SENTINEL {
        return Err(ArtifactPartsError::Trailing {
            offset: parsed.consumed,
        });
    } else {
        Some(parse_map_epilogue(rest).map_err(ArtifactPartsError::Map)?)
    };
    Ok(ArtifactParts {
        module: parsed.module,
        body_end: parsed.consumed,
        entries,
    })
}
