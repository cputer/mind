// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use crate::ir::canonical_verify::instruction_metadata_present;
use crate::ir::{CanonicalMetadataError, IRModule, Instr, ValueId, verify_canonical_metadata};
use crate::types::{
    CanonicalModuleTypes, FunctionDeclaration, FunctionIdentity, FunctionKind,
    FunctionSemanticTypes, FunctionSignature, RegistryLimits, ScalarType, SchemaError,
    SchemaRegistryBuilder, SemanticType,
};

#[cfg(feature = "std-surface")]
use crate::types::DType;

#[test]
fn function_metadata_is_checked_in_its_local_value_scope() {
    let identity = FunctionIdentity::new("module", "id");
    let mut bundle =
        CanonicalModuleTypes::new(SchemaRegistryBuilder::default().finish().expect("registry"));
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(
                vec![SemanticType::Scalar(ScalarType::I64)],
                Some(SemanticType::Scalar(ScalarType::I64)),
            ),
        ))
        .expect("function declaration");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module.instrs.push(Instr::FnDef {
        name: "id".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(0)),
        body: vec![
            Instr::Param {
                dst: ValueId(0),
                name: "x".to_string(),
                index: 0,
            },
            Instr::Return {
                value: Some(ValueId(0)),
            },
        ],
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: Some(Box::new({
            let mut types = FunctionSemanticTypes::new(identity);
            types
                .set_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
                .expect("parameter type");
            types
        })),
    });
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
    let before = module.instrs.clone();
    assert!(matches!(
        crate::opt::ir_canonical::canonicalize_module_checked(&mut module),
        Err(CanonicalMetadataError::OptimizationUnsupported)
    ));
    crate::opt::native_opt::optimize_mic3(&mut module, crate::opt::native_opt::OptLevel::Basic);
    assert_eq!(format!("{:?}", module.instrs), format!("{:?}", before));
}

#[test]
fn user_call_without_resolved_identity_is_rejected() {
    let identity = FunctionIdentity::new("module", "caller");
    let mut bundle =
        CanonicalModuleTypes::new(SchemaRegistryBuilder::default().finish().expect("registry"));
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(
                vec![SemanticType::Scalar(ScalarType::I64)],
                Some(SemanticType::Scalar(ScalarType::I64)),
            ),
        ))
        .expect("function declaration");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module.instrs.push(Instr::FnDef {
        name: "caller".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(0)),
        body: vec![
            Instr::Param {
                dst: ValueId(0),
                name: "x".to_string(),
                index: 0,
            },
            Instr::Call {
                dst: ValueId(1),
                name: "__mind_fake".to_string(),
                args: vec![ValueId(0)],
                resolved_callee: None,
            },
        ],
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: Some(Box::new({
            let mut types = FunctionSemanticTypes::new(identity);
            types
                .set_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
                .expect("parameter type");
            types
                .set_value_type(ValueId(1), SemanticType::Scalar(ScalarType::I64))
                .expect("call result type");
            types
        })),
    });
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::UnresolvedTypedCall { .. })
    ));
}

#[test]
fn canonical_authority_requires_every_function_body_metadata() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
        .expect("module authority");
    let mut module = IRModule::new();
    module.instrs.push(Instr::ConstI64(ValueId(0), 1));
    module.instrs.push(Instr::FnDef {
        name: "missing".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: Vec::new(),
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: None,
    });
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::MissingFunctionMetadata { name }) if name == "missing"
    ));
}

#[test]
fn local_declaration_without_a_body_is_rejected() {
    let identity = FunctionIdentity::new("module", "missing_body");
    let mut bundle =
        CanonicalModuleTypes::new(SchemaRegistryBuilder::default().finish().expect("registry"));
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity,
            FunctionKind::Local,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("declaration");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::FunctionNotDefined { identity })
            if identity == "module::missing_body"
    ));
}

#[test]
fn external_declaration_cannot_gain_a_local_body() {
    let identity = FunctionIdentity::new("module", "external");
    let mut bundle =
        CanonicalModuleTypes::new(SchemaRegistryBuilder::default().finish().expect("registry"));
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::External,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("declaration");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module.instrs.push(Instr::FnDef {
        name: "external".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: Vec::new(),
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: Some(Box::new(FunctionSemanticTypes::new(identity))),
    });
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::NonLocalFunctionBody {
            kind: FunctionKind::External,
            ..
        })
    ));
}

#[test]
fn canonical_authority_requires_module_call_resolution() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
        .expect("module authority");
    let mut module = IRModule::new();
    module.instrs.push(Instr::ConstI64(ValueId(0), 1));
    module.instrs.push(Instr::legacy_call(
        ValueId(1),
        "user_call".to_string(),
        vec![ValueId(0)],
    ));
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::UnresolvedTypedCall { name }) if name == "user_call"
    ));
}

#[test]
fn nested_orphan_metadata_is_seen_by_the_presence_guard() {
    let nested = Instr::FnDef {
        name: "outer".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: vec![Instr::Call {
            dst: ValueId(0),
            name: "nested_call".to_string(),
            args: Vec::new(),
            resolved_callee: Some(Box::new(FunctionIdentity::new("module", "nested_call"))),
        }],
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: None,
    };
    assert!(instruction_metadata_present(std::slice::from_ref(&nested)));
    let mut module = IRModule::new();
    module.instrs.push(nested);
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::InvalidBundle(message))
            if message.contains("no canonical module bundle")
    ));
}

#[test]
fn metadata_presence_scan_handles_flat_and_deep_streams() {
    let flat = [Instr::ConstI64(ValueId(0), 1)];
    assert!(!instruction_metadata_present(&flat));

    let mut nested = Instr::Call {
        dst: ValueId(0),
        name: "deep_call".to_string(),
        args: Vec::new(),
        resolved_callee: Some(Box::new(FunctionIdentity::new("module", "deep_call"))),
    };
    for _ in 0..1_024 {
        nested = Instr::FnDef {
            name: "nested".to_string(),
            params: Vec::new(),
            ret_id: None,
            body: vec![nested],
            reap_threshold: None,
            #[cfg(feature = "std-surface")]
            value_types: std::collections::BTreeMap::new(),
            semantic_types: None,
        };
    }
    assert!(instruction_metadata_present(std::slice::from_ref(&nested)));
}

#[test]
fn intrinsic_authority_is_explicit_and_reserved() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let bad = FunctionDeclaration::new(
        FunctionIdentity::new("pkg", "intrinsic"),
        FunctionKind::Intrinsic,
        FunctionSignature::new(Vec::new(), None),
    );
    assert!(matches!(
        bundle.add_declaration(bad),
        Err(SchemaError::InvalidIntrinsicIdentity { .. })
    ));
}

#[test]
fn declared_intrinsic_identity_is_the_only_intrinsic_bypass() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let intrinsic = FunctionIdentity::new("__mind_intrinsic", "load_i64");
    bundle
        .add_declaration(FunctionDeclaration::new(
            intrinsic.clone(),
            FunctionKind::Intrinsic,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("reserved intrinsic declaration");
    let mut module = IRModule::new();
    module.instrs.push(Instr::Call {
        dst: ValueId(0),
        name: "load_i64".to_string(),
        args: Vec::new(),
        resolved_callee: Some(Box::new(intrinsic)),
    });
    module.canonical_types = Some(Box::new(bundle));
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
}

#[test]
fn populated_metadata_selects_the_checked_v04_boundary() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty schema registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
        .expect("canonical module value");
    let mut module = IRModule::new();
    module.next_id = 1;
    module.instrs.push(Instr::ConstI64(ValueId(0), 1));
    module.canonical_types = Some(Box::new(bundle));
    let result = crate::ir::compact::v3::emit_mic3_checked(&module);
    let bytes = result.expect("canonical metadata is encoded by v0x04");
    assert_eq!(bytes[4], crate::ir::compact::v3::MIC3_VERSION_V04);
    let parsed =
        crate::ir::compact::v3::parse_mic3_body(&bytes).expect("v0x04 metadata round-trip");
    assert_eq!(
        crate::ir::compact::v3::emit_mic3_checked(&parsed).expect("v0x04 re-emission"),
        bytes
    );
}

#[test]
fn empty_optional_bundle_keeps_the_legacy_mic_boundary() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty schema registry");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(CanonicalModuleTypes::new(registry)));
    let checked = crate::ir::compact::v3::emit_mic3_checked(&module).expect("legacy bytes");
    assert_eq!(checked, crate::ir::compact::v3::emit_mic3(&module));
}

#[test]
fn instruction_metadata_without_a_bundle_is_rejected() {
    let mut module = IRModule::new();
    module.instrs.push(Instr::Call {
        dst: ValueId(0),
        name: "callee".to_string(),
        args: Vec::new(),
        resolved_callee: Some(Box::new(FunctionIdentity::new("module", "callee"))),
    });
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::InvalidBundle(message))
            if message.contains("no canonical module bundle")
    ));
    assert!(matches!(
        crate::ir::compact::v3::emit_mic3_checked(&module),
        Err(crate::ir::compact::v3::Mic3EncodeError::InvalidCanonicalMetadata(message))
            if message.contains("no canonical module bundle")
    ));
}

#[test]
fn duplicate_function_rejection_preserves_the_registered_definition() {
    let identity = FunctionIdentity::new("module", "same_name");
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty schema registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let first = FunctionDeclaration::new(
        identity.clone(),
        FunctionKind::Local,
        FunctionSignature::new(
            vec![SemanticType::Scalar(ScalarType::I64)],
            Some(SemanticType::Scalar(ScalarType::I64)),
        ),
    );
    bundle.add_declaration(first).expect("first definition");
    let replacement = FunctionDeclaration::new(
        identity.clone(),
        FunctionKind::Local,
        FunctionSignature::new(
            vec![SemanticType::Scalar(ScalarType::F64)],
            Some(SemanticType::Scalar(ScalarType::F64)),
        ),
    );
    assert!(matches!(
        bundle.add_declaration(replacement),
        Err(SchemaError::DuplicateFunction { .. })
    ));
    assert_eq!(
        bundle
            .functions()
            .get(&identity)
            .expect("original definition")
            .signature()
            .params(),
        &[SemanticType::Scalar(ScalarType::I64)]
    );
}

#[test]
fn semantic_aggregate_limits_are_checked_before_insertion() {
    let limits = RegistryLimits {
        max_extent: 4,
        ..RegistryLimits::default()
    };
    let registry = SchemaRegistryBuilder::new(limits)
        .finish()
        .expect("empty schema registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    assert!(matches!(
        bundle.set_module_value_type(
            ValueId(0),
            SemanticType::FixedArray {
                element: Box::new(SemanticType::Scalar(ScalarType::I64)),
                extent: 5,
            },
        ),
        Err(SchemaError::ExtentLimitExceeded {
            extent: 5,
            limit: 4
        })
    ));
    assert!(bundle.module_values().is_empty());
}

#[test]
fn function_and_module_shapes_share_nested_limit_validation() {
    let fixed = |extent| SemanticType::FixedArray {
        element: Box::new(SemanticType::Scalar(ScalarType::I64)),
        extent,
    };
    let registry = SchemaRegistryBuilder::new(RegistryLimits {
        max_fixed_elements: 4,
        ..RegistryLimits::default()
    })
    .finish()
    .expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    assert!(matches!(
        bundle.set_module_value_type(
            ValueId(0),
            SemanticType::FixedArray {
                element: Box::new(fixed(5)),
                extent: 0,
            },
        ),
        Err(SchemaError::FixedElementLimitExceeded { limit: 4 })
    ));
    assert!(matches!(
        bundle.set_module_value_type(
            ValueId(1),
            SemanticType::DynamicArray {
                element: Box::new(fixed(4)),
            },
        ),
        Err(SchemaError::FixedElementLimitExceeded { limit: 4 })
    ));
    assert!(
        bundle
            .set_module_value_type(
                ValueId(2),
                SemanticType::DynamicArray {
                    element: Box::new(fixed(3)),
                },
            )
            .is_ok()
    );
    let declaration = FunctionDeclaration::new(
        FunctionIdentity::new("module", "bounded"),
        FunctionKind::External,
        FunctionSignature::new(
            vec![SemanticType::DynamicArray {
                element: Box::new(fixed(4)),
            }],
            None,
        ),
    );
    assert!(matches!(
        bundle.add_declaration(declaration),
        Err(SchemaError::InvalidFunctionType { .. })
    ));
}

#[test]
fn function_local_shape_is_checked_against_the_shared_registry() {
    let identity = FunctionIdentity::new("module", "bad_shape");
    let registry = SchemaRegistryBuilder::new(RegistryLimits {
        max_fixed_elements: 4,
        ..RegistryLimits::default()
    })
    .finish()
    .expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("declaration");
    let mut semantic = FunctionSemanticTypes::new(identity);
    semantic
        .set_value_type(
            ValueId(1),
            SemanticType::FixedArray {
                element: Box::new(SemanticType::FixedArray {
                    element: Box::new(SemanticType::Scalar(ScalarType::I64)),
                    extent: 5,
                }),
                extent: 0,
            },
        )
        .expect("local metadata draft");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module.instrs.push(Instr::FnDef {
        name: "bad_shape".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: vec![Instr::ConstI64(ValueId(1), 1)],
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: Some(Box::new(semantic)),
    });
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::InvalidFunctionValueType { function, value })
            if function == "module::bad_shape" && value == ValueId(1)
    ));
}

#[test]
fn function_local_exact_nested_limit_is_accepted() {
    let identity = FunctionIdentity::new("module", "good_shape");
    let registry = SchemaRegistryBuilder::new(RegistryLimits {
        max_fixed_elements: 4,
        ..RegistryLimits::default()
    })
    .finish()
    .expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("declaration");
    let mut semantic = FunctionSemanticTypes::new(identity);
    semantic
        .set_value_type(
            ValueId(0),
            SemanticType::DynamicArray {
                element: Box::new(SemanticType::FixedArray {
                    element: Box::new(SemanticType::Scalar(ScalarType::I64)),
                    extent: 3,
                }),
            },
        )
        .expect("local metadata draft");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module.instrs.push(Instr::FnDef {
        name: "good_shape".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: vec![Instr::ConstI64(ValueId(0), 1)],
        reap_threshold: None,
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
        semantic_types: Some(Box::new(semantic)),
    });
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
}

#[cfg(feature = "std-surface")]
#[test]
fn canonical_and_legacy_aggregate_tables_cannot_disagree() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty schema registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
        .expect("canonical value type");
    let mut module = IRModule::new();
    module.instrs.push(Instr::ConstI64(ValueId(0), 1));
    module.value_types.insert(
        ValueId(0),
        crate::ir::ArrayType {
            elem_dtype: DType::I64,
            size: crate::ir::ArraySize::Fixed(1),
        },
    );
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::LegacyTypeConflict {
            function,
            value: ValueId(0)
        }) if function == "<module>"
    ));
}

#[cfg(feature = "std-surface")]
fn legacy_i64_array() -> crate::ir::ArrayType {
    crate::ir::ArrayType {
        elem_dtype: DType::I64,
        size: crate::ir::ArraySize::Fixed(1),
    }
}

#[cfg(feature = "std-surface")]
fn canonical_i64_array() -> SemanticType {
    SemanticType::FixedArray {
        element: Box::new(SemanticType::Scalar(ScalarType::I64)),
        extent: 1,
    }
}

#[cfg(feature = "std-surface")]
#[test]
fn populated_module_authority_requires_matching_legacy_array_rows() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), canonical_i64_array())
        .expect("canonical array");
    let mut module = IRModule::new();
    module.instrs.extend([Instr::ConstI64(ValueId(0), 1)]);
    module.value_types.insert(ValueId(0), legacy_i64_array());
    module.canonical_types = Some(Box::new(bundle));
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
}

#[cfg(feature = "std-surface")]
#[test]
fn populated_module_authority_rejects_an_unmatched_legacy_array_row() {
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(1), SemanticType::Scalar(ScalarType::I64))
        .expect("canonical scalar");
    let mut module = IRModule::new();
    module.instrs.extend([
        Instr::ConstI64(ValueId(0), 1),
        Instr::ConstI64(ValueId(1), 2),
    ]);
    module.value_types.insert(ValueId(0), legacy_i64_array());
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::MissingCanonicalLegacyType {
            function,
            value: ValueId(0)
        }) if function == "<module>"
    ));
}

#[cfg(feature = "std-surface")]
#[test]
fn populated_function_authority_checks_reused_value_zero() {
    let identity = FunctionIdentity::new("module", "array_fn");
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("function declaration");
    let mut semantic = FunctionSemanticTypes::new(identity);
    semantic
        .set_value_type(ValueId(0), canonical_i64_array())
        .expect("canonical array");
    let mut module = IRModule::new();
    module.instrs.push(Instr::FnDef {
        name: "array_fn".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: vec![Instr::ConstI64(ValueId(0), 1)],
        reap_threshold: None,
        value_types: [(ValueId(0), legacy_i64_array())].into_iter().collect(),
        semantic_types: Some(Box::new(semantic)),
    });
    module.canonical_types = Some(Box::new(bundle));
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
}

#[cfg(feature = "std-surface")]
#[test]
fn populated_function_authority_rejects_an_unmatched_legacy_array_row() {
    let identity = FunctionIdentity::new("module", "array_fn");
    let registry = SchemaRegistryBuilder::default().finish().expect("registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .add_declaration(FunctionDeclaration::new(
            identity.clone(),
            FunctionKind::Local,
            FunctionSignature::new(Vec::new(), None),
        ))
        .expect("function declaration");
    let mut semantic = FunctionSemanticTypes::new(identity);
    semantic
        .set_value_type(ValueId(1), SemanticType::Scalar(ScalarType::I64))
        .expect("canonical scalar");
    let mut module = IRModule::new();
    module.instrs.push(Instr::FnDef {
        name: "array_fn".to_string(),
        params: Vec::new(),
        ret_id: None,
        body: vec![
            Instr::ConstI64(ValueId(0), 1),
            Instr::ConstI64(ValueId(1), 2),
        ],
        reap_threshold: None,
        value_types: [(ValueId(0), legacy_i64_array())].into_iter().collect(),
        semantic_types: Some(Box::new(semantic)),
    });
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        verify_canonical_metadata(&module),
        Err(CanonicalMetadataError::MissingCanonicalLegacyType {
            function,
            value: ValueId(0)
        }) if function == "module::array_fn"
    ));
}

#[cfg(feature = "std-surface")]
#[test]
fn legacy_array_rows_remain_valid_without_canonical_authority() {
    let mut legacy = IRModule::new();
    legacy.instrs.push(Instr::ConstI64(ValueId(0), 1));
    legacy.value_types.insert(ValueId(0), legacy_i64_array());
    assert_eq!(verify_canonical_metadata(&legacy), Ok(()));

    let mut empty = legacy;
    empty.canonical_types = Some(Box::new(CanonicalModuleTypes::new(
        SchemaRegistryBuilder::default().finish().expect("registry"),
    )));
    assert_eq!(verify_canonical_metadata(&empty), Ok(()));
}
