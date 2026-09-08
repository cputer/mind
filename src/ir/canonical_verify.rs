// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Verification of the transient B1 semantic carrier.
//!
//! This module deliberately does not infer types from instruction order or
//! from the legacy i64 storage representation.  It checks only metadata that
//! was supplied by the resolver and the exact SSA scope that owns it.

use std::collections::BTreeSet;

use crate::types::{
    CanonicalModuleTypes, FunctionIdentity, FunctionKind, FunctionSemanticTypes, SemanticType,
};

use super::{IRModule, Instr, ValueId};

impl Instr {
    pub(crate) fn legacy_call(dst: ValueId, name: impl Into<String>, args: Vec<ValueId>) -> Self {
        Self::Call {
            dst,
            name: name.into(),
            args,
            resolved_callee: None,
        }
    }
}

/// A malformed or incomplete B1 semantic carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalMetadataError {
    InvalidBundle(String),
    MissingFunctionMetadata {
        name: String,
    },
    DuplicateFunctionIdentity {
        identity: String,
    },
    FunctionNotRegistered {
        identity: String,
    },
    FunctionNotDefined {
        identity: String,
    },
    NonLocalFunctionBody {
        identity: String,
        kind: FunctionKind,
    },
    FunctionIdentityMismatch {
        name: String,
    },
    ValueOutsideScope {
        function: String,
        value: ValueId,
    },
    MissingParameterType {
        function: String,
        value: ValueId,
    },
    ParameterArityMismatch {
        function: String,
    },
    ParameterTypeMismatch {
        function: String,
        value: ValueId,
    },
    InvalidFunctionValueType {
        function: String,
        value: ValueId,
    },
    ReturnTypeMismatch {
        function: String,
        value: ValueId,
    },
    UnresolvedTypedCall {
        name: String,
    },
    CalleeNotRegistered {
        identity: String,
    },
    CallArityMismatch {
        identity: String,
    },
    CallArgumentTypeMismatch {
        identity: String,
        value: ValueId,
    },
    CallReturnTypeMismatch {
        identity: String,
        value: ValueId,
    },
    OptimizationUnsupported,
    #[cfg(feature = "std-surface")]
    LegacyTypeConflict {
        function: String,
        value: ValueId,
    },
    #[cfg(feature = "std-surface")]
    MissingCanonicalLegacyType {
        function: String,
        value: ValueId,
    },
}

impl std::fmt::Display for CanonicalMetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBundle(message) => write!(f, "invalid canonical metadata: {message}"),
            Self::MissingFunctionMetadata { name } => {
                write!(
                    f,
                    "function {name} is missing co-located canonical metadata"
                )
            }
            Self::DuplicateFunctionIdentity { identity } => {
                write!(f, "duplicate canonical function identity {identity}")
            }
            Self::FunctionNotRegistered { identity } => {
                write!(
                    f,
                    "function {identity} is not in the module signature registry"
                )
            }
            Self::FunctionNotDefined { identity } => {
                write!(f, "local function {identity} has no co-located FnDef body")
            }
            Self::NonLocalFunctionBody { identity, kind } => {
                write!(f, "{kind:?} function {identity} cannot carry a local body")
            }
            Self::FunctionIdentityMismatch { name } => {
                write!(f, "function metadata identity does not match {name}")
            }
            Self::ValueOutsideScope { function, value } => {
                write!(
                    f,
                    "canonical value {value} is outside function scope {function}"
                )
            }
            Self::MissingParameterType { function, value } => {
                write!(f, "parameter {value} has no canonical type in {function}")
            }
            Self::ParameterArityMismatch { function } => {
                write!(f, "canonical parameter arity mismatch in {function}")
            }
            Self::ParameterTypeMismatch { function, value } => {
                write!(
                    f,
                    "canonical parameter type mismatch for {value} in {function}"
                )
            }
            Self::InvalidFunctionValueType { function, value } => {
                write!(
                    f,
                    "canonical value {value} has an invalid semantic shape in {function}"
                )
            }
            Self::ReturnTypeMismatch { function, value } => {
                write!(
                    f,
                    "canonical return type mismatch for {value} in {function}"
                )
            }
            Self::UnresolvedTypedCall { name } => {
                write!(f, "typed call {name} has no resolved canonical identity")
            }
            Self::CalleeNotRegistered { identity } => {
                write!(f, "resolved callee {identity} is not registered")
            }
            Self::CallArityMismatch { identity } => {
                write!(f, "call arity mismatch for {identity}")
            }
            Self::CallArgumentTypeMismatch { identity, value } => {
                write!(f, "call argument {value} has the wrong type for {identity}")
            }
            Self::CallReturnTypeMismatch { identity, value } => {
                write!(f, "call result {value} has the wrong type for {identity}")
            }
            Self::OptimizationUnsupported => {
                write!(
                    f,
                    "the legacy optimizer cannot preserve populated canonical metadata"
                )
            }
            #[cfg(feature = "std-surface")]
            Self::LegacyTypeConflict { function, value } => {
                write!(
                    f,
                    "legacy aggregate type conflicts with canonical type for {value} in {function}"
                )
            }
            #[cfg(feature = "std-surface")]
            Self::MissingCanonicalLegacyType { function, value } => {
                write!(
                    f,
                    "legacy aggregate type for {value} has no canonical type in {function}"
                )
            }
        }
    }
}

impl std::error::Error for CanonicalMetadataError {}

/// Verify the optional B1 carrier.  An absent carrier is the legacy state and
/// remains valid.  Once present, every supplied scope and resolved call is
/// checked against its defining registry and owning SSA stream.
pub fn verify_canonical_metadata(module: &IRModule) -> Result<(), CanonicalMetadataError> {
    let Some(bundle) = module.canonical_types.as_ref() else {
        if instruction_metadata_present(&module.instrs) {
            return Err(CanonicalMetadataError::InvalidBundle(
                "instruction metadata has no canonical module bundle".to_string(),
            ));
        }
        return Ok(());
    };
    bundle
        .validate()
        .map_err(|error| CanonicalMetadataError::InvalidBundle(error.to_string()))?;
    let authority_present = !bundle.is_empty() || instruction_metadata_present(&module.instrs);
    let module_defs = scope_definitions(&module.instrs, std::iter::empty());
    for value in bundle.module_values().keys() {
        if !module_defs.contains(value) {
            return Err(CanonicalMetadataError::ValueOutsideScope {
                function: "<module>".to_string(),
                value: *value,
            });
        }
    }
    #[cfg(feature = "std-surface")]
    if authority_present {
        verify_legacy_scope_compatibility(&module.value_types, bundle.module_values(), "<module>")?;
    }
    let mut identities = BTreeSet::new();
    let mut local_definitions = BTreeSet::new();
    verify_stream(
        &module.instrs,
        bundle,
        None,
        authority_present,
        &mut identities,
        &mut local_definitions,
    )?;
    for (identity, declaration) in bundle.functions() {
        if declaration.kind() == FunctionKind::Local && !local_definitions.contains(identity) {
            return Err(CanonicalMetadataError::FunctionNotDefined {
                identity: identity.to_string(),
            });
        }
    }
    Ok(())
}

/// Detect co-located B1 fields independently of the optional module bundle.
/// A caller must not be able to put semantic data on an instruction and then
/// fall back to the legacy metadata-free serializer.
pub(crate) fn instruction_metadata_present(instrs: &[Instr]) -> bool {
    let mut pending = Vec::new();
    let mut stream = instrs;
    loop {
        for instr in stream {
            match instr {
                Instr::FnDef {
                    semantic_types,
                    body,
                    ..
                } => {
                    if semantic_types.is_some() {
                        return true;
                    }
                    pending.push(body.as_slice());
                }
                Instr::Call {
                    resolved_callee, ..
                } if resolved_callee.is_some() => return true,
                #[cfg(feature = "std-surface")]
                Instr::If {
                    cond_instrs,
                    then_instrs,
                    else_instrs,
                    ..
                } => pending.extend([cond_instrs.as_slice(), then_instrs, else_instrs]),
                #[cfg(feature = "std-surface")]
                Instr::While {
                    cond_instrs, body, ..
                } => pending.extend([cond_instrs.as_slice(), body]),
                #[cfg(feature = "std-surface")]
                Instr::Region { body, .. } => pending.push(body),
                _ => {}
            }
        }
        let Some(next) = pending.pop() else {
            return false;
        };
        stream = next;
    }
}

fn verify_stream(
    instrs: &[Instr],
    bundle: &CanonicalModuleTypes,
    owner: Option<&FunctionSemanticTypes>,
    authority_present: bool,
    identities: &mut BTreeSet<FunctionIdentity>,
    local_definitions: &mut BTreeSet<FunctionIdentity>,
) -> Result<(), CanonicalMetadataError> {
    for instr in instrs {
        match instr {
            Instr::FnDef {
                name,
                params,
                ret_id,
                body,
                semantic_types,
                #[cfg(feature = "std-surface")]
                value_types,
                ..
            } => {
                let Some(semantic) = semantic_types.as_ref() else {
                    if authority_present {
                        return Err(CanonicalMetadataError::MissingFunctionMetadata {
                            name: name.clone(),
                        });
                    }
                    continue;
                };
                let identity_key = semantic.identity().clone();
                let identity = identity_key.to_string();
                if !identities.insert(identity_key.clone()) {
                    return Err(CanonicalMetadataError::DuplicateFunctionIdentity { identity });
                }
                if semantic.identity().name() != name {
                    return Err(CanonicalMetadataError::FunctionIdentityMismatch {
                        name: name.clone(),
                    });
                }
                let Some(registered) = bundle.functions().get(semantic.identity()) else {
                    return Err(CanonicalMetadataError::FunctionNotRegistered { identity });
                };
                if registered.kind() != FunctionKind::Local {
                    return Err(CanonicalMetadataError::NonLocalFunctionBody {
                        identity,
                        kind: registered.kind(),
                    });
                }
                local_definitions.insert(identity_key);
                if params.len() != registered.signature().params().len() {
                    return Err(CanonicalMetadataError::ParameterArityMismatch {
                        function: identity.clone(),
                    });
                }
                for ((_, value), expected) in params.iter().zip(registered.signature().params()) {
                    let Some(actual) = semantic.values().get(value) else {
                        return Err(CanonicalMetadataError::MissingParameterType {
                            function: identity.clone(),
                            value: *value,
                        });
                    };
                    if actual != expected {
                        return Err(CanonicalMetadataError::ParameterTypeMismatch {
                            function: identity.clone(),
                            value: *value,
                        });
                    }
                }
                if let Some(value) = ret_id {
                    if registered
                        .signature()
                        .return_type()
                        .is_none_or(|expected| semantic.values().get(value) != Some(expected))
                    {
                        return Err(CanonicalMetadataError::ReturnTypeMismatch {
                            function: identity.clone(),
                            value: *value,
                        });
                    }
                } else if registered.signature().return_type().is_some() {
                    return Err(CanonicalMetadataError::ReturnTypeMismatch {
                        function: identity.clone(),
                        value: ValueId(usize::MAX),
                    });
                }
                let function_defs = scope_definitions(body, params.iter().map(|(_, v)| *v));
                for value in semantic.values().keys() {
                    if !function_defs.contains(value) {
                        return Err(CanonicalMetadataError::ValueOutsideScope {
                            function: identity.clone(),
                            value: *value,
                        });
                    }
                }
                let mut function_descriptor_total = 0_u128;
                for (value, ty) in semantic.values() {
                    let Ok(cost) = bundle.validate_value_type(ty) else {
                        return Err(CanonicalMetadataError::InvalidFunctionValueType {
                            function: identity.clone(),
                            value: *value,
                        });
                    };
                    function_descriptor_total = function_descriptor_total
                        .checked_add(cost)
                        .ok_or_else(|| CanonicalMetadataError::InvalidFunctionValueType {
                            function: identity.clone(),
                            value: *value,
                        })?;
                    if function_descriptor_total
                        > bundle.schema_registry().limits().max_fixed_elements
                    {
                        return Err(CanonicalMetadataError::InvalidFunctionValueType {
                            function: identity.clone(),
                            value: *value,
                        });
                    }
                }
                #[cfg(feature = "std-surface")]
                if authority_present {
                    verify_legacy_scope_compatibility(value_types, semantic.values(), &identity)?;
                }
                verify_stream(
                    body,
                    bundle,
                    Some(semantic),
                    authority_present,
                    identities,
                    local_definitions,
                )?;
            }
            Instr::Call {
                dst,
                name,
                args,
                resolved_callee,
            } => {
                let Some(identity) = resolved_callee.as_ref() else {
                    if authority_present {
                        return Err(CanonicalMetadataError::UnresolvedTypedCall {
                            name: name.clone(),
                        });
                    }
                    continue;
                };
                let Some(function) = bundle.functions().get(identity) else {
                    return Err(CanonicalMetadataError::CalleeNotRegistered {
                        identity: identity.to_string(),
                    });
                };
                if args.len() != function.signature().params().len() {
                    return Err(CanonicalMetadataError::CallArityMismatch {
                        identity: identity.to_string(),
                    });
                }
                for (value, expected) in args.iter().zip(function.signature().params()) {
                    if type_for(*value, bundle, owner) != Some(expected) {
                        return Err(CanonicalMetadataError::CallArgumentTypeMismatch {
                            identity: identity.to_string(),
                            value: *value,
                        });
                    }
                }
                if let Some(expected) = function.signature().return_type() {
                    if type_for(*dst, bundle, owner) != Some(expected) {
                        return Err(CanonicalMetadataError::CallReturnTypeMismatch {
                            identity: identity.to_string(),
                            value: *dst,
                        });
                    }
                }
            }
            #[cfg(feature = "std-surface")]
            Instr::If {
                cond_instrs,
                then_instrs,
                else_instrs,
                ..
            } => {
                verify_stream(
                    cond_instrs,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
                verify_stream(
                    then_instrs,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
                verify_stream(
                    else_instrs,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
            }
            #[cfg(feature = "std-surface")]
            Instr::While {
                cond_instrs, body, ..
            } => {
                verify_stream(
                    cond_instrs,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
                verify_stream(
                    body,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
            }
            #[cfg(feature = "std-surface")]
            Instr::Region { body, .. } => {
                verify_stream(
                    body,
                    bundle,
                    owner,
                    authority_present,
                    identities,
                    local_definitions,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(feature = "std-surface")]
fn verify_legacy_scope_compatibility(
    legacy: &std::collections::BTreeMap<ValueId, super::ArrayType>,
    canonical: &std::collections::BTreeMap<ValueId, SemanticType>,
    function: &str,
) -> Result<(), CanonicalMetadataError> {
    for (value, legacy_type) in legacy {
        let Some(canonical_type) = canonical.get(value) else {
            return Err(CanonicalMetadataError::MissingCanonicalLegacyType {
                function: function.to_string(),
                value: *value,
            });
        };
        if !legacy_matches_canonical(legacy_type, canonical_type) {
            return Err(CanonicalMetadataError::LegacyTypeConflict {
                function: function.to_string(),
                value: *value,
            });
        }
    }
    Ok(())
}

#[cfg(feature = "std-surface")]
fn legacy_matches_canonical(legacy: &super::ArrayType, canonical: &SemanticType) -> bool {
    let (element, size_matches) = match canonical {
        SemanticType::FixedArray { element, extent } => (
            element.as_ref(),
            matches!(legacy.size, super::ArraySize::Fixed(n) if n == *extent),
        ),
        SemanticType::DynamicArray { element } => (
            element.as_ref(),
            matches!(legacy.size, super::ArraySize::Dynamic),
        ),
        SemanticType::Scalar(_) | SemanticType::RecordRef(_) => return false,
    };
    size_matches
        && matches!(
            (element, &legacy.elem_dtype),
            (
                SemanticType::Scalar(crate::types::ScalarType::I32),
                crate::types::DType::I32
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::I64),
                crate::types::DType::I64
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::F32),
                crate::types::DType::F32
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::F64),
                crate::types::DType::F64
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::BF16),
                crate::types::DType::BF16
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::F16),
                crate::types::DType::F16
            ) | (
                SemanticType::Scalar(crate::types::ScalarType::Q16),
                crate::types::DType::Q16
            )
        )
}

fn type_for<'a>(
    value: ValueId,
    bundle: &'a CanonicalModuleTypes,
    owner: Option<&'a FunctionSemanticTypes>,
) -> Option<&'a SemanticType> {
    match owner {
        Some(function) => function.values().get(&value),
        None => bundle.module_values().get(&value),
    }
}

fn scope_definitions<I>(instrs: &[Instr], params: I) -> BTreeSet<ValueId>
where
    I: IntoIterator<Item = ValueId>,
{
    let mut definitions = params.into_iter().collect();
    scope_definitions_into(instrs, &mut definitions);
    definitions
}

fn scope_definitions_into(instrs: &[Instr], definitions: &mut BTreeSet<ValueId>) {
    for instr in instrs {
        if let Some(dst) = super::instruction_dst(instr) {
            definitions.insert(dst);
        }
        match instr {
            Instr::FnDef { .. } => {}
            #[cfg(feature = "std-surface")]
            Instr::If {
                cond_instrs,
                then_instrs,
                else_instrs,
                dst,
                merges,
                ..
            } => {
                definitions.insert(*dst);
                definitions.extend(merges.iter().map(|(merge_id, _, _)| *merge_id));
                scope_definitions_into(cond_instrs, definitions);
                scope_definitions_into(then_instrs, definitions);
                scope_definitions_into(else_instrs, definitions);
            }
            #[cfg(feature = "std-surface")]
            Instr::While {
                cond_instrs,
                body,
                exit_ids,
                ..
            } => {
                definitions.extend(exit_ids.iter().copied());
                scope_definitions_into(cond_instrs, definitions);
                scope_definitions_into(body, definitions);
            }
            #[cfg(feature = "std-surface")]
            Instr::Region { body, result, .. } => {
                definitions.insert(*result);
                scope_definitions_into(body, definitions);
            }
            _ => {}
        }
    }
}
