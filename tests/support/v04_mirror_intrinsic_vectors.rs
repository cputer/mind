// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Exact canonical-intrinsic differential vectors for the pure-MIND mirror.

use libmind::ir::IRModule;
use libmind::ir::compact::v3::{emit_mic3_checked, parse_mic3_prefix};
use libmind::types::{
    CanonicalModuleTypes, FunctionDeclaration, FunctionIdentity, FunctionKind, FunctionSignature,
    ScalarType, SchemaError, SchemaRegistryBuilder, SemanticType,
};

use super::mirror_oracle::{synthetic_head, write_uleb};
use super::{Vector, code};

const SCALAR_I32: u8 = 0;
const SCALAR_I64: u8 = 1;

fn i64_type() -> SemanticType {
    SemanticType::Scalar(ScalarType::I64)
}

fn i64s(count: usize) -> Vec<SemanticType> {
    (0..count).map(|_| i64_type()).collect()
}

fn all_intrinsics_module() -> IRModule {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    for (name, arity) in [
        ("alloc", 1),
        ("argc", 0),
        ("argv", 1),
        ("load8", 1),
        ("load_i64", 1),
        ("open", 1),
        ("read", 4),
        ("store8", 2),
        ("store_i64", 2),
        ("write", 4),
    ] {
        bundle
            .add_declaration(FunctionDeclaration::new(
                FunctionIdentity::new("__mind_intrinsic", name),
                FunctionKind::Intrinsic,
                FunctionSignature::new(i64s(arity), Some(i64_type())),
            ))
            .unwrap_or_else(|error| panic!("exact intrinsic {name}: {error}"));
    }
    let mut module = IRModule::new();
    module.canonical_types = Some(Box::new(bundle));
    module
}

fn append_zero_body_tail(out: &mut Vec<u8>) {
    // next_id, exports, instructions, four fixed compatibility counts, and
    // module semantic values.
    out.extend_from_slice(&[0; 8]);
}

fn malformed_intrinsic(name: &str, parameters: &[u8], return_scalar: Option<u8>) -> Vec<u8> {
    // Every exercised logical name sorts after the reserved owner.
    let mut out = synthetic_head(&["__mind_intrinsic", name]);
    write_uleb(&mut out, 0); // schemas
    write_uleb(&mut out, 1); // functions
    write_uleb(&mut out, 0); // owner string
    write_uleb(&mut out, 1); // name string
    out.push(2); // intrinsic kind
    write_uleb(&mut out, parameters.len() as u64);
    for scalar in parameters {
        out.extend_from_slice(&[0, *scalar]);
    }
    match return_scalar {
        None => out.push(0),
        Some(scalar) => out.extend_from_slice(&[1, 0, scalar]),
    }
    append_zero_body_tail(&mut out);
    out
}

fn require_contract_refusal(name: &str, bytes: &[u8]) {
    let error = parse_mic3_prefix(bytes).expect_err("malformed intrinsic was admitted");
    assert!(
        error.to_string().contains("intrinsic"),
        "{name} must reach intrinsic validation, got: {error}"
    );
}

fn require_generic_encoder_refusal() {
    let registry = SchemaRegistryBuilder::default()
        .finish()
        .expect("empty registry");
    let mut bundle = CanonicalModuleTypes::new(registry);
    let error = bundle
        .add_declaration(FunctionDeclaration::new(
            FunctionIdentity::new("__mind_intrinsic", "load_i64").with_type_args(["T"]),
            FunctionKind::Intrinsic,
            FunctionSignature::new(i64s(1), Some(i64_type())),
        ))
        .expect_err("generic intrinsic acquired a wire representation");
    assert!(
        matches!(error, SchemaError::UnsupportedFunctionTypeArguments { .. }),
        "generic refusal must be attributable: {error}"
    );
}

pub(super) fn add_intrinsic_vectors(vectors: &mut Vec<Vector>) {
    require_generic_encoder_refusal();

    let all_valid = emit_mic3_checked(&all_intrinsics_module())
        .expect("reference emits all ten exact canonical intrinsic declarations");
    assert!(
        parse_mic3_prefix(&all_valid).is_ok(),
        "reference accepts its complete intrinsic catalog artifact"
    );
    vectors.push(Vector {
        name: "pos_full_body_intrinsic_catalog",
        bytes: all_valid,
        expect: code::REMAINDER_REFUSED,
        note: "all ten exact registered intrinsic names and i64 wire signatures",
    });

    for (name, bytes, note) in [
        (
            "neg_unknown_intrinsic",
            malformed_intrinsic("evil", &[], Some(SCALAR_I64)),
            "reserved intrinsic owner with an unregistered logical name",
        ),
        (
            "neg_intrinsic_wrong_arity",
            malformed_intrinsic("load_i64", &[], Some(SCALAR_I64)),
            "load_i64 with zero parameters instead of one",
        ),
        (
            "neg_intrinsic_wrong_param_type",
            malformed_intrinsic("load_i64", &[SCALAR_I32], Some(SCALAR_I64)),
            "load_i64 parameter is i32 instead of i64",
        ),
        (
            "neg_intrinsic_wrong_return_type",
            malformed_intrinsic("load_i64", &[SCALAR_I64], Some(SCALAR_I32)),
            "load_i64 return is i32 instead of i64",
        ),
        (
            "neg_intrinsic_missing_return",
            malformed_intrinsic("store_i64", &[SCALAR_I64, SCALAR_I64], None),
            "store_i64 omits its historical i64 wire return",
        ),
    ] {
        require_contract_refusal(name, &bytes);
        vectors.push(Vector {
            name,
            bytes,
            expect: code::INTRINSIC_CONTRACT,
            note,
        });
    }
}
