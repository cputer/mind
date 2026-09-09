// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use super::evidence::MAP_SENTINEL;
use super::{
    Determinism, Mic3EnvelopeError, Mic3NonCanonical, emit_mic3, emit_mic3_checked,
    emit_mic3_with_evidence_and_receipts, emit_mic3_with_evidence_checked, mic3_canonical_check,
    parse_mic3, parse_mic3_body, parse_mic3_envelope, parse_mic3_prefix,
};
use crate::ir::evidence::{ir_trace_hash, ir_trace_hash_checked};
use crate::ir::{IRModule, Instr, ValueId};
use crate::types::{CanonicalModuleTypes, ScalarType, SchemaRegistryBuilder, SemanticType};

fn legacy_module() -> IRModule {
    let mut module = IRModule::new();
    module.exports.insert("M".to_string());
    module.instrs.push(Instr::ConstI64(ValueId(0), 42));
    module
}

fn unsupported_b1_module() -> IRModule {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut canonical = CanonicalModuleTypes::new(registry);
    canonical
        .set_module_value_type(ValueId(0), SemanticType::Scalar(ScalarType::I64))
        .expect("module value type");
    let mut module = IRModule::new();
    module.next_id = 1;
    module.instrs.push(Instr::ConstI64(ValueId(0), 42));
    module.canonical_types = Some(Box::new(canonical));
    module
}

fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn pinned_v03_fixtures() -> [(&'static str, Vec<u8>); 2] {
    [
        (
            "module-table",
            decode_hex("4d49433303000100010100540000000100010001"),
        ),
        (
            "fndef-table",
            decode_hex("4d49433303010766697874757265000001150000000001010054010001000100000000"),
        ),
    ]
}

#[test]
fn ordinary_legacy_body_has_exact_cursor_and_unchanged_bytes_and_hash() {
    let module = legacy_module();
    let legacy = emit_mic3(&module);
    let checked = emit_mic3_checked(&module).expect("legacy body remains encodable");
    assert_eq!(checked, legacy, "checked seam must not change legacy bytes");
    assert_eq!(
        ir_trace_hash_checked(&module).expect("legacy hash remains available"),
        ir_trace_hash(&module),
        "checked seam must not change the legacy trace hash"
    );

    let prefix = parse_mic3_prefix(&legacy).expect("body prefix");
    assert_eq!(prefix.consumed, legacy.len());
    parse_mic3_body(&legacy).expect("strict body");
    parse_mic3_envelope(&legacy).expect("plain body is a valid envelope");
}

#[test]
fn valid_map_uses_cursor_boundary_even_when_body_contains_map_byte() {
    let module = legacy_module();
    let body = emit_mic3(&module);
    assert!(
        body.contains(&MAP_SENTINEL),
        "the exported name M supplies an in-body sentinel-like byte"
    );
    let artifact = emit_mic3_with_evidence_checked(
        &module,
        "cpu",
        None,
        Determinism::Deterministic,
        "test-toolchain",
    )
    .expect("evidence artifact");
    let prefix = parse_mic3_prefix(&artifact).expect("body prefix");
    assert_eq!(prefix.consumed, body.len());
    assert_eq!(artifact[prefix.consumed], MAP_SENTINEL);
    parse_mic3_envelope(&artifact).expect("strict evidence envelope");
    let loaded = crate::ir::load(&artifact).expect("IRModule::load accepts evidence envelope");
    assert_eq!(loaded.instrs.len(), module.instrs.len());
    assert_eq!(loaded.exports, module.exports);
    assert!(parse_mic3_body(&artifact).is_err());
    parse_mic3(&artifact).expect("legacy parser preserves suffix compatibility");
}

#[test]
fn strict_envelope_rejects_non_map_and_malformed_suffixes() {
    let body = emit_mic3(&legacy_module());
    let mut trailing = body.clone();
    trailing.push(0x7f);
    assert!(matches!(
        parse_mic3_envelope(&trailing),
        Err(Mic3EnvelopeError::TrailingData { offset }) if offset == body.len()
    ));

    let mut malformed = body;
    malformed.extend_from_slice(&[MAP_SENTINEL, 0x00, 0xff]);
    assert!(matches!(
        parse_mic3_envelope(&malformed),
        Err(Mic3EnvelopeError::MalformedMap)
    ));

    for (bytes, expected) in [
        (trailing, "non-MAP data"),
        (malformed, "MAP epilogue is malformed"),
    ] {
        let error = crate::ir::load(&bytes).expect_err("malformed envelope must refuse");
        match error {
            crate::ir::LoadError::Mic3(error) => assert!(
                error.message.contains(expected),
                "unexpected load diagnostic: {}",
                error.message
            ),
            other => panic!("expected LoadError::Mic3, got {other:?}"),
        }
    }
}

#[cfg(feature = "std-surface")]
#[test]
fn std_v03_pinned_tables_are_full_roundtrips() {
    for (label, bytes) in pinned_v03_fixtures() {
        let prefix = parse_mic3_prefix(&bytes).unwrap_or_else(|error| {
            panic!("{label}: std parser rejected pinned v0x03 fixture: {error}")
        });
        assert_eq!(
            prefix.consumed,
            bytes.len(),
            "{label}: body must be fully consumed"
        );
        let parsed = parse_mic3_body(&bytes)
            .unwrap_or_else(|error| panic!("{label}: strict body parse failed: {error}"));
        assert_eq!(emit_mic3(&parsed), bytes, "{label}: v0x03 fixed point");
        parse_mic3_envelope(&bytes)
            .unwrap_or_else(|error| panic!("{label}: strict envelope parse failed: {error}"));
    }
}

#[cfg(not(feature = "std-surface"))]
#[test]
fn bare_refuses_v03_before_metadata_can_be_discarded() {
    let mut fixtures = pinned_v03_fixtures().map(|(_, bytes)| bytes).to_vec();
    fixtures.push(decode_hex("4d494333030000000000000000"));
    for bytes in fixtures {
        let prefix = parse_mic3_prefix(&bytes).expect_err("bare parser must refuse v0x03");
        assert!(
            prefix
                .message
                .contains("MIC3 version 0x03 requires the std-surface feature"),
            "unexpected prefix diagnostic: {}",
            prefix.message
        );

        for parsed in [parse_mic3(&bytes), parse_mic3_body(&bytes)] {
            let error = parsed.expect_err("legacy/body parser must preserve v0x03 refusal");
            assert!(
                error
                    .message
                    .contains("MIC3 version 0x03 requires the std-surface feature"),
                "unexpected parser diagnostic: {}",
                error.message
            );
        }

        let envelope = parse_mic3_envelope(&bytes).expect_err("envelope must preserve refusal");
        assert!(
            envelope
                .to_string()
                .contains("MIC3 version 0x03 requires the std-surface feature"),
            "unexpected envelope diagnostic: {envelope}"
        );

        let loaded = crate::ir::load(&bytes).expect_err("load must preserve refusal");
        match loaded {
            crate::ir::LoadError::Mic3(error) => assert!(
                error
                    .message
                    .contains("MIC3 version 0x03 requires the std-surface feature"),
                "unexpected load diagnostic: {}",
                error.message
            ),
            other => panic!("expected LoadError::Mic3, got {other:?}"),
        }
    }
}

#[test]
fn current_lenient_body_boundary_is_exact_while_map_boundaries_are_minimal() {
    let body = emit_mic3(&IRModule::new());
    assert_eq!(body[5], 0, "fixture starts with an empty string table");
    let mut nonminimal_body = body.clone();
    nonminimal_body.splice(5..6, [0x80, 0x00]);
    let prefix = parse_mic3_prefix(&nonminimal_body).expect("legacy body ULEB remains lenient");
    assert_eq!(prefix.consumed, nonminimal_body.len());
    parse_mic3_body(&nonminimal_body).expect("current body contract accepts padded ULEB");
    assert!(matches!(
        mic3_canonical_check(&nonminimal_body),
        Err(Mic3NonCanonical::Body { .. })
    ));

    let mut truncated_map = body.clone();
    truncated_map.push(MAP_SENTINEL);
    assert!(matches!(
        parse_mic3_envelope(&truncated_map),
        Err(Mic3EnvelopeError::MalformedMap)
    ));
    let mut nonminimal_map = body;
    nonminimal_map.extend_from_slice(&[MAP_SENTINEL, 0x80, 0x00]);
    assert!(matches!(
        parse_mic3_envelope(&nonminimal_map),
        Err(Mic3EnvelopeError::MalformedMap)
    ));
}

#[test]
fn checked_body_hash_and_evidence_seams_carry_v04_metadata() {
    let module = unsupported_b1_module();
    let body = emit_mic3_checked(&module).expect("v0x04 body");
    assert_eq!(body[4], super::MIC3_VERSION_V04);
    ir_trace_hash_checked(&module).expect("v0x04 trace hash");
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    let mut artifacts = vec![
        emit_mic3_with_evidence_checked(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "test-toolchain",
        )
        .expect("v0x04 evidence"),
        emit_mic3_with_evidence_and_receipts(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "test-toolchain",
            None,
            &[],
            &[],
        )
        .expect("v0x04 evidence with receipts"),
    ];
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    let artifacts = vec![
        emit_mic3_with_evidence_checked(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "test-toolchain",
        )
        .expect("v0x04 evidence"),
        emit_mic3_with_evidence_and_receipts(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "test-toolchain",
            None,
            &[],
            &[],
        )
        .expect("v0x04 evidence with receipts"),
    ];
    #[cfg(all(feature = "evidence-mldsa", feature = "evidence-slhdsa"))]
    artifacts.push(
        super::emit_mic3_with_signed_evidence_scheme(
            &module,
            "cpu",
            None,
            Determinism::Deterministic,
            "test-toolchain",
            &super::evidence::SigningKey::PqcHybrid {
                mldsa87: [7; 32],
                slhdsa: [8; 96],
            },
        )
        .expect("signed v0x04 evidence"),
    );
    #[cfg(not(all(feature = "evidence-mldsa", feature = "evidence-slhdsa")))]
    assert!(
        format!(
            "{:?}",
            super::emit_mic3_with_signed_evidence_checked(
                &module,
                "cpu",
                None,
                Determinism::Deterministic,
                "test-toolchain",
                &[7; 32],
            )
            .expect_err("retired Ed signing must refuse")
        )
        .contains("SchemeRetired")
    );
    for artifact in artifacts {
        parse_mic3_envelope(&artifact).expect("v0x04 evidence envelope");
        mic3_canonical_check(&artifact).expect("v0x04 canonical evidence");
    }
    crate::conformance::run_value_oracle("42", &module)
        .expect("conformance carries v0x04 metadata");
}
