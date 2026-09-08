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

use std::collections::{BTreeMap, BTreeSet};
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
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Deterministic validation limits.  These bound descriptor validation and
/// metadata accounting; they do not enable a backend or impose runtime array
/// allocation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryLimits {
    pub max_schemas: u64,
    pub max_fields: u64,
    pub max_type_depth: u32,
    pub max_extent: u64,
    pub max_static_bytes: u128,
}

impl Default for RegistryLimits {
    fn default() -> Self {
        Self {
            max_schemas: 100_000,
            max_fields: 1_000_000,
            max_type_depth: 64,
            max_extent: u32::MAX as u64,
            max_static_bytes: 1_u128 << 40,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SchemaError {
    #[error("empty {kind} in logical identity")]
    EmptyIdentity { kind: &'static str },
    #[error("path-like owner is not a logical identity: {owner}")]
    PathLikeOwner { owner: String },
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
    #[error("fixed-array size arithmetic overflow")]
    StaticSizeOverflow,
    #[error("static descriptor bytes exceed {limit}")]
    StaticBytesExceeded { limit: u128 },
}

/// Frozen canonical registry.  It is intentionally not attached to IRModule
/// until a later propagation/wire slice establishes one semantic authority for
/// every SSA value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaRegistry {
    limits: RegistryLimits,
    schemas: Vec<CanonicalSchema>,
    ids: BTreeMap<SchemaIdentity, SchemaId>,
}

impl SchemaRegistry {
    pub fn schemas(&self) -> &[CanonicalSchema] {
        &self.schemas
    }

    pub fn limits(&self) -> RegistryLimits {
        self.limits
    }

    pub fn schema_id(&self, identity: &SchemaIdentity) -> Option<SchemaId> {
        self.ids.get(identity).copied()
    }

    pub fn schema(&self, identity: &SchemaIdentity) -> Option<&CanonicalSchema> {
        self.schema_id(identity)
            .and_then(|id| self.schemas.get(id.index()))
    }
}

#[derive(Debug, Clone)]
pub struct SchemaRegistryBuilder {
    limits: RegistryLimits,
    aliases: BTreeMap<AliasIdentity, TypeExpr>,
    schemas: BTreeMap<SchemaIdentity, SchemaDraft>,
}

impl Default for SchemaRegistryBuilder {
    fn default() -> Self {
        Self::new(RegistryLimits::default())
    }
}

impl SchemaRegistryBuilder {
    pub fn new(limits: RegistryLimits) -> Self {
        Self {
            limits,
            aliases: BTreeMap::new(),
            schemas: BTreeMap::new(),
        }
    }

    pub fn add_alias(
        &mut self,
        identity: AliasIdentity,
        target: TypeExpr,
    ) -> Result<(), SchemaError> {
        validate_alias_identity(&identity)?;
        if self.aliases.insert(identity.clone(), target).is_some() {
            return Err(SchemaError::DuplicateAlias { identity });
        }
        Ok(())
    }

    pub fn add_schema(&mut self, draft: SchemaDraft) -> Result<(), SchemaError> {
        validate_schema_identity(&draft.identity)?;
        if let Some(existing) = self.schemas.get(&draft.identity) {
            return Err(if existing == &draft {
                SchemaError::DuplicateSchema {
                    identity: draft.identity,
                }
            } else {
                SchemaError::ConflictingSchema {
                    identity: draft.identity,
                }
            });
        }
        self.schemas.insert(draft.identity.clone(), draft);
        Ok(())
    }

    pub fn finish(self) -> Result<SchemaRegistry, SchemaError> {
        let schema_count =
            u64::try_from(self.schemas.len()).map_err(|_| SchemaError::SchemaLimitExceeded {
                limit: self.limits.max_schemas,
            })?;
        if schema_count > self.limits.max_schemas {
            return Err(SchemaError::SchemaLimitExceeded {
                limit: self.limits.max_schemas,
            });
        }

        let mut ids = BTreeMap::new();
        let mut schemas = Vec::with_capacity(self.schemas.len());
        for (ordinal, (identity, draft)) in self.schemas.iter().enumerate() {
            let id =
                SchemaId(
                    u32::try_from(ordinal).map_err(|_| SchemaError::SchemaLimitExceeded {
                        limit: self.limits.max_schemas,
                    })?,
                );
            ids.insert(identity.clone(), id);
            schemas.push((id, draft));
        }

        let mut total_fields = 0_u64;
        let mut total_bytes = 0_u128;
        let mut frozen = Vec::with_capacity(schemas.len());
        for (id, draft) in schemas {
            let field_count =
                u64::try_from(draft.fields.len()).map_err(|_| SchemaError::FieldLimitExceeded {
                    limit: self.limits.max_fields,
                })?;
            total_fields =
                total_fields
                    .checked_add(field_count)
                    .ok_or(SchemaError::FieldLimitExceeded {
                        limit: self.limits.max_fields,
                    })?;
            if total_fields > self.limits.max_fields {
                return Err(SchemaError::FieldLimitExceeded {
                    limit: self.limits.max_fields,
                });
            }

            let mut names = BTreeSet::new();
            let mut fields = Vec::with_capacity(draft.fields.len());
            for field in &draft.fields {
                if field.name.is_empty() {
                    return Err(SchemaError::EmptyField {
                        identity: draft.identity.clone(),
                    });
                }
                if !names.insert(&field.name) {
                    return Err(SchemaError::DuplicateField {
                        identity: draft.identity.clone(),
                        field: field.name.clone(),
                    });
                }
                let ty = resolve_type(
                    &field.ty,
                    &self.aliases,
                    &self.schemas,
                    &self.limits,
                    &mut BTreeSet::new(),
                    0,
                )?;
                total_bytes = total_bytes
                    .checked_add(static_size(&ty, &self.limits)?)
                    .ok_or(SchemaError::StaticSizeOverflow)?;
                if total_bytes > self.limits.max_static_bytes {
                    return Err(SchemaError::StaticBytesExceeded {
                        limit: self.limits.max_static_bytes,
                    });
                }
                fields.push(CanonicalField {
                    name: field.name.clone(),
                    ty,
                });
            }
            frozen.push(CanonicalSchema {
                id,
                identity: draft.identity.clone(),
                fields,
            });
        }

        Ok(SchemaRegistry {
            limits: self.limits,
            schemas: frozen,
            ids,
        })
    }
}

fn validate_identity_parts(owner: &str, name: &str) -> Result<(), SchemaError> {
    if owner.is_empty() {
        return Err(SchemaError::EmptyIdentity { kind: "owner" });
    }
    if name.is_empty() {
        return Err(SchemaError::EmptyIdentity { kind: "name" });
    }
    if owner.contains('/') || owner.contains('\\') {
        return Err(SchemaError::PathLikeOwner {
            owner: owner.to_owned(),
        });
    }
    Ok(())
}

fn validate_schema_identity(identity: &SchemaIdentity) -> Result<(), SchemaError> {
    validate_identity_parts(&identity.owner, &identity.name)?;
    if identity.type_args.iter().any(String::is_empty) {
        return Err(SchemaError::EmptyIdentity {
            kind: "type argument",
        });
    }
    Ok(())
}

fn validate_alias_identity(identity: &AliasIdentity) -> Result<(), SchemaError> {
    validate_identity_parts(&identity.owner, &identity.name)
}

fn resolve_type(
    ty: &TypeExpr,
    aliases: &BTreeMap<AliasIdentity, TypeExpr>,
    schemas: &BTreeMap<SchemaIdentity, SchemaDraft>,
    limits: &RegistryLimits,
    active_aliases: &mut BTreeSet<AliasIdentity>,
    depth: u32,
) -> Result<SemanticType, SchemaError> {
    if depth > limits.max_type_depth {
        return Err(SchemaError::TypeDepthExceeded {
            limit: limits.max_type_depth,
        });
    }
    match ty {
        TypeExpr::Scalar(scalar) => Ok(SemanticType::Scalar(*scalar)),
        TypeExpr::RecordRef(identity) => {
            validate_schema_identity(identity)?;
            if !schemas.contains_key(identity) {
                return Err(SchemaError::UnknownSchema {
                    identity: identity.clone(),
                });
            }
            Ok(SemanticType::RecordRef(identity.clone()))
        }
        TypeExpr::FixedArray { element, extent } => {
            if *extent > limits.max_extent {
                return Err(SchemaError::ExtentLimitExceeded {
                    extent: *extent,
                    limit: limits.max_extent,
                });
            }
            Ok(SemanticType::FixedArray {
                element: Box::new(resolve_type(
                    element,
                    aliases,
                    schemas,
                    limits,
                    active_aliases,
                    depth + 1,
                )?),
                extent: *extent,
            })
        }
        TypeExpr::DynamicArray { element } => Ok(SemanticType::DynamicArray {
            element: Box::new(resolve_type(
                element,
                aliases,
                schemas,
                limits,
                active_aliases,
                depth + 1,
            )?),
        }),
        TypeExpr::Alias(identity) => {
            validate_alias_identity(identity)?;
            let target = aliases
                .get(identity)
                .ok_or_else(|| SchemaError::UnknownAlias {
                    identity: identity.clone(),
                })?;
            if !active_aliases.insert(identity.clone()) {
                return Err(SchemaError::AliasCycle {
                    identity: identity.clone(),
                });
            }
            let resolved =
                resolve_type(target, aliases, schemas, limits, active_aliases, depth + 1);
            active_aliases.remove(identity);
            resolved
        }
        TypeExpr::InlineRecord { .. } => Err(SchemaError::InlineRecordUnsupported),
    }
}

fn static_size(ty: &SemanticType, limits: &RegistryLimits) -> Result<u128, SchemaError> {
    let size = match ty {
        SemanticType::Scalar(scalar) => match scalar {
            ScalarType::Bool => 1,
            ScalarType::I32
            | ScalarType::U32
            | ScalarType::F32
            | ScalarType::BF16
            | ScalarType::F16
            | ScalarType::Q16 => 4,
            ScalarType::I64 | ScalarType::F64 => 8,
        },
        // RecordRef is an opaque semantic reference; its physical ABI remains
        // a later backend decision, but the descriptor's reference slot is 8.
        SemanticType::RecordRef(_) => 8,
        // A dynamic container has no static element extent.  Its descriptor
        // itself is accounted as one slot while runtime allocation stays out
        // of this registry's validation policy.
        SemanticType::DynamicArray { .. } => 8,
        SemanticType::FixedArray { element, extent } => {
            let element_size = static_size(element, limits)?;
            element_size
                .checked_mul(u128::from(*extent))
                .ok_or(SchemaError::StaticSizeOverflow)?
        }
    };
    if size > limits.max_static_bytes {
        return Err(SchemaError::StaticBytesExceeded {
            limit: limits.max_static_bytes,
        });
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(owner: &str, name: &str) -> SchemaIdentity {
        SchemaIdentity::new(owner, name)
    }

    fn record(identity: SchemaIdentity, fields: Vec<FieldDraft>) -> SchemaDraft {
        SchemaDraft::new(identity, fields)
    }

    fn scalar(field: &str, ty: ScalarType) -> FieldDraft {
        FieldDraft::new(field, TypeExpr::Scalar(ty))
    }

    #[test]
    fn schema_ids_are_insertion_order_independent() {
        let a = record(id("pkg.core", "A"), vec![scalar("x", ScalarType::I64)]);
        let b = record(id("pkg.core", "B"), vec![scalar("y", ScalarType::F64)]);
        let mut first = SchemaRegistryBuilder::default();
        first.add_schema(a.clone()).unwrap();
        first.add_schema(b.clone()).unwrap();
        let mut second = SchemaRegistryBuilder::default();
        second.add_schema(b).unwrap();
        second.add_schema(a).unwrap();
        let first = first.finish().unwrap();
        let second = second.finish().unwrap();
        assert_eq!(first.schemas(), second.schemas());
        assert_eq!(first.schema_id(&id("pkg.core", "A")), Some(SchemaId(0)));
        assert_eq!(first.schema_id(&id("pkg.core", "B")), Some(SchemaId(1)));
    }

    #[test]
    fn declaration_order_and_alias_resolution_are_preserved() {
        let target = id("defining.module", "Value");
        let alias = AliasIdentity::new("consumer.module", "ImportedValue");
        let mut builder = SchemaRegistryBuilder::default();
        builder
            .add_schema(record(
                target.clone(),
                vec![
                    scalar("first", ScalarType::I32),
                    FieldDraft::new("second", TypeExpr::Alias(alias.clone())),
                ],
            ))
            .unwrap();
        builder
            .add_alias(alias, TypeExpr::RecordRef(target.clone()))
            .unwrap();
        let schema = builder.finish().unwrap().schema(&target).unwrap().clone();
        assert_eq!(schema.fields()[0].name(), "first");
        assert!(matches!(
            schema.fields()[1].ty(),
            SemanticType::RecordRef(found) if found == &target
        ));
    }

    #[test]
    fn record_reference_cycles_are_finite_and_valid() {
        let left = id("pkg", "Left");
        let right = id("pkg", "Right");
        let mut builder = SchemaRegistryBuilder::default();
        builder
            .add_schema(record(
                left.clone(),
                vec![FieldDraft::new("right", TypeExpr::RecordRef(right.clone()))],
            ))
            .unwrap();
        builder
            .add_schema(record(
                right.clone(),
                vec![FieldDraft::new("left", TypeExpr::RecordRef(left.clone()))],
            ))
            .unwrap();
        let registry = builder.finish().unwrap();
        assert_eq!(registry.schemas().len(), 2);
        assert!(matches!(
            registry.schema(&left).unwrap().fields()[0].ty(),
            SemanticType::RecordRef(found) if found == &right
        ));
    }

    #[test]
    fn unknown_and_conflicting_schemas_fail_closed() {
        let owner = id("pkg", "Owner");
        let missing = id("pkg", "Missing");
        let mut unknown = SchemaRegistryBuilder::default();
        unknown
            .add_schema(record(
                owner.clone(),
                vec![FieldDraft::new(
                    "missing",
                    TypeExpr::RecordRef(missing.clone()),
                )],
            ))
            .unwrap();
        assert!(matches!(
            unknown.finish(),
            Err(SchemaError::UnknownSchema { identity }) if identity == missing
        ));

        let mut conflict = SchemaRegistryBuilder::default();
        conflict
            .add_schema(record(owner.clone(), vec![scalar("x", ScalarType::I64)]))
            .unwrap();
        assert!(matches!(
            conflict.add_schema(record(owner, vec![scalar("x", ScalarType::F64)])),
            Err(SchemaError::ConflictingSchema { .. })
        ));
    }

    #[test]
    fn inline_record_and_alias_cycles_fail_without_expansion() {
        let owner = id("pkg", "Owner");
        let alias = AliasIdentity::new("pkg", "Loop");
        let mut inline = SchemaRegistryBuilder::default();
        inline
            .add_schema(record(
                owner.clone(),
                vec![FieldDraft::new(
                    "nested",
                    TypeExpr::InlineRecord { fields: Vec::new() },
                )],
            ))
            .unwrap();
        assert!(matches!(
            inline.finish(),
            Err(SchemaError::InlineRecordUnsupported)
        ));

        let mut aliases = SchemaRegistryBuilder::default();
        aliases
            .add_schema(record(
                owner,
                vec![FieldDraft::new("value", TypeExpr::Alias(alias.clone()))],
            ))
            .unwrap();
        aliases
            .add_alias(alias.clone(), TypeExpr::Alias(alias))
            .unwrap();
        assert!(matches!(
            aliases.finish(),
            Err(SchemaError::AliasCycle { .. })
        ));
    }

    #[test]
    fn checked_limits_reject_extent_depth_and_static_size() {
        let limits = RegistryLimits {
            max_schemas: 1,
            max_fields: 1,
            max_type_depth: 1,
            max_extent: 2,
            max_static_bytes: 8,
        };
        let mut extent = SchemaRegistryBuilder::new(limits);
        extent
            .add_schema(record(
                id("pkg", "TooWide"),
                vec![FieldDraft::new(
                    "values",
                    TypeExpr::FixedArray {
                        element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                        extent: 3,
                    },
                )],
            ))
            .unwrap();
        assert!(matches!(
            extent.finish(),
            Err(SchemaError::ExtentLimitExceeded { .. })
        ));

        let mut size = SchemaRegistryBuilder::new(RegistryLimits {
            max_static_bytes: 16,
            ..RegistryLimits::default()
        });
        size.add_schema(record(
            id("pkg", "TooLarge"),
            vec![FieldDraft::new(
                "values",
                TypeExpr::FixedArray {
                    element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                    extent: 2,
                },
            )],
        ))
        .unwrap();
        assert!(size.finish().is_ok());

        let mut depth = SchemaRegistryBuilder::new(limits);
        depth
            .add_schema(record(
                id("pkg", "TooDeep"),
                vec![FieldDraft::new(
                    "values",
                    TypeExpr::FixedArray {
                        element: Box::new(TypeExpr::FixedArray {
                            element: Box::new(TypeExpr::Scalar(ScalarType::I64)),
                            extent: 1,
                        }),
                        extent: 1,
                    },
                )],
            ))
            .unwrap();
        assert!(matches!(
            depth.finish(),
            Err(SchemaError::TypeDepthExceeded { .. })
        ));
    }

    #[test]
    fn duplicate_fields_and_path_owners_are_rejected() {
        let mut duplicate = SchemaRegistryBuilder::default();
        duplicate
            .add_schema(record(
                id("pkg", "Pair"),
                vec![scalar("x", ScalarType::I64), scalar("x", ScalarType::I64)],
            ))
            .unwrap();
        assert!(matches!(
            duplicate.finish(),
            Err(SchemaError::DuplicateField { .. })
        ));

        let mut path = SchemaRegistryBuilder::default();
        assert!(matches!(
            path.add_schema(record(
                SchemaIdentity::new("/tmp/project", "Bad"),
                Vec::new(),
            )),
            Err(SchemaError::PathLikeOwner { .. })
        ));
    }
}
