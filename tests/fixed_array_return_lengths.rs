// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

#![cfg(feature = "std-surface")]

use libmind::{parser, type_checker};

fn diagnostics(source: &str) -> Vec<libmind::diagnostics::Diagnostic> {
    let module = parser::parse(source).expect("fixed-array return source must parse");
    type_checker::check_module_types(&module, source, &Default::default())
}

#[test]
fn fixed_array_return_shape_and_element_errors_are_check_time_errors() {
    let cases = [
        (
            "explicit parenthesized return",
            "pub fn bad() -> [i64; 3] { return ([1, 2]); }",
            "array length mismatch",
        ),
        (
            "implicit parenthesized return",
            "pub fn bad() -> [i64; 3] { ([1, 2]) }",
            "array length mismatch",
        ),
        (
            "alias return",
            "type Triple = [i64; 3]\npub fn bad() -> Triple { return ([1, 2]); }",
            "array length mismatch",
        ),
        (
            "integer return with float elements",
            "pub fn bad() -> [i64; 2] { return [1.25, 2.5]; }",
            "array element type mismatch",
        ),
        (
            "negative float element",
            "pub fn bad() -> [i64; 1] { return [(-1.25)]; }",
            "array element type mismatch",
        ),
        (
            "named narrow integer element",
            "pub fn bad() -> [u8; 1] { return [1.25]; }",
            "array element type mismatch",
        ),
        (
            "nested alias return",
            "type Row = [i64; 2]\ntype Matrix = [Row; 2]\npub fn bad() -> Matrix { return [[1], [2]]; }",
            "array length mismatch",
        ),
    ];
    for (label, source, expected) in cases {
        let diags = diagnostics(source);
        assert!(
            diags
                .iter()
                .any(|d| d.code == "E2001" && d.message.contains(expected)),
            "{label} should fail at check time: {diags:?}"
        );
    }
}

#[test]
fn fixed_array_return_computed_integer_and_float_values_remain_valid() {
    let cases = [
        "pub fn good(x: i64) -> [i64; 3] { return ([x, x + 1, x + 2]); }",
        "pub fn good() -> [f64; 2] { return [1.25, 2.5]; }",
        "pub fn good() -> [f64; 2] { return [1, 2]; }",
        "pub fn good(x: f64) -> [i64; 1] { let x: i64 = 7; return [x]; }",
        "type Triple = [i64; 3]\npub fn good(x: i64) -> Triple { ([x, x + 1, x + 2]) }",
        "pub fn scalar() -> i64 { return 7; }",
    ];
    for source in cases {
        let diags = diagnostics(source);
        assert!(
            diags.is_empty(),
            "valid fixed-array return refused: {source}: {diags:?}"
        );
    }

    let scalar_diags = diagnostics("pub fn scalar() -> i64 { return 1.25; }");
    assert!(
        scalar_diags
            .iter()
            .all(|d| !d.message.contains("array element type mismatch")),
        "scalar return took the fixed-array path: {scalar_diags:?}"
    );
}
