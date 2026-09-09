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
//! deliberately a standalone registry. The draft checked v0x04 MIC reader can
//! reconstruct it; resolver and lowering attachment remain separate. The builder
//! resolves aliases while ownership is explicit, then freezes a deterministic table.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use super::canonical_registry::{self, SchemaRegistry};
use crate::ir::ValueId;

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

/// Logical identity of a function declaration.  The owner is supplied by the
/// resolver and is independent of import aliases, source paths, traversal
/// order, and backend symbol spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionIdentity {
    owner: String,
    name: String,
    type_args: Vec<String>,
}

impl FunctionIdentity {
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

impl fmt::Display for FunctionIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.owner, self.name)?;
        if !self.type_args.is_empty() {
            write!(f, "<{}>", self.type_args.join(","))?;
        }
        Ok(())
    }
}

/// Canonical signature for a function declaration or external declaration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionSignature {
    params: Vec<SemanticType>,
    return_type: Option<SemanticType>,
}

impl FunctionSignature {
    pub fn new(params: Vec<SemanticType>, return_type: Option<SemanticType>) -> Self {
        Self {
            params,
            return_type,
        }
    }

    pub fn params(&self) -> &[SemanticType] {
        &self.params
    }

    pub fn return_type(&self) -> Option<&SemanticType> {
        self.return_type.as_ref()
    }
}

/// The declaration authority for a logical function identity.  The kind is
/// explicit so intrinsic handling never depends on a spelling prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FunctionKind {
    Local,
    External,
    Intrinsic,
}

/// One canonical declaration.  Signatures live here, once, rather than being
/// duplicated in every function body carrier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionDeclaration {
    identity: FunctionIdentity,
    kind: FunctionKind,
    signature: FunctionSignature,
}

impl FunctionDeclaration {
    pub fn new(
        identity: FunctionIdentity,
        kind: FunctionKind,
        signature: FunctionSignature,
    ) -> Self {
        Self {
            identity,
            kind,
            signature,
        }
    }

    pub fn identity(&self) -> &FunctionIdentity {
        &self.identity
    }

    pub fn kind(&self) -> FunctionKind {
        self.kind
    }

    pub fn signature(&self) -> &FunctionSignature {
        &self.signature
    }
}

/// Semantic metadata co-located with one function body.  Value IDs are local
/// to this function scope; two functions may therefore both contain `%0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSemanticTypes {
    identity: FunctionIdentity,
    values: BTreeMap<ValueId, SemanticType>,
}

impl FunctionSemanticTypes {
    pub fn new(identity: FunctionIdentity) -> Self {
        Self {
            identity,
            values: BTreeMap::new(),
        }
    }

    pub fn identity(&self) -> &FunctionIdentity {
        &self.identity
    }

    pub fn values(&self) -> &BTreeMap<ValueId, SemanticType> {
        &self.values
    }

    pub fn set_value_type(&mut self, value: ValueId, ty: SemanticType) -> Result<(), SchemaError> {
        if self.values.contains_key(&value) {
            return Err(SchemaError::DuplicateValueType { value });
        }
        self.values.insert(value, ty);
        Ok(())
    }
}

/// The single semantic authority carried by an IR module in B1. The draft
/// v0x04 codec transports it; source lowering and consumer propagation remain
/// incomplete. Callers must use its validation boundary before serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalModuleTypes {
    schema_registry: SchemaRegistry,
    module_values: BTreeMap<ValueId, SemanticType>,
    functions: BTreeMap<FunctionIdentity, FunctionDeclaration>,
}

impl CanonicalModuleTypes {
    pub fn new(schema_registry: SchemaRegistry) -> Self {
        Self {
            schema_registry,
            module_values: BTreeMap::new(),
            functions: BTreeMap::new(),
        }
    }

    /// Construct a complete decoded carrier and validate its cumulative
    /// descriptor budget once. Wire decoders already reject duplicate and
    /// unsorted rows while staging these maps; this checked seam prevents a
    /// staged invalid carrier from becoming publicly observable.
    pub(crate) fn from_parts_checked(
        schema_registry: SchemaRegistry,
        module_values: BTreeMap<ValueId, SemanticType>,
        functions: BTreeMap<FunctionIdentity, FunctionDeclaration>,
    ) -> Result<Self, SchemaError> {
        let bundle = Self {
            schema_registry,
            module_values,
            functions,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn schema_registry(&self) -> &SchemaRegistry {
        &self.schema_registry
    }

    pub fn module_values(&self) -> &BTreeMap<ValueId, SemanticType> {
        &self.module_values
    }

    pub fn functions(&self) -> &BTreeMap<FunctionIdentity, FunctionDeclaration> {
        &self.functions
    }

    /// Whether this carrier contains any semantic authority.  An empty
    /// optional bundle is equivalent to the legacy `None` state and does not
    /// require a newer wire version.
    pub fn is_empty(&self) -> bool {
        self.schema_registry.schemas().is_empty()
            && self.module_values.is_empty()
            && self.functions.is_empty()
    }

    pub fn set_module_value_type(
        &mut self,
        value: ValueId,
        ty: SemanticType,
    ) -> Result<(), SchemaError> {
        if self.module_values.contains_key(&value) {
            return Err(SchemaError::DuplicateValueType { value });
        }
        validate_descriptor_budget(
            &self.schema_registry,
            self.module_values.values().chain(std::iter::once(&ty)),
        )?;
        self.module_values.insert(value, ty);
        Ok(())
    }

    /// Validate a function-local semantic value against the same registry
    /// shape authority used for module values and frozen schemas.
    pub(crate) fn validate_value_type(&self, ty: &SemanticType) -> Result<u128, SchemaError> {
        canonical_registry::validate_semantic_descriptor(ty, &self.schema_registry)
    }

    pub fn add_declaration(&mut self, declaration: FunctionDeclaration) -> Result<(), SchemaError> {
        let identity = declaration.identity.clone();
        validate_function_identity(&identity)?;
        validate_function_declaration(&declaration)?;
        if self.functions.contains_key(&identity) {
            return Err(SchemaError::DuplicateFunction { identity });
        }
        let existing_functions = self.functions.values().flat_map(|declaration| {
            declaration
                .signature
                .params()
                .iter()
                .chain(declaration.signature.return_type())
        });
        let candidate_types = declaration
            .signature
            .params()
            .iter()
            .chain(declaration.signature.return_type());
        if validate_descriptor_budget(
            &self.schema_registry,
            self.module_values
                .values()
                .chain(existing_functions)
                .chain(candidate_types),
        )
        .is_err()
        {
            return Err(SchemaError::InvalidFunctionType { identity });
        }
        self.functions.insert(identity, declaration);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SchemaError> {
        let function_types = self.functions.values().flat_map(|declaration| {
            declaration
                .signature
                .params()
                .iter()
                .chain(declaration.signature.return_type())
        });
        validate_descriptor_budget(
            &self.schema_registry,
            self.module_values.values().chain(function_types),
        )?;
        for (identity, declaration) in &self.functions {
            validate_function_identity(identity)?;
            if identity != declaration.identity() {
                return Err(SchemaError::FunctionIdentityMismatch {
                    identity: identity.clone(),
                });
            }
            validate_function_declaration(declaration)?;
        }
        Ok(())
    }
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
    /// Maximum cumulative logical element descriptors. Fixed arrays multiply
    /// their element shape; dynamic arrays count one descriptor plus one
    /// element shape, without assuming a runtime length. Every nested shape
    /// must also fit independently, including inside an empty fixed array.
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
    #[error("empty function identity component: {kind}")]
    EmptyFunctionIdentity { kind: &'static str },
    #[error("path-like function identity component: {value}")]
    PathLikeFunctionIdentity { value: String },
    #[error("unsupported generic function identity: {identity}")]
    UnsupportedFunctionTypeArguments { identity: FunctionIdentity },
    #[error("duplicate function identity: {identity}")]
    DuplicateFunction { identity: FunctionIdentity },
    #[error("intrinsic declaration uses a non-reserved identity: {identity}")]
    InvalidIntrinsicIdentity { identity: FunctionIdentity },
    #[error("unknown canonical intrinsic contract: {identity}")]
    UnknownIntrinsic { identity: FunctionIdentity },
    #[error("canonical intrinsic signature does not match its registry contract: {identity}")]
    IntrinsicSignatureMismatch { identity: FunctionIdentity },
    #[error("canonical intrinsic is not supported by backend profile {profile}: {identity}")]
    IntrinsicProfileMismatch {
        identity: FunctionIdentity,
        profile: &'static str,
    },
    #[error("function identity does not match its registry key: {identity}")]
    FunctionIdentityMismatch { identity: FunctionIdentity },
    #[error("invalid semantic type in function {identity}")]
    InvalidFunctionType { identity: FunctionIdentity },
    #[error("semantic type references an unknown schema: {identity}")]
    UnknownSemanticSchema { identity: SchemaIdentity },
    #[error("duplicate canonical type for value {value}")]
    DuplicateValueType { value: ValueId },
}

fn validate_function_identity(identity: &FunctionIdentity) -> Result<(), SchemaError> {
    if identity.owner.is_empty() {
        return Err(SchemaError::EmptyFunctionIdentity { kind: "owner" });
    }
    if identity.name.is_empty() {
        return Err(SchemaError::EmptyFunctionIdentity { kind: "name" });
    }
    for component in [&identity.owner, &identity.name] {
        if !identity_component_is_valid(component) {
            return Err(SchemaError::PathLikeFunctionIdentity {
                value: component.to_string(),
            });
        }
    }
    if !identity.type_args.is_empty() {
        return Err(SchemaError::UnsupportedFunctionTypeArguments {
            identity: identity.clone(),
        });
    }
    Ok(())
}

/// Shared wire-safe identity component predicate. Components remain exact UTF-8
/// strings: this rejects path-like spellings without applying normalization.
pub(crate) fn identity_component_is_valid(component: &str) -> bool {
    !component.is_empty()
        && !component.contains('/')
        && !component.contains('\\')
        && !component.contains("..")
}

fn validate_function_declaration(declaration: &FunctionDeclaration) -> Result<(), SchemaError> {
    let reserved_owner = "__mind_intrinsic";
    match declaration.kind {
        FunctionKind::Intrinsic => {
            if declaration.identity.owner != reserved_owner {
                return Err(SchemaError::InvalidIntrinsicIdentity {
                    identity: declaration.identity.clone(),
                });
            }
            // Canonical declarations are backend-neutral on the wire, but
            // their only admitted first-cut capability is the exact frozen
            // native profile. Other backend selections must go through the
            // explicit profile seam below and refuse closed.
            validate_intrinsic_profile(
                &declaration.identity,
                crate::intrinsics::IntrinsicProfile::FrozenNative,
            )?;
            let Some(contract) = crate::intrinsics::intrinsic_contract(&declaration.identity.name)
            else {
                return Err(SchemaError::UnknownIntrinsic {
                    identity: declaration.identity.clone(),
                });
            };
            let parameters_match = contract.parameters.len()
                == declaration.signature.params().len()
                && contract
                    .parameters
                    .iter()
                    .zip(declaration.signature.params())
                    .all(|(expected, actual)| {
                        matches!(
                            (expected, actual),
                            (
                                crate::intrinsics::IntrinsicValueType::I64,
                                SemanticType::Scalar(ScalarType::I64)
                            )
                        )
                    });
            let result_matches = match contract.result {
                crate::intrinsics::IntrinsicResult::I64 => declaration
                    .signature
                    .return_type()
                    .is_some_and(|ty| matches!(ty, SemanticType::Scalar(ScalarType::I64))),
                crate::intrinsics::IntrinsicResult::DiscardOnly => {
                    // The v04 wire ABI remains `(i64...) -> i64` for stores.
                    // DiscardOnly is a native result-use rule enforced by
                    // later admission, not a unit return type.
                    declaration
                        .signature
                        .return_type()
                        .is_some_and(|ty| matches!(ty, SemanticType::Scalar(ScalarType::I64)))
                }
            };
            if !parameters_match || !result_matches {
                return Err(SchemaError::IntrinsicSignatureMismatch {
                    identity: declaration.identity.clone(),
                });
            }
            Ok(())
        }
        FunctionKind::Local | FunctionKind::External
            if declaration.identity.owner == reserved_owner =>
        {
            Err(SchemaError::InvalidIntrinsicIdentity {
                identity: declaration.identity.clone(),
            })
        }
        _ => Ok(()),
    }
}

pub(crate) use super::canonical_intrinsic_validation::validate_intrinsic_profile;

fn validate_descriptor_budget<'a, I>(schemas: &SchemaRegistry, types: I) -> Result<(), SchemaError>
where
    I: IntoIterator<Item = &'a SemanticType>,
{
    let mut total = 0_u128;
    for ty in types {
        let cost = canonical_registry::validate_semantic_descriptor(ty, schemas)?;
        total = total
            .checked_add(cost)
            .ok_or(SchemaError::FixedElementOverflow)?;
        if total > schemas.limits().max_fixed_elements {
            return Err(SchemaError::FixedElementLimitExceeded {
                limit: schemas.limits().max_fixed_elements,
            });
        }
    }
    Ok(())
}
