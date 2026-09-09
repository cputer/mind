// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use super::{
    Determinism, EvidenceEmitError, MAX_MIC3_INPUT, MIC3_VERSION_V04, Mic3EncodeError, emit_mic3,
    emit_mic3_checked, emit_mic3_with_evidence_and_receipts, emit_mic3_with_evidence_checked,
    emit_mic3_with_signed_evidence_checked, mic3_canonical_check, mic3_evidence_report,
    parse_mic3_body, parse_mic3_prefix,
};
#[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
use super::{SignatureStatus, mic3_signature_status};
use crate::ir::evidence::ir_trace_hash_checked;
use crate::ir::{IRModule, Instr, ValueId};
use crate::types::{
    CanonicalModuleTypes, FieldDraft, FunctionDeclaration, FunctionIdentity, FunctionKind,
    FunctionSemanticTypes, FunctionSignature, ScalarType, SchemaDraft, SchemaIdentity,
    SchemaRegistryBuilder, SemanticType, TypeExpr,
};

#[path = "v04_test_support.rs"]
mod support;
use support::*;

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

    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    let signed = super::emit_mic3_with_signed_evidence_scheme(
        &module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-test",
        &super::evidence::SigningKey::PqcHybrid {
            mldsa87: [7; 32],
            slhdsa: [8; 96],
        },
    )
    .expect("signed v0x04 evidence");
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    let signed = {
        let err = super::emit_mic3_with_signed_evidence_checked(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "v04-test",
            &[7; 32],
        )
        .expect_err("retired Ed signing must refuse");
        assert!(format!("{err:?}").contains("SchemeRetired"));
        evidence.clone()
    };
    let signed_report = mic3_evidence_report(&signed).expect("signed v0x04 report");
    assert!(
        signed_report.trace_hash_valid,
        "signed v0x04 trace hash must validate"
    );
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
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
fn checked_evidence_caps_complete_artifact_before_map_append() {
    let small_body = emit_mic3_checked(&module_value_only()).expect("small v0x04 body");
    let small_unsigned = emit_mic3_with_evidence_checked(
        &module_value_only(),
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
    )
    .expect("small unsigned evidence");
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    let small_signed = super::emit_mic3_with_signed_evidence_scheme(
        &module_value_only(),
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
        &super::evidence::SigningKey::PqcHybrid {
            mldsa87: [7; 32],
            slhdsa: [8; 96],
        },
    )
    .expect("small signed evidence");
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    let small_signed = small_unsigned.clone();
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    let unsigned_map_len = small_unsigned.len() - small_body.len();
    let signed_map_len = small_signed.len() - small_body.len();

    // Leave a small, measured margin for the signed MAP: this produces a real
    // near-limit artifact whose complete bytes still pass every reader.
    let positive_limit = MAX_MIC3_INPUT - signed_map_len - 128;
    let (near_module, near_body) = largest_body_at_most(positive_limit);
    assert!(near_body.len() > MAX_MIC3_INPUT - signed_map_len - 256);
    let near_unsigned = emit_mic3_with_evidence_checked(
        &near_module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
    )
    .expect("near-limit unsigned evidence");
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    let near_signed = super::emit_mic3_with_signed_evidence_scheme(
        &near_module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
        &super::evidence::SigningKey::PqcHybrid {
            mldsa87: [7; 32],
            slhdsa: [8; 96],
        },
    )
    .expect("near-limit signed evidence");
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    let near_signed = near_unsigned.clone();
    assert!(near_unsigned.len() <= MAX_MIC3_INPUT);
    assert!(near_signed.len() <= MAX_MIC3_INPUT);
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    {
        assert!(near_signed.len() > MAX_MIC3_INPUT - signed_map_len - 256);
        assert!(
            mic3_evidence_report(&near_signed)
                .expect("near-limit report")
                .trace_hash_valid
        );
        assert!(matches!(
            mic3_signature_status(&near_signed),
            Ok(SignatureStatus::Valid(_))
        ));
    }
    mic3_canonical_check(&near_unsigned).expect("near-limit unsigned canonical");
    mic3_canonical_check(&near_signed).expect("near-limit signed canonical");

    // Keep the body itself admitted, then make only the envelope too large. Both
    // public checked emitters must refuse before returning a partially appended
    // artifact; legacy wrappers retain their existing infallible contract.
    let (oversized_module, oversized_body) = largest_body_at_most(MAX_MIC3_INPUT - 1);
    assert!(oversized_body.len() < MAX_MIC3_INPUT);
    let unsigned_error = emit_mic3_with_evidence_checked(
        &oversized_module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
    )
    .expect_err("oversized unsigned envelope must refuse");
    assert!(matches!(
        unsigned_error,
        EvidenceEmitError::ArtifactTooLarge { .. }
    ));
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    {
        let signed_error = super::emit_mic3_with_signed_evidence_scheme(
            &oversized_module,
            "cpu",
            None,
            Determinism::Deterministic,
            "v04-cap-test",
            &super::evidence::SigningKey::PqcHybrid {
                mldsa87: [7; 32],
                slhdsa: [8; 96],
            },
        )
        .expect_err("oversized signed envelope must refuse");
        assert!(matches!(
            signed_error,
            EvidenceEmitError::ArtifactTooLarge { .. }
        ));
    }
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    {
        let retired_error = super::emit_mic3_with_signed_evidence_checked(
            &oversized_module,
            "cpu",
            None,
            Determinism::Deterministic,
            "v04-cap-test",
            &[7; 32],
        )
        .expect_err("retired Ed signing must refuse");
        assert!(format!("{retired_error:?}").contains("SchemeRetired"));
    }
    let receipts_error = emit_mic3_with_evidence_and_receipts(
        &oversized_module,
        "cpu",
        None,
        Determinism::Deterministic,
        "v04-cap-test",
        None,
        &[],
        &[],
    )
    .expect_err("oversized receipt envelope must refuse");
    assert!(matches!(
        receipts_error,
        EvidenceEmitError::ArtifactTooLarge { .. }
    ));
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    assert!(unsigned_map_len < signed_map_len);
}

#[test]
fn evidence_size_accounting_refuses_usize_overflow() {
    assert_eq!(
        super::evidence_size::checked_len_add(usize::MAX, 1),
        Err(EvidenceEmitError::ArtifactSizeOverflow)
    );
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
fn decoded_parts_enforce_one_cumulative_descriptor_budget() {
    fn fixed_product(inner_extent: u64) -> SemanticType {
        SemanticType::FixedArray {
            extent: 1 << 20,
            element: Box::new(SemanticType::FixedArray {
                extent: inner_extent,
                element: Box::new(i64_type()),
            }),
        }
    }

    // One function signature and two module rows each contribute 2^38
    // elements. Their combined 3 * 2^38 cost is below the 2^40 carrier limit.
    let accepted_type = fixed_product(1 << 18);
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let external = FunctionIdentity::new("dep", "wide");
    bundle
        .add_declaration(FunctionDeclaration::new(
            external,
            FunctionKind::External,
            FunctionSignature::new(vec![accepted_type.clone()], None),
        ))
        .expect("signature below cumulative limit");
    let mut module = IRModule::new();
    for constant in [7, 9] {
        let value = module.fresh();
        module.instrs.push(Instr::ConstI64(value, constant));
        bundle
            .set_module_value_type(value, accepted_type.clone())
            .expect("row below cumulative limit");
    }
    module.canonical_types = Some(Box::new(bundle));
    let accepted = emit_mic3_checked(&module).expect("accepted cumulative body");
    parse_mic3_body(&accepted).expect("staged parts at 3 * 2^38 must decode");

    // Raise only the three inner extents from 2^18 to 2^19. Each descriptor
    // remains individually legal, while their combined 3 * 2^39 exceeds 2^40.
    let encoded_accepted_type = [2, 0x80, 0x80, 0x40, 2, 0x80, 0x80, 0x10, 0, 1];
    let matches: Vec<usize> = accepted
        .windows(encoded_accepted_type.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == encoded_accepted_type).then_some(offset))
        .collect();
    assert_eq!(matches.len(), 3, "one signature plus two module rows");
    let mut excessive = accepted.clone();
    for offset in matches {
        excessive[offset + 7] = 0x20;
    }
    let error = parse_mic3_body(&excessive).expect_err("combined descriptor budget must fail");
    assert!(error.message.contains("fixed-array element count exceeds"));
    parse_mic3_body(&accepted).expect("positive decode after cumulative refusal");
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
