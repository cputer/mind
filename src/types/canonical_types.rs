// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Canonical semantic record and aggregate descriptors.
//!
//! This is the foundation for the canonical aggregate representation.  It is
//! deliberately a transient, standalone registry: no lowering or MIC reader
//! populates it yet.  The builder resolves aliases while defining ownership is
//! still explicit, then freezes a deterministic schema table for later slices.

use std::fmt;

use thiserror::Error;

/// A logical declaration identity.  `owner` is a resolver-provided logical
/// module/package owner, never a filesystem path or a caller's import alias.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaIdentity {
    owner: String,
    name: String,
    type_args: Vec<String>,
}

impl SchemaIdentity {
    /// Construct an identity without normalizing it.  Validation happens when
    /// the identity is inserted into a registry so callers can build drafts.
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
            type_args: Vec::new(),
        }
    }

    pub fn with_type_args<I, S>(mut self, type_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.type_args = type_args.into_iter().map(Into::into).collect();
        self
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn type_args(&self) -> &[String] {
        &self.type_args
    }
}

impl fmt::Display for SchemaIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.owner, self.name)?;
        if !self.type_args.is_empty() {
            write!(f, "<{}>", self.type_args.join(","))?;
        }
        Ok(())
    }
}

/// The defining identity of an alias.  Alias names are resolved before a
/// schema is assigned an ID, so an import spelling cannot affect identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AliasIdentity {
    owner: String,
    name: String,
}

impl AliasIdentity {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for AliasIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.owner, self.name)
    }
}

/// Scalar semantic types known to the current language surface.  Physical
/// backend widths are intentionally not encoded by this registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScalarType {
    I32,
    I64,
    U32,
    F32,
    F64,
    Bool,
    BF16,
    F16,
    Q16,
}

/// Input type expression.  `Alias` is resolver-owned and disappears from the
/// frozen registry.  `InlineRecord` is retained as an explicit input marker so
/// an accidental anonymous/inline recursive representation fails closed;
/// identity-bearing records must use `RecordRef`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeExpr {
    Scalar(ScalarType),
    RecordRef(SchemaIdentity),
    FixedArray { element: Box<TypeExpr>, extent: u64 },
    DynamicArray { element: Box<TypeExpr> },
    Alias(AliasIdentity),
    InlineRecord { fields: Vec<FieldDraft> },
}

/// A resolved semantic descriptor.  Schema references are opaque identities,
/// which permits recursive record graphs without recursively embedding a
/// schema's fields.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticType {
    Scalar(ScalarType),
    RecordRef(SchemaIdentity),
    FixedArray {
        element: Box<SemanticType>,
        extent: u64,
    },
    DynamicArray {
        element: Box<SemanticType>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldDraft {
    name: String,
    ty: TypeExpr,
}

impl FieldDraft {
    pub fn new(name: impl Into<String>, ty: TypeExpr) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> &TypeExpr {
        &self.ty
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaDraft {
    identity: SchemaIdentity,
    fields: Vec<FieldDraft>,
}

impl SchemaDraft {
    pub fn new(identity: SchemaIdentity, fields: Vec<FieldDraft>) -> Self {
        Self { identity, fields }
    }

    pub fn identity(&self) -> &SchemaIdentity {
        &self.identity
    }

    pub fn fields(&self) -> &[FieldDraft] {
        &self.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalField {
    name: String,
    ty: SemanticType,
}

impl CanonicalField {
    pub(crate) fn new(name: String, ty: SemanticType) -> Self {
        Self { name, ty }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> &SemanticType {
        &self.ty
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalSchema {
    id: SchemaId,
    identity: SchemaIdentity,
    fields: Vec<CanonicalField>,
}

impl CanonicalSchema {
    pub(crate) fn new(id: SchemaId, identity: SchemaIdentity, fields: Vec<CanonicalField>) -> Self {
        Self {
            id,
            identity,
            fields,
        }
    }

    pub fn id(&self) -> SchemaId {
        self.id
    }

    pub fn identity(&self) -> &SchemaIdentity {
        &self.identity
    }

    pub fn fields(&self) -> &[CanonicalField] {
        &self.fields
    }
}

/// Stable index into a finished registry.  IDs are assigned after sorting
/// identities, never at insertion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaId(u32);

impl SchemaId {
    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Deterministic validation limits.  These bound descriptor validation and
/// metadata accounting; they do not enable a backend or impose runtime array
/// allocation behavior. The defaults are 100,000 schemas, 1,000,000 total
/// fields, 100,000 aliases, depth 64, extent `u32::MAX`, 2^40 fixed semantic
/// elements, and 2^30 logical identity bytes. Element and byte counters
/// describe semantic descriptors only; they are not physical ABI sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryLimits {
    /// Maximum number of schema declarations admitted by the builder.
    pub max_schemas: u64,
    /// Maximum total number of fields across admitted schema declarations.
    pub max_fields: u64,
    /// Maximum number of alias declarations admitted by the builder.
    pub max_aliases: u64,
    /// Maximum recursive type-expression depth.
    pub max_type_depth: u32,
    /// Maximum extent of one fixed-array descriptor.
    pub max_extent: u64,
    /// Maximum cumulative logical fixed-array element descriptors.
    pub max_fixed_elements: u128,
    /// Maximum cumulative bytes in logical identity/type descriptor strings.
    pub max_identity_bytes: u64,
}

impl Default for RegistryLimits {
    fn default() -> Self {
        Self {
            max_schemas: 100_000,
            max_fields: 1_000_000,
            max_aliases: 100_000,
            max_type_depth: 64,
            max_extent: u32::MAX as u64,
            max_fixed_elements: 1_u128 << 40,
            max_identity_bytes: 1_u64 << 30,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SchemaError {
    #[error("empty {kind} in logical identity")]
    EmptyIdentity { kind: &'static str },
    #[error("path-like logical identity component: {value}")]
    PathLikeIdentity { value: String },
    #[error("duplicate schema identity: {identity}")]
    DuplicateSchema { identity: SchemaIdentity },
    #[error("conflicting schema definition for identity: {identity}")]
    ConflictingSchema { identity: SchemaIdentity },
    #[error("duplicate alias identity: {identity}")]
    DuplicateAlias { identity: AliasIdentity },
    #[error("unknown alias identity: {identity}")]
    UnknownAlias { identity: AliasIdentity },
    #[error("alias cycle through {identity}")]
    AliasCycle { identity: AliasIdentity },
    #[error("unknown record schema: {identity}")]
    UnknownSchema { identity: SchemaIdentity },
    #[error("duplicate field {field} in schema {identity}")]
    DuplicateField {
        identity: SchemaIdentity,
        field: String,
    },
    #[error("empty field name in schema {identity}")]
    EmptyField { identity: SchemaIdentity },
    #[error("inline record requires an identity-bearing RecordRef")]
    InlineRecordUnsupported,
    #[error("type descriptor depth exceeds {limit}")]
    TypeDepthExceeded { limit: u32 },
    #[error("schema count exceeds {limit}")]
    SchemaLimitExceeded { limit: u64 },
    #[error("field count exceeds {limit}")]
    FieldLimitExceeded { limit: u64 },
    #[error("fixed-array extent {extent} exceeds {limit}")]
    ExtentLimitExceeded { extent: u64, limit: u64 },
    #[error("fixed-array element-count arithmetic overflow")]
    FixedElementOverflow,
    #[error("fixed-array element count exceeds {limit}")]
    FixedElementLimitExceeded { limit: u128 },
    #[error("unsupported generic identity arguments for {identity}")]
    UnsupportedTypeArguments { identity: SchemaIdentity },
    #[error("alias count exceeds {limit}")]
    AliasLimitExceeded { limit: u64 },
    #[error("logical identity bytes exceed {limit}")]
    IdentityBytesExceeded { limit: u64 },
}
