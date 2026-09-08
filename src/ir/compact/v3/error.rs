// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

/// Structured failure from the checked MIC@3 body encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mic3EncodeError {
    /// Co-located instruction metadata has no module-level authority, or the
    /// carrier fails the B1 semantic consistency checks.
    InvalidCanonicalMetadata(String),
    /// B1 metadata is valid in memory, but MIC@3 v0x01-v0x03 cannot represent
    /// it. The v0x04 codec is a later slice.
    UnsupportedCanonicalMetadata,
}

impl std::fmt::Display for Mic3EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCanonicalMetadata(message) => {
                write!(f, "canonical metadata is invalid: {message}")
            }
            Self::UnsupportedCanonicalMetadata => {
                f.write_str("canonical semantic metadata requires unsupported MIC@3 v0x04")
            }
        }
    }
}

impl std::error::Error for Mic3EncodeError {}

/// Structured failure from MIC@3 body-plus-evidence emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceEmitError {
    Body(Mic3EncodeError),
    InvalidApplicationEntries,
    SchemeUnavailable(&'static str),
}

impl std::fmt::Display for EvidenceEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(error) => write!(f, "mic@3 body emission failed: {error}"),
            Self::InvalidApplicationEntries => {
                f.write_str("invalid application-namespace evidence entry")
            }
            Self::SchemeUnavailable(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EvidenceEmitError {}

impl From<Mic3EncodeError> for EvidenceEmitError {
    fn from(error: Mic3EncodeError) -> Self {
        Self::Body(error)
    }
}

/// Error produced by the MIC@3 body decoders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mic3Error {
    pub message: String,
}

impl std::fmt::Display for Mic3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mic3: {}", self.message)
    }
}

impl std::error::Error for Mic3Error {}

impl From<std::io::Error> for Mic3Error {
    fn from(e: std::io::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}
