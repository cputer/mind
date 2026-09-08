// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Canonical semantic record and aggregate descriptors.
//!
//! This foundation is intentionally standalone. Later slices will connect the
//! resolver, IR value metadata, MIC codec, and runnable backends.

#[path = "canonical_registry.rs"]
mod canonical_registry;
#[path = "canonical_types.rs"]
mod canonical_types;

#[cfg(test)]
#[path = "canonical_tests.rs"]
mod tests;

pub use canonical_registry::{SchemaRegistry, SchemaRegistryBuilder};
pub use canonical_types::{
    AliasIdentity, CanonicalField, CanonicalSchema, FieldDraft, RegistryLimits, ScalarType,
    SchemaDraft, SchemaError, SchemaId, SchemaIdentity, SemanticType, TypeExpr,
};
