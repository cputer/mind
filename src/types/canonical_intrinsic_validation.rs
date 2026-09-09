// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0

//! Backend-profile validation for canonical intrinsic declarations.

use super::canonical_types::*;

/// Validate the backend profile separately from the wire declaration.  The
/// v04 declaration format intentionally carries no backend selector; admission
/// code must ask this seam before assigning a native capability.
pub(crate) fn validate_intrinsic_profile(
    identity: &FunctionIdentity,
    profile: crate::intrinsics::IntrinsicProfile,
) -> Result<(), SchemaError> {
    // This seam is also callable by future admission code, so it must not
    // treat owner/name alone as a validated identity. Generic identities are
    // rejected by canonical declaration validation and are rejected here as
    // well before capability lookup can succeed.
    if !identity.type_args().is_empty() {
        return Err(SchemaError::UnsupportedFunctionTypeArguments {
            identity: identity.clone(),
        });
    }
    if identity.owner() != "__mind_intrinsic" {
        return Err(SchemaError::InvalidIntrinsicIdentity {
            identity: identity.clone(),
        });
    }
    if crate::intrinsics::intrinsic_contract(identity.name()).is_none() {
        return Err(SchemaError::UnknownIntrinsic {
            identity: identity.clone(),
        });
    }
    if !crate::intrinsics::intrinsic_supports_profile(identity.name(), profile) {
        return Err(SchemaError::IntrinsicProfileMismatch {
            identity: identity.clone(),
            profile: match profile {
                crate::intrinsics::IntrinsicProfile::FrozenNative => "FrozenNative",
                crate::intrinsics::IntrinsicProfile::RustMlir => "RustMlir",
            },
        });
    }
    Ok(())
}
