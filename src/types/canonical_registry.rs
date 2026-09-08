// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Alias resolution, validation, and deterministic registry freezing.

use std::collections::{BTreeMap, BTreeSet};

use super::canonical_types::*;

/// Frozen canonical registry. It is intentionally not attached to IRModule
/// until a later propagation/wire slice establishes one semantic authority.
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
    alias_count: u64,
    field_count: u64,
    identity_bytes: u64,
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
            alias_count: 0,
            field_count: 0,
            identity_bytes: 0,
        }
    }

    pub fn add_alias(
        &mut self,
        identity: AliasIdentity,
        target: TypeExpr,
    ) -> Result<(), SchemaError> {
        validate_alias_identity(&identity)?;
        validate_type_expr_shape(&target, &self.limits, 0)?;
        if self.aliases.contains_key(&identity) {
            return Err(SchemaError::DuplicateAlias { identity });
        }
        let alias_count =
            self.alias_count
                .checked_add(1)
                .ok_or(SchemaError::AliasLimitExceeded {
                    limit: self.limits.max_aliases,
                })?;
        if alias_count > self.limits.max_aliases {
            return Err(SchemaError::AliasLimitExceeded {
                limit: self.limits.max_aliases,
            });
        }
        let identity_bytes = checked_alias_identity_bytes(&identity, &target)?;
        let new_bytes = self.identity_bytes.checked_add(identity_bytes).ok_or(
            SchemaError::IdentityBytesExceeded {
                limit: self.limits.max_identity_bytes,
            },
        )?;
        if new_bytes > self.limits.max_identity_bytes {
            return Err(SchemaError::IdentityBytesExceeded {
                limit: self.limits.max_identity_bytes,
            });
        }
        // All limit and duplicate checks precede insertion. A rejected alias
        // therefore cannot replace an accepted target or alter later results.
        self.aliases.insert(identity, target);
        self.alias_count = alias_count;
        self.identity_bytes = new_bytes;
        Ok(())
    }

    pub fn add_schema(&mut self, draft: SchemaDraft) -> Result<(), SchemaError> {
        validate_schema_identity(draft.identity())?;
        if let Some(existing) = self.schemas.get(draft.identity()) {
            return Err(if existing == &draft {
                SchemaError::DuplicateSchema {
                    identity: draft.identity().clone(),
                }
            } else {
                SchemaError::ConflictingSchema {
                    identity: draft.identity().clone(),
                }
            });
        }
        for field in draft.fields() {
            validate_type_expr_shape(field.ty(), &self.limits, 0)?;
        }
        let schema_count = u64::try_from(self.schemas.len())
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(SchemaError::SchemaLimitExceeded {
                limit: self.limits.max_schemas,
            })?;
        if schema_count > self.limits.max_schemas {
            return Err(SchemaError::SchemaLimitExceeded {
                limit: self.limits.max_schemas,
            });
        }
        let field_count =
            u64::try_from(draft.fields().len()).map_err(|_| SchemaError::FieldLimitExceeded {
                limit: self.limits.max_fields,
            })?;
        let new_fields =
            self.field_count
                .checked_add(field_count)
                .ok_or(SchemaError::FieldLimitExceeded {
                    limit: self.limits.max_fields,
                })?;
        if new_fields > self.limits.max_fields {
            return Err(SchemaError::FieldLimitExceeded {
                limit: self.limits.max_fields,
            });
        }
        validate_field_names(&draft)?;
        let identity_bytes = checked_identity_bytes(draft.identity(), &draft)?;
        let new_bytes = self.identity_bytes.checked_add(identity_bytes).ok_or(
            SchemaError::IdentityBytesExceeded {
                limit: self.limits.max_identity_bytes,
            },
        )?;
        if new_bytes > self.limits.max_identity_bytes {
            return Err(SchemaError::IdentityBytesExceeded {
                limit: self.limits.max_identity_bytes,
            });
        }
        self.schemas.insert(draft.identity().clone(), draft);
        self.field_count = new_fields;
        self.identity_bytes = new_bytes;
        Ok(())
    }

    pub fn finish(self) -> Result<SchemaRegistry, SchemaError> {
        // Resolve every alias, including aliases not referenced by a field.
        // This keeps accepted registry contents fully validated rather than
        // silently admitting an unreachable dangling or cyclic definition.
        let mut fixed_elements = 0_u128;
        for identity in self.aliases.keys() {
            let resolved = resolve_type(
                &TypeExpr::Alias(identity.clone()),
                &self.aliases,
                &self.schemas,
                &self.limits,
                &mut BTreeSet::new(),
                0,
            )?;
            add_fixed_elements(&resolved, &self.limits, &mut fixed_elements)?;
        }
        for draft in self.schemas.values() {
            for field in draft.fields() {
                let resolved = resolve_type(
                    field.ty(),
                    &self.aliases,
                    &self.schemas,
                    &self.limits,
                    &mut BTreeSet::new(),
                    0,
                )?;
                add_fixed_elements(&resolved, &self.limits, &mut fixed_elements)?;
            }
        }

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
        let mut frozen = Vec::with_capacity(self.schemas.len());
        for (ordinal, (identity, draft)) in self.schemas.iter().enumerate() {
            let id = SchemaId::from_index(u32::try_from(ordinal).map_err(|_| {
                SchemaError::SchemaLimitExceeded {
                    limit: self.limits.max_schemas,
                }
            })?);
            ids.insert(identity.clone(), id);
            let fields = draft
                .fields()
                .iter()
                .map(|field| {
                    Ok(CanonicalField::new(
                        field.name().to_owned(),
                        resolve_type(
                            field.ty(),
                            &self.aliases,
                            &self.schemas,
                            &self.limits,
                            &mut BTreeSet::new(),
                            0,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, SchemaError>>()?;
            frozen.push(CanonicalSchema::new(id, identity.clone(), fields));
        }
        Ok(SchemaRegistry {
            limits: self.limits,
            schemas: frozen,
            ids,
        })
    }
}

fn validate_field_names(draft: &SchemaDraft) -> Result<(), SchemaError> {
    let mut names = BTreeSet::new();
    for field in draft.fields() {
        if field.name().is_empty() {
            return Err(SchemaError::EmptyField {
                identity: draft.identity().clone(),
            });
        }
        if !names.insert(field.name()) {
            return Err(SchemaError::DuplicateField {
                identity: draft.identity().clone(),
                field: field.name().to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_identity_parts(owner: &str, name: &str) -> Result<(), SchemaError> {
    if owner.is_empty() {
        return Err(SchemaError::EmptyIdentity { kind: "owner" });
    }
    if name.is_empty() {
        return Err(SchemaError::EmptyIdentity { kind: "name" });
    }
    if owner.contains('/') || owner.contains('\\') {
        return Err(SchemaError::PathLikeIdentity {
            value: owner.to_owned(),
        });
    }
    Ok(())
}

fn validate_schema_identity(identity: &SchemaIdentity) -> Result<(), SchemaError> {
    validate_identity_parts(identity.owner(), identity.name())?;
    if !identity.type_args().is_empty() {
        return Err(SchemaError::UnsupportedTypeArguments {
            identity: identity.clone(),
        });
    }
    Ok(())
}

fn validate_alias_identity(identity: &AliasIdentity) -> Result<(), SchemaError> {
    validate_identity_parts(identity.owner(), identity.name())
}

fn validate_type_expr_shape(
    ty: &TypeExpr,
    limits: &RegistryLimits,
    depth: u32,
) -> Result<(), SchemaError> {
    if depth > limits.max_type_depth {
        return Err(SchemaError::TypeDepthExceeded {
            limit: limits.max_type_depth,
        });
    }
    match ty {
        TypeExpr::Scalar(_) => Ok(()),
        TypeExpr::RecordRef(identity) => validate_schema_identity(identity),
        TypeExpr::FixedArray { element, extent } => {
            if *extent > limits.max_extent {
                return Err(SchemaError::ExtentLimitExceeded {
                    extent: *extent,
                    limit: limits.max_extent,
                });
            }
            validate_type_expr_shape(element, limits, depth + 1)
        }
        TypeExpr::DynamicArray { element } => validate_type_expr_shape(element, limits, depth + 1),
        TypeExpr::Alias(identity) => validate_alias_identity(identity),
        TypeExpr::InlineRecord { fields } => fields
            .iter()
            .try_for_each(|field| validate_type_expr_shape(field.ty(), limits, depth + 1)),
    }
}

fn checked_identity_bytes<T>(identity: &SchemaIdentity, value: &T) -> Result<u64, SchemaError>
where
    T: TypeIdentityBytes,
{
    let mut total = identity_bytes(identity)?;
    total = total
        .checked_add(value.identity_bytes()?)
        .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
    Ok(total)
}

fn checked_alias_identity_bytes(
    identity: &AliasIdentity,
    target: &TypeExpr,
) -> Result<u64, SchemaError> {
    alias_identity_bytes(identity)?
        .checked_add(target.identity_bytes()?)
        .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })
}

trait TypeIdentityBytes {
    fn identity_bytes(&self) -> Result<u64, SchemaError>;
}

impl TypeIdentityBytes for SchemaDraft {
    fn identity_bytes(&self) -> Result<u64, SchemaError> {
        let mut total = 0_u64;
        for field in self.fields() {
            total = total
                .checked_add(checked_string_len(field.name())?)
                .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
            total = total
                .checked_add(type_expr_identity_bytes(field.ty())?)
                .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
        }
        Ok(total)
    }
}

impl TypeIdentityBytes for TypeExpr {
    fn identity_bytes(&self) -> Result<u64, SchemaError> {
        type_expr_identity_bytes(self)
    }
}

fn identity_bytes(identity: &SchemaIdentity) -> Result<u64, SchemaError> {
    let mut total = checked_string_len(identity.owner())?
        .checked_add(checked_string_len(identity.name())?)
        .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
    for arg in identity.type_args() {
        total = total
            .checked_add(checked_string_len(arg)?)
            .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
    }
    Ok(total)
}

fn alias_identity_bytes(identity: &AliasIdentity) -> Result<u64, SchemaError> {
    checked_string_len(identity.owner())?
        .checked_add(checked_string_len(identity.name())?)
        .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })
}

fn checked_string_len(value: &str) -> Result<u64, SchemaError> {
    u64::try_from(value.len()).map_err(|_| SchemaError::IdentityBytesExceeded { limit: u64::MAX })
}

fn type_expr_identity_bytes(ty: &TypeExpr) -> Result<u64, SchemaError> {
    match ty {
        TypeExpr::Scalar(_) => Ok(0),
        TypeExpr::RecordRef(identity) => identity_bytes(identity),
        TypeExpr::FixedArray { element, .. } | TypeExpr::DynamicArray { element } => {
            type_expr_identity_bytes(element)
        }
        TypeExpr::Alias(identity) => alias_identity_bytes(identity),
        TypeExpr::InlineRecord { fields } => {
            let mut total = 0_u64;
            for field in fields {
                total = total
                    .checked_add(checked_string_len(field.name())?)
                    .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
                total = total
                    .checked_add(type_expr_identity_bytes(field.ty())?)
                    .ok_or(SchemaError::IdentityBytesExceeded { limit: u64::MAX })?;
            }
            Ok(total)
        }
    }
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

fn add_fixed_elements(
    ty: &SemanticType,
    limits: &RegistryLimits,
    cumulative: &mut u128,
) -> Result<u128, SchemaError> {
    let elements = descriptor_elements(ty)?;
    *cumulative = cumulative
        .checked_add(elements)
        .ok_or(SchemaError::FixedElementOverflow)?;
    if elements > limits.max_fixed_elements || *cumulative > limits.max_fixed_elements {
        return Err(SchemaError::FixedElementLimitExceeded {
            limit: limits.max_fixed_elements,
        });
    }
    Ok(elements)
}

fn descriptor_elements(ty: &SemanticType) -> Result<u128, SchemaError> {
    match ty {
        SemanticType::Scalar(_)
        | SemanticType::RecordRef(_)
        | SemanticType::DynamicArray { .. } => Ok(1),
        SemanticType::FixedArray { element, extent } => descriptor_elements(element)?
            .checked_mul(u128::from(*extent))
            .ok_or(SchemaError::FixedElementOverflow),
    }
}
