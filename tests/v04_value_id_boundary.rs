// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Reference-decoder controls for the module `next_id` semantic boundary.
//!
//! These are deliberately separate from the 69 wire-mirror vectors.  They pin
//! the reference's `validate_core_value_ids` scope: module instructions and
//! module semantic rows share one exclusive bound, while function scopes do
//! not.  Duplicate instruction definitions are outside this predicate; the
//! general IR verifier owns that rule.

use libmind::ir::compact::v3::parse_mic3_body;
use libmind::ir::{Instr, ValueId};

fn uleb(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let low = (value & 0x7f) as u8;
        value >>= 7;
        out.push(if value == 0 { low } else { low | 0x80 });
        if value == 0 {
            return out;
        }
    }
}

fn body(next_id: u64, instruction_ids: &[u64], row_ids: &[u64]) -> Vec<u8> {
    let mut out = b"MIC3\x04\x00".to_vec();
    out.extend(uleb(0)); // strings
    out.extend(uleb(0)); // schemas
    out.extend(uleb(0)); // function declarations
    out.extend(uleb(next_id));
    out.extend(uleb(0)); // exports
    out.extend(uleb(instruction_ids.len() as u64));
    for id in instruction_ids {
        out.push(0x01); // ConstI64
        out.extend(uleb(*id));
        out.extend(uleb(84)); // zigzag(42)
    }
    out.extend([0, 0, 0, 0]); // reserved compatibility counts
    out.extend(uleb(row_ids.len() as u64));
    for id in row_ids {
        out.extend(uleb(*id));
        out.extend([0, 1]); // Scalar(I64)
    }
    out
}

#[test]
fn sparse_module_value_id_is_admitted_when_next_id_covers_it() {
    let bytes = body(301, &[300], &[300]);
    parse_mic3_body(&bytes).expect("sparse module ValueId 300 with next_id 301");
}

#[test]
fn function_scope_value_id_is_not_compared_with_module_next_id() {
    let bytes = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/pos_function_scope_id_not_module_bound.mic3"
    );
    let module = parse_mic3_body(bytes).expect("function-local ValueId 300 with module next_id 0");
    match module.instrs.first() {
        Some(Instr::FnDef {
            ret_id: Some(ret_id),
            ..
        }) => assert_eq!(*ret_id, ValueId(300)),
        other => panic!("positive fixture must carry a matching return ValueId: {other:?}"),
    }
}

#[test]
fn canonical_return_presence_and_descriptor_match_the_declaration() {
    for name in ["neg_return_missing_value", "neg_return_type_mismatch"] {
        let bytes = match name {
            "neg_return_missing_value" => &include_bytes!(
                "../examples/mind_mirror_v04/testdata/semantic/neg_return_missing_value.mic3"
            )[..],
            _ => &include_bytes!(
                "../examples/mind_mirror_v04/testdata/semantic/neg_return_type_mismatch.mic3"
            )[..],
        };
        let error = parse_mic3_body(bytes).expect_err("return metadata mismatch");
        assert!(
            error.message.contains("return type mismatch"),
            "{name} must identify the return/signature mismatch: {}",
            error.message
        );
    }
}

#[test]
fn canonical_function_name_matches_its_resolved_declaration() {
    let bytes = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_function_name_mismatch.mic3"
    );
    let error = parse_mic3_body(bytes).expect_err("function identity mismatch");
    assert!(
        error.message.contains("identity") && error.message.contains("match"),
        "identity mismatch must be diagnosed explicitly: {}",
        error.message
    );
}

#[test]
fn canonical_parameter_arity_matches_its_declaration() {
    let positive = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/pos_function_two_params.mic3"
    );
    parse_mic3_body(positive).expect("matching two-parameter declaration");

    let negative = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_function_parameter_arity.mic3"
    );
    let error = parse_mic3_body(negative).expect_err("parameter arity mismatch");
    assert!(
        error.message.contains("parameter") && error.message.contains("arity"),
        "arity mismatch must be diagnosed explicitly: {}",
        error.message
    );
}

#[test]
fn canonical_nonlocal_function_body_is_refused_by_declaration_kind() {
    let bytes = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_nonlocal_function_body.mic3"
    );
    let error = parse_mic3_body(bytes).expect_err("external declaration cannot carry a body");
    assert!(
        error.message.contains("External function") && error.message.contains("local body"),
        "nonlocal FnDef must identify the declaration-kind refusal: {}",
        error.message
    );
}

#[test]
fn nested_functions_keep_their_own_return_value_id() {
    let positive =
        include_bytes!("../examples/mind_mirror_v04/testdata/semantic/pos_nested_return_ids.mic3");
    parse_mic3_body(positive).expect("outer return 300 and nested return 301 use separate scopes");

    let negative = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_nested_outer_return_row.mic3"
    );
    let error =
        parse_mic3_body(negative).expect_err("nested return 301 cannot replace outer return 300");
    assert!(
        error.message.contains("return type mismatch"),
        "outer return must be checked against its own scoped rows: {}",
        error.message
    );
}

#[test]
fn module_next_id_refuses_a_missing_high_instruction_or_row_id() {
    for (label, bytes) in [
        ("instruction", body(300, &[300], &[300])),
        ("row", body(0, &[0], &[0])),
    ] {
        let error = parse_mic3_body(&bytes).expect_err("underbound module next_id");
        assert!(
            error.message.contains("next_id"),
            "{label} underbound must be diagnosed by the core ValueId check: {}",
            error.message
        );
    }
}

#[test]
fn duplicate_instruction_id_is_not_a_core_value_id_bound_rule() {
    let bytes = body(1, &[0, 0], &[0]);
    parse_mic3_body(&bytes)
        .expect("the reference core bound tracks the maximum, not definition uniqueness");
}

#[test]
fn duplicate_module_rows_are_refused_before_core_bound_check() {
    let bytes = body(1, &[0], &[0, 0]);
    let error = parse_mic3_body(&bytes).expect_err("duplicate semantic rows");
    assert!(
        error.message.contains("strictly sorted") || error.message.contains("duplicate"),
        "duplicate row refusal must identify row ordering: {}",
        error.message
    );
}
