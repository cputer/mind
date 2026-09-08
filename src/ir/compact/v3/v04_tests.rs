// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use super::{
    Determinism, MIC3_VERSION_V04, Mic3EncodeError, SignatureStatus, emit_mic3, emit_mic3_checked,
    emit_mic3_with_evidence_checked, emit_mic3_with_signed_evidence_checked, mic3_canonical_check,
    mic3_evidence_report, mic3_signature_status, parse_mic3_body, parse_mic3_prefix,
};
use crate::ir::evidence::ir_trace_hash_checked;
use crate::ir::{IRModule, Instr, ValueId};
use crate::types::{
    CanonicalModuleTypes, FieldDraft, FunctionDeclaration, FunctionIdentity, FunctionKind,
    FunctionSemanticTypes, FunctionSignature, ScalarType, SchemaDraft, SchemaIdentity,
    SchemaRegistryBuilder, SemanticType, TypeExpr,
};

fn i64_type() -> SemanticType {
    SemanticType::Scalar(ScalarType::I64)
}

fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex"), 16).unwrap())
        .collect()
}

fn declaration(identity: FunctionIdentity, kind: FunctionKind) -> FunctionDeclaration {
    FunctionDeclaration::new(
        identity,
        kind,
        FunctionSignature::new(vec![i64_type()], Some(i64_type())),
    )
}

fn semantic(identity: FunctionIdentity, values: &[usize]) -> Box<FunctionSemanticTypes> {
    let mut semantic = FunctionSemanticTypes::new(identity);
    for value in values {
        semantic
            .set_value_type(ValueId(*value), i64_type())
            .expect("unique function type");
    }
    Box::new(semantic)
}

fn scoped_module(omit_step_value: bool) -> IRModule {
    let mut schemas = SchemaRegistryBuilder::default();
    for owner in ["ownerA", "ownerB"] {
        schemas
            .add_schema(SchemaDraft::new(
                SchemaIdentity::new(owner, "Pair"),
                vec![
                    FieldDraft::new("z", TypeExpr::Scalar(ScalarType::I64)),
                    FieldDraft::new("a", TypeExpr::Scalar(ScalarType::Bool)),
                ],
            ))
            .expect("schema");
    }
    let mut bundle = CanonicalModuleTypes::new(schemas.finish().expect("registry"));
    let main = FunctionIdentity::new("entry", "main");
    let local_step = FunctionIdentity::new("ownerA", "step");
    let external_step = FunctionIdentity::new("ownerB", "step");
    for (identity, kind) in [
        (main.clone(), FunctionKind::Local),
        (local_step.clone(), FunctionKind::Local),
        (external_step.clone(), FunctionKind::External),
    ] {
        bundle
            .add_declaration(declaration(identity, kind))
            .expect("declaration");
    }
    bundle
        .set_module_value_type(ValueId(0), i64_type())
        .expect("module %0");

    let mut module = IRModule::new();
    let module_zero = module.fresh();
    assert_eq!(module_zero, ValueId(0));
    module.instrs.push(Instr::ConstI64(module_zero, 9));
    module.instrs.push(Instr::Output(module_zero));
    module.instrs.push(Instr::FnDef {
        name: "step".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(0)),
        body: vec![Instr::Return {
            value: Some(ValueId(0)),
        }],
        reap_threshold: None,
        semantic_types: Some(semantic(
            local_step,
            if omit_step_value { &[] } else { &[0] },
        )),
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
    });
    module.instrs.push(Instr::FnDef {
        name: "main".to_string(),
        params: vec![("x".to_string(), ValueId(0))],
        ret_id: Some(ValueId(1)),
        body: vec![
            Instr::Call {
                dst: ValueId(1),
                name: "step".to_string(),
                args: vec![ValueId(0)],
                resolved_callee: Some(Box::new(external_step)),
            },
            Instr::Return {
                value: Some(ValueId(1)),
            },
        ],
        reap_threshold: None,
        semantic_types: Some(semantic(main, &[0, 1])),
        #[cfg(feature = "std-surface")]
        value_types: std::collections::BTreeMap::new(),
    });
    module.canonical_types = Some(Box::new(bundle));
    module
}

fn module_value_only() -> IRModule {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    bundle
        .set_module_value_type(ValueId(0), i64_type())
        .expect("module type");
    let mut module = IRModule::new();
    let value = module.fresh();
    module.instrs.push(Instr::ConstI64(value, 42));
    module.canonical_types = Some(Box::new(bundle));
    module
}

fn descriptor_only_module(reverse: bool) -> IRModule {
    let left = SchemaDraft::new(
        SchemaIdentity::new("ownerA", "Node"),
        vec![
            FieldDraft::new(
                "z",
                TypeExpr::DynamicArray {
                    element: Box::new(TypeExpr::RecordRef(SchemaIdentity::new("ownerB", "Node"))),
                },
            ),
            FieldDraft::new("a", TypeExpr::Scalar(ScalarType::Bool)),
        ],
    );
    let right = SchemaDraft::new(
        SchemaIdentity::new("ownerB", "Node"),
        vec![FieldDraft::new(
            "back",
            TypeExpr::FixedArray {
                extent: 2,
                element: Box::new(TypeExpr::RecordRef(SchemaIdentity::new("ownerA", "Node"))),
            },
        )],
    );
    let mut registry = SchemaRegistryBuilder::default();
    let drafts = if reverse {
        [right, left]
    } else {
        [left, right]
    };
    for draft in drafts {
        registry.add_schema(draft).expect("schema draft");
    }
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(CanonicalModuleTypes::new(
        registry.finish().expect("cyclic registry"),
    )));
    module
}

#[test]
fn v04_roundtrip_preserves_owner_signature_call_and_three_value_zero_scopes() {
    let module = scoped_module(false);
    let bytes = emit_mic3_checked(&module).expect("v0x04 emission");
    assert_eq!(bytes[4], MIC3_VERSION_V04);
    let parsed = parse_mic3_body(&bytes).expect("v0x04 parse");
    assert_eq!(
        emit_mic3_checked(&parsed).expect("v0x04 re-emission"),
        bytes
    );

    let bundle = parsed.canonical_types.as_ref().expect("canonical bundle");
    assert!(bundle.module_values().contains_key(&ValueId(0)));
    assert_eq!(bundle.schema_registry().schemas().len(), 2);
    let mut function_zero_scopes = 0;
    let mut resolved_owner = None;
    for instruction in &parsed.instrs {
        if let Instr::FnDef {
            semantic_types,
            body,
            ..
        } = instruction
        {
            if semantic_types
                .as_ref()
                .is_some_and(|types| types.values().contains_key(&ValueId(0)))
            {
                function_zero_scopes += 1;
            }
            for nested in body {
                if let Instr::Call {
                    resolved_callee: Some(identity),
                    ..
                } = nested
                {
                    resolved_owner = Some(identity.owner().to_string());
                }
            }
        }
    }
    assert_eq!(function_zero_scopes, 2);
    assert_eq!(resolved_owner.as_deref(), Some("ownerB"));
}

#[test]
fn v04_checked_evidence_and_signature_consumers_validate_complete_artifacts() {
    let module = module_value_only();
    let body = emit_mic3_checked(&module).expect("v0x04 body");
    assert_eq!(body[4], MIC3_VERSION_V04);

    let evidence = emit_mic3_with_evidence_checked(
        &module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-test",
    )
    .expect("v0x04 evidence");
    let report = mic3_evidence_report(&evidence).expect("v0x04 evidence report");
    assert!(report.trace_hash_valid, "v0x04 trace hash must validate");
    mic3_canonical_check(&evidence).expect("v0x04 evidence must be canonical");

    let signed = emit_mic3_with_signed_evidence_checked(
        &module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-test",
        &[7; 32],
    )
    .expect("signed v0x04 evidence");
    let signed_report = mic3_evidence_report(&signed).expect("signed v0x04 report");
    assert!(
        signed_report.trace_hash_valid,
        "signed v0x04 trace hash must validate"
    );
    assert!(
        matches!(
            mic3_signature_status(&signed),
            Ok(SignatureStatus::Valid(_))
        ),
        "signed v0x04 artifact must verify"
    );
    mic3_canonical_check(&signed).expect("signed v0x04 evidence must be canonical");
}

#[test]
fn missing_type_in_one_function_refuses_while_other_zero_scopes_remain_typed() {
    let module = scoped_module(true);
    let bundle = module.canonical_types.as_ref().expect("bundle");
    assert!(bundle.module_values().contains_key(&ValueId(0)));
    assert!(module.instrs.iter().any(|instruction| matches!(
        instruction,
        Instr::FnDef {
            name,
            semantic_types: Some(types),
            ..
        } if name == "main" && types.values().contains_key(&ValueId(0))
    )));
    let error = emit_mic3_checked(&module).expect_err("step %0 has no same-scope type");
    assert!(
        matches!(&error, Mic3EncodeError::InvalidCanonicalMetadata(_)),
        "{error}"
    );
    assert!(
        error.to_string().contains("parameter %0") && error.to_string().contains("ownerA::step"),
        "{error}"
    );
}

#[test]
fn incomplete_v04_metadata_refuses_through_checked_consumers() {
    let module = scoped_module(true);
    assert!(matches!(
        ir_trace_hash_checked(&module),
        Err(Mic3EncodeError::InvalidCanonicalMetadata(_))
    ));
    assert!(matches!(
        emit_mic3_with_evidence_checked(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "v04-test",
        ),
        Err(crate::ir::compact::v3::EvidenceEmitError::Body(
            Mic3EncodeError::InvalidCanonicalMetadata(_)
        ))
    ));
    assert!(matches!(
        emit_mic3_with_signed_evidence_checked(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "v04-test",
            &[7; 32],
        ),
        Err(crate::ir::compact::v3::EvidenceEmitError::Body(
            Mic3EncodeError::InvalidCanonicalMetadata(_)
        ))
    ));
}

#[test]
fn v04_rejects_nonminimal_counts_surface_claims_and_bad_descriptors() {
    let bytes = emit_mic3_checked(&module_value_only()).expect("v0x04 body");

    let mut nonminimal = bytes.clone();
    nonminimal.splice(5..6, [0x80, 0x00]);
    assert!(
        parse_mic3_prefix(&nonminimal)
            .expect_err("nonminimal ULEB")
            .message
            .contains("non-minimal")
    );

    let mut unsupported_surface = bytes.clone();
    unsupported_surface[5] = 1;
    assert!(
        parse_mic3_prefix(&unsupported_surface)
            .expect_err("unsupported surface")
            .message
            .contains("unsupported")
    );

    let mut count_bomb = bytes.clone();
    count_bomb[7] = 0x7f;
    assert!(
        parse_mic3_prefix(&count_bomb)
            .expect_err("count beyond remaining bytes")
            .message
            .contains("remaining")
    );

    let mut unknown_descriptor = bytes.clone();
    let descriptor_tag = unknown_descriptor.len() - 2;
    unknown_descriptor[descriptor_tag] = 0xff;
    assert!(
        parse_mic3_prefix(&unknown_descriptor)
            .expect_err("unknown descriptor")
            .message
            .contains("semantic type tag")
    );

    parse_mic3_body(&bytes).expect("positive control after refusals");
}

#[test]
fn module_value_zero_const_i64_has_a_fixed_draft_golden() {
    let bytes = emit_mic3_checked(&module_value_only()).expect("v0x04 body");
    assert_eq!(
        bytes,
        decode_hex("4d49433304000000000100010100540000000001000001")
    );
    let parsed = parse_mic3_body(&bytes).expect("golden parses");
    assert!(matches!(
        parsed.instrs.as_slice(),
        [Instr::ConstI64(ValueId(0), 42)]
    ));
}

#[test]
fn every_truncated_canonical_body_is_refused() {
    for module in [
        module_value_only(),
        scoped_module(false),
        descriptor_only_module(false),
    ] {
        let bytes = emit_mic3_checked(&module).expect("complete canonical fixture");
        parse_mic3_body(&bytes).expect("complete fixture must parse");
        for cut in 0..bytes.len() {
            assert!(
                parse_mic3_body(&bytes[..cut]).is_err(),
                "truncated canonical body accepted at {cut}/{}",
                bytes.len()
            );
        }
        parse_mic3_body(&bytes).expect("refusals must not poison the next decode");
    }
}

#[test]
fn single_bit_wire_mutations_refuse_or_roundtrip_exactly() {
    let mut mutations = 0;
    let mut refused = 0;
    for module in [
        module_value_only(),
        scoped_module(false),
        descriptor_only_module(false),
    ] {
        let bytes = emit_mic3_checked(&module).expect("complete canonical fixture");
        for offset in 0..bytes.len() {
            for bit in 0..8 {
                let mut changed = bytes.clone();
                changed[offset] ^= 1 << bit;
                mutations += 1;
                match parse_mic3_body(&changed) {
                    Ok(parsed) => assert_eq!(
                        emit_mic3_checked(&parsed).expect("admitted body must remain encodable"),
                        changed,
                        "accepted mutation changed bytes at offset {offset}, bit {bit}"
                    ),
                    Err(_) => refused += 1,
                }
            }
        }
        parse_mic3_body(&bytes).expect("mutations must not poison the next decode");
    }
    assert!(mutations > 128, "the mutation corpus must execute");
    assert!(
        refused > 0,
        "malformed wire variants must actually be refused"
    );
}

#[test]
fn checked_value_bounds_refuse_before_emission_and_decode_without_poisoning() {
    let mut below_bound = module_value_only();
    below_bound.next_id = 0;
    assert!(matches!(
        emit_mic3_checked(&below_bound),
        Err(Mic3EncodeError::InvalidCanonicalMetadata(message))
            if message.contains("next_id")
    ));

    let mut max_id = module_value_only();
    max_id.next_id = usize::MAX;
    max_id.instrs[0] = Instr::ConstI64(ValueId(usize::MAX), 42);
    max_id
        .canonical_types
        .as_mut()
        .expect("bundle")
        .set_module_value_type(ValueId(usize::MAX), i64_type())
        .expect("distinct max row");
    assert!(matches!(
        emit_mic3_checked(&max_id),
        Err(Mic3EncodeError::InvalidCanonicalMetadata(message))
            if message.contains("usize::MAX")
    ));

    let bytes = emit_mic3_checked(&module_value_only()).expect("positive body");
    let mut malformed_bound = bytes.clone();
    malformed_bound[9] = 0;
    assert!(
        parse_mic3_prefix(&malformed_bound)
            .expect_err("next_id below ValueId bound")
            .message
            .contains("next_id")
    );
    let mut hostile = bytes.clone();
    hostile.splice(13..14, [0xff; 9].into_iter().chain([0x01]));
    assert!(
        parse_mic3_prefix(&hostile)
            .expect_err("maximum ValueId")
            .message
            .contains("exclusive bound")
    );
    parse_mic3_body(&bytes).expect("positive control after bound refusals");
}

#[test]
fn encoder_depth_preflight_precedes_recursive_metadata_validation() {
    let mut module = module_value_only();
    let mut nested = vec![Instr::ConstI64(ValueId(0), 0)];
    for index in 0..=256 {
        nested = vec![Instr::FnDef {
            name: format!("nested{index}"),
            params: Vec::new(),
            ret_id: None,
            body: nested,
            reap_threshold: None,
            semantic_types: None,
            #[cfg(feature = "std-surface")]
            value_types: std::collections::BTreeMap::new(),
        }];
    }
    module.instrs.extend(nested);
    assert!(matches!(
        emit_mic3_checked(&module),
        Err(Mic3EncodeError::InvalidCanonicalMetadata(message))
            if message.contains("instruction nesting")
    ));
    parse_mic3_body(&emit_mic3_checked(&module_value_only()).expect("small body"))
        .expect("positive after depth refusal");
}

#[test]
fn encoder_refuses_a_body_that_its_fixed_decode_budget_cannot_admit() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let mut module = IRModule::new();
    for index in 0..2_500 {
        let value = module.fresh();
        assert_eq!(value, ValueId(index));
        module.instrs.push(Instr::ConstI64(value, 0));
        bundle
            .set_module_value_type(value, i64_type())
            .expect("unique scalar row");
    }
    module.canonical_types = Some(Box::new(bundle));
    assert!(matches!(
        emit_mic3_checked(&module),
        Err(Mic3EncodeError::V04ResourceLimit(message))
            if message.contains("decoded allocation cost")
    ));
    parse_mic3_body(&emit_mic3_checked(&module_value_only()).expect("small body"))
        .expect("small positive after refusal");
}

#[test]
fn nested_record_descriptors_cycles_and_insertion_order_are_canonical() {
    let forward = emit_mic3_checked(&descriptor_only_module(false)).expect("forward registry");
    let reverse = emit_mic3_checked(&descriptor_only_module(true)).expect("reverse registry");
    assert_eq!(forward, reverse);
    let parsed = parse_mic3_body(&forward).expect("nested descriptor registry parses");
    let schemas = parsed
        .canonical_types
        .as_ref()
        .expect("bundle")
        .schema_registry()
        .schemas();
    assert_eq!(schemas[0].fields()[0].name(), "z");
    assert_eq!(schemas[0].fields()[1].name(), "a");
    assert_eq!(emit_mic3_checked(&parsed).expect("fixed point"), forward);
}

#[test]
fn empty_optional_bundle_still_uses_unchanged_legacy_bytes() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(CanonicalModuleTypes::new(registry)));
    let checked = emit_mic3_checked(&module).expect("legacy emission");
    assert_ne!(checked[4], MIC3_VERSION_V04);
    assert_eq!(checked, emit_mic3(&module));
}

#[cfg(feature = "std-surface")]
#[test]
fn core_v04_encoder_refuses_std_registry_and_binop_operand_surface() {
    let mut registry_module = module_value_only();
    registry_module
        .const_array_defs
        .insert("TABLE".to_string(), vec![1, 2]);
    assert!(matches!(
        emit_mic3_checked(&registry_module),
        Err(Mic3EncodeError::UnsupportedV04Surface(message))
            if message.contains("required_surface_bits")
    ));

    let mut operand_module = module_value_only();
    operand_module.next_id = 3;
    let rhs = operand_module.fresh();
    let dst = operand_module.fresh();
    operand_module.instrs.extend([
        Instr::ConstI64(rhs, 1),
        Instr::BinOp {
            dst,
            op: crate::ir::BinOp::BitAnd,
            lhs: ValueId(0),
            rhs,
        },
    ]);
    assert!(matches!(
        emit_mic3_checked(&operand_module),
        Err(Mic3EncodeError::UnsupportedV04Surface(message))
            if message.contains("required_surface_bits")
    ));
}
