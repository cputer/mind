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

/// A declaration may be DEFINED at most once, across sibling and nested bodies
/// alike.
///
/// The reference keeps one module-wide identity set for the whole instruction
/// stream (`canonical_verify.rs`), so the rule spans nesting levels. That is why
/// both a sibling pair and a nested pair are pinned here: a per-frame
/// implementation of this rule accepts exactly the bodies it exists to refuse,
/// and only the nested fixture can tell the two apart.
#[test]
fn canonical_function_identity_is_defined_at_most_once() {
    let distinct = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/pos_distinct_function_identities.mic3"
    );
    parse_mic3_body(distinct).expect("two distinct identities are a valid module");

    for (label, bytes) in [
        (
            "siblings",
            &include_bytes!(
                "../examples/mind_mirror_v04/testdata/semantic/neg_duplicate_function_siblings.mic3"
            )[..],
        ),
        (
            "nested",
            &include_bytes!(
                "../examples/mind_mirror_v04/testdata/semantic/neg_duplicate_function_nested.mic3"
            )[..],
        ),
    ] {
        let error = parse_mic3_body(bytes).expect_err("duplicate function identity");
        assert!(
            error.message.contains("duplicate") && error.message.contains("identity"),
            "{label}: the duplicate identity must be the diagnosed cause, not an \
             incidental refusal: {}",
            error.message
        );
    }
}

/// A `Return` outside any function body is refused, but only when the module
/// carries semantic authority.
///
/// The reference guards this rule on `authority_present`, so it is conditional,
/// not blanket. A body with no schema, no declaration and no module row is
/// refused for carrying no semantic authority at all rather than for the
/// return, and that distinction is why the mirror defers this verdict until the
/// module rows have been read instead of refusing inside the Return arm.
#[test]
fn canonical_module_scope_return_is_refused_under_authority() {
    let bytes = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_module_scope_return.mic3"
    );
    let error = parse_mic3_body(bytes).expect_err("module-scope return");
    assert!(
        error.message.contains("return") && error.message.contains("<module>"),
        "the module scope must be the diagnosed cause: {}",
        error.message
    );
}

/// A body carrying no schema, no declaration and no module semantic row is
/// refused for carrying no semantic authority.
///
/// This is the control that makes the module-scope return rule above FALSIFIABLE
/// as a CONDITIONAL rule. Without an authority-free body in the corpus, a
/// mutation collapsing that conditional into a blanket refusal passes every
/// vector; measured, it did, until this fixture existed.
#[test]
fn canonical_body_without_semantic_authority_is_refused() {
    let bytes = include_bytes!(
        "../examples/mind_mirror_v04/testdata/semantic/neg_no_semantic_authority.mic3"
    );
    let error = parse_mic3_body(bytes).expect_err("no semantic authority");
    assert!(
        error.message.contains("authority"),
        "authority must be the diagnosed cause, not the return rule: {}",
        error.message
    );
}

/// Module semantic rows share ONE cumulative element scope with every
/// declaration's signature, and a function's own scoped rows do not.
///
/// Three fixtures pin both directions of that scoping decision, because each
/// direction fails differently and each needs its own witness:
///   * exactly 2^40 across signatures alone is accepted;
///   * one module row on top of it is refused, which is what proves module rows
///     are IN the shared scope;
///   * a function's scoped row on top of it is accepted, which is what proves
///     those rows are OUT of it. Without this third fixture a mirror that
///     wrongly charged function rows into the shared scope passed every vector;
///     measured, it did.
#[test]
fn shared_element_scope_covers_module_rows_but_not_function_rows() {
    let boundary =
        include_bytes!("../examples/mind_mirror_v04/testdata/pos_module_row_element_boundary.mic3");
    parse_mic3_body(boundary).expect("exactly 2^40 elements is admissible");

    let outside = include_bytes!(
        "../examples/mind_mirror_v04/testdata/pos_function_rows_outside_shared_scope.mic3"
    );
    parse_mic3_body(outside).expect("function scoped rows are outside the shared scope");

    let over = include_bytes!(
        "../examples/mind_mirror_v04/testdata/neg_module_row_cumulative_elements.mic3"
    );
    let error = parse_mic3_body(over).expect_err("2^40 + 1 across the shared scope");
    assert!(
        error.message.contains("element count exceeds"),
        "the element bound must be the diagnosed cause: {}",
        error.message
    );
}
