// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0

//! Contract metadata shared by the intrinsic registry and canonical types.

use crate::intrinsics::Det;

/// The native result-use policy visible to canonical metadata. Stores retain
/// their historical i64 wire return for compatibility, but the FrozenNative
/// emitter does not define a usable synthetic destination for them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicResult {
    I64,
    DiscardOnly,
}

/// A backend profile whose ABI is described by a contract row. FrozenNative
/// provenance is kept distinct from the existing Rust/MLIR rows; a profile is
/// present only when that emitter's support is established by the registry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicProfile {
    FrozenNative,
    RustMlir,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicEffect {
    Argc,
    Argv,
    OpenReadOnly,
    ArenaAlloc,
    MemoryRead { width_bytes: u8 },
    MemoryWrite { width_bytes: u8 },
    FdRead,
    FdWrite,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicOffsetPolicy {
    Any,
    MustBeMinusOneIgnored,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicValueType {
    I64,
}

/// The proven emitter implementation attached to a contract.  The seed ID is
/// provenance, not a public wire or CLI selector; later admission code can
/// require the exact frozen implementation before assigning native capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntrinsicImplementation {
    FrozenNative { seed_id: u8 },
}

/// One row in the intrinsic registry.  Legacy rows retain their historical
/// arity/determinism behaviour, while the ten FrozenNative rows additionally
/// carry exact logical spelling, ABI result, effect, and profile metadata.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct IntrinsicSpec {
    pub(crate) physical_name: &'static str,
    pub(crate) arity: usize,
    pub(crate) det: Det,
    pub(crate) contract: Option<IntrinsicContract>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct IntrinsicContract {
    pub(crate) logical_name: &'static str,
    pub(crate) parameters: &'static [IntrinsicValueType],
    pub(crate) result: IntrinsicResult,
    pub(crate) effect: IntrinsicEffect,
    pub(crate) offset_policy: IntrinsicOffsetPolicy,
    pub(crate) profiles: &'static [IntrinsicProfile],
    pub(crate) implementation: IntrinsicImplementation,
}

const FROZEN_NATIVE_ONLY: &[IntrinsicProfile] = &[IntrinsicProfile::FrozenNative];
const FROZEN_AND_RUST_MLIR: &[IntrinsicProfile] =
    &[IntrinsicProfile::FrozenNative, IntrinsicProfile::RustMlir];

impl IntrinsicSpec {
    pub(crate) const fn legacy(physical_name: &'static str, arity: usize, det: Det) -> Self {
        Self {
            physical_name,
            arity,
            det,
            contract: None,
        }
    }

    pub(crate) const fn frozen(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        result: IntrinsicResult,
        effect: IntrinsicEffect,
        offset_policy: IntrinsicOffsetPolicy,
        seed_id: u8,
    ) -> Self {
        Self {
            physical_name,
            arity: parameters.len(),
            det: Det::Pure,
            contract: Some(IntrinsicContract {
                logical_name,
                parameters,
                result,
                effect,
                offset_policy,
                profiles: FROZEN_AND_RUST_MLIR,
                implementation: IntrinsicImplementation::FrozenNative { seed_id },
            }),
        }
    }

    pub(crate) const fn frozen_native_only(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        result: IntrinsicResult,
        effect: IntrinsicEffect,
        offset_policy: IntrinsicOffsetPolicy,
        seed_id: u8,
    ) -> Self {
        Self {
            physical_name,
            arity: parameters.len(),
            det: Det::Pure,
            contract: Some(IntrinsicContract {
                logical_name,
                parameters,
                result,
                effect,
                offset_policy,
                profiles: FROZEN_NATIVE_ONLY,
                implementation: IntrinsicImplementation::FrozenNative { seed_id },
            }),
        }
    }

    pub(crate) const fn frozen_value(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        effect: IntrinsicEffect,
        seed_id: u8,
    ) -> Self {
        Self::frozen(
            physical_name,
            logical_name,
            parameters,
            IntrinsicResult::I64,
            effect,
            IntrinsicOffsetPolicy::Any,
            seed_id,
        )
    }

    pub(crate) const fn frozen_native_value(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        effect: IntrinsicEffect,
        seed_id: u8,
    ) -> Self {
        Self::frozen_native_only(
            physical_name,
            logical_name,
            parameters,
            IntrinsicResult::I64,
            effect,
            IntrinsicOffsetPolicy::Any,
            seed_id,
        )
    }

    pub(crate) const fn frozen_discard(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        effect: IntrinsicEffect,
        seed_id: u8,
    ) -> Self {
        Self::frozen(
            physical_name,
            logical_name,
            parameters,
            IntrinsicResult::DiscardOnly,
            effect,
            IntrinsicOffsetPolicy::Any,
            seed_id,
        )
    }

    pub(crate) const fn frozen_io(
        physical_name: &'static str,
        logical_name: &'static str,
        parameters: &'static [IntrinsicValueType],
        effect: IntrinsicEffect,
        seed_id: u8,
    ) -> Self {
        Self::frozen(
            physical_name,
            logical_name,
            parameters,
            IntrinsicResult::I64,
            effect,
            IntrinsicOffsetPolicy::MustBeMinusOneIgnored,
            seed_id,
        )
    }
}

pub(crate) const NO_I64: &[IntrinsicValueType] = &[];
pub(crate) const ONE_I64: &[IntrinsicValueType] = &[IntrinsicValueType::I64];
pub(crate) const TWO_I64: &[IntrinsicValueType] =
    &[IntrinsicValueType::I64, IntrinsicValueType::I64];
pub(crate) const FOUR_I64: &[IntrinsicValueType] = &[
    IntrinsicValueType::I64,
    IntrinsicValueType::I64,
    IntrinsicValueType::I64,
    IntrinsicValueType::I64,
];

/// Look up a canonical intrinsic declaration by its logical identity name.
/// The owner is checked by the canonical-types layer; only exact contract rows
/// are exposed here.
pub(crate) fn intrinsic_contract(logical_name: &str) -> Option<&'static IntrinsicContract> {
    crate::intrinsics::STD_SURFACE_INTRINSICS
        .iter()
        .find_map(|spec| {
            spec.contract
                .as_ref()
                .filter(|contract| contract.logical_name == logical_name)
        })
}

pub(crate) fn intrinsic_supports_profile(logical_name: &str, profile: IntrinsicProfile) -> bool {
    intrinsic_contract(logical_name).is_some_and(|contract| contract.profiles.contains(&profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intrinsics::STD_SURFACE_INTRINSICS;
    #[cfg(feature = "std-surface")]
    use crate::intrinsics::std_surface_intrinsic_arity;

    #[test]
    fn frozen_native_contract_registry_has_exact_first_cut_surface() {
        let mut rows: Vec<&IntrinsicContract> = STD_SURFACE_INTRINSICS
            .iter()
            .filter_map(|spec| spec.contract.as_ref())
            .collect();
        rows.sort_by_key(|contract| contract.logical_name);
        assert_eq!(
            rows.iter()
                .map(|contract| contract.logical_name)
                .collect::<Vec<_>>(),
            vec![
                "alloc",
                "argc",
                "argv",
                "load8",
                "load_i64",
                "open",
                "read",
                "store8",
                "store_i64",
                "write",
            ]
        );
        for (name, seed_id) in [
            ("argc", 10),
            ("argv", 11),
            ("open", 12),
            ("alloc", 1),
            ("load_i64", 3),
            ("store_i64", 2),
            ("load8", 5),
            ("store8", 4),
            ("read", 9),
            ("write", 8),
        ] {
            assert!(matches!(
                intrinsic_contract(name).expect("contract row").implementation,
                IntrinsicImplementation::FrozenNative { seed_id: id } if id == seed_id
            ));
        }
        for (name, effect) in [
            ("alloc", IntrinsicEffect::ArenaAlloc),
            ("argc", IntrinsicEffect::Argc),
            ("argv", IntrinsicEffect::Argv),
            ("load8", IntrinsicEffect::MemoryRead { width_bytes: 1 }),
            ("load_i64", IntrinsicEffect::MemoryRead { width_bytes: 8 }),
            ("open", IntrinsicEffect::OpenReadOnly),
            ("read", IntrinsicEffect::FdRead),
            ("store8", IntrinsicEffect::MemoryWrite { width_bytes: 1 }),
            ("store_i64", IntrinsicEffect::MemoryWrite { width_bytes: 8 }),
            ("write", IntrinsicEffect::FdWrite),
        ] {
            assert_eq!(
                intrinsic_contract(name).expect("contract row").effect,
                effect
            );
        }
        for name in ["read", "write"] {
            assert_eq!(
                intrinsic_contract(name)
                    .expect("I/O contract")
                    .offset_policy,
                IntrinsicOffsetPolicy::MustBeMinusOneIgnored
            );
        }
        for name in [
            "alloc",
            "argc",
            "argv",
            "load8",
            "load_i64",
            "open",
            "store8",
            "store_i64",
        ] {
            assert_eq!(
                intrinsic_contract(name)
                    .expect("non-I/O contract")
                    .offset_policy,
                IntrinsicOffsetPolicy::Any
            );
        }
        assert!(intrinsic_supports_profile(
            "load_i64",
            IntrinsicProfile::RustMlir
        ));
        assert!(intrinsic_supports_profile(
            "load_i64",
            IntrinsicProfile::FrozenNative
        ));
        assert!(intrinsic_supports_profile(
            "argc",
            IntrinsicProfile::FrozenNative
        ));
        assert!(!intrinsic_supports_profile(
            "argc",
            IntrinsicProfile::RustMlir
        ));
        #[cfg(feature = "std-surface")]
        {
            assert_eq!(std_surface_intrinsic_arity("__mind_argc"), None);
            assert_eq!(std_surface_intrinsic_arity("__mind_load_i8"), Some(1));
        }

        let read = intrinsic_contract("read").expect("read contract");
        assert_eq!(read.parameters.len(), 4);
        assert_eq!(read.result, IntrinsicResult::I64);
        assert_eq!(read.effect, IntrinsicEffect::FdRead);
        assert_eq!(
            read.offset_policy,
            IntrinsicOffsetPolicy::MustBeMinusOneIgnored
        );
        for name in ["store_i64", "store8"] {
            let contract = intrinsic_contract(name).expect("store contract");
            assert_eq!(contract.result, IntrinsicResult::DiscardOnly);
            assert!(matches!(
                contract.effect,
                IntrinsicEffect::MemoryWrite { .. }
            ));
        }
    }
}
