// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

#![cfg(feature = "std-surface")]

use libmind::{parser, type_checker};

fn diagnostics(source: &str) -> Vec<libmind::diagnostics::Diagnostic> {
    let module = parser::parse(source).expect("fixed-array return source must parse");
    type_checker::check_module_types(&module, source, &Default::default())
}

fn array_field_errors(source: &str) -> Vec<libmind::diagnostics::Diagnostic> {
    diagnostics(source)
        .into_iter()
        .filter(|diag| {
            diag.message.contains("array element type mismatch")
                || diag.message.contains("array field type mismatch")
                || diag.message.contains("array length mismatch for")
        })
        .collect()
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

#[test]
fn fixed_i64_struct_field_checks_length_and_known_element_types() {
    let cases = [
        (
            "wrong field length",
            r#"
struct R { xs: [i64; 2] }
fn bad() -> i64 {
    let r: R = R { xs: [1] };
    return 0;
}
"#,
            "array length mismatch",
        ),
        (
            "float field elements",
            r#"
struct R { xs: [i64; 2] }
fn bad() -> i64 {
    let r: R = R { xs: [1.25, 2.5] };
    return 0;
}
"#,
            "array element type mismatch",
        ),
        (
            "bool field elements",
            r#"
struct R { xs: [i64; 2] }
fn bad() -> i64 {
    let r: R = R { xs: [true, false] };
    return 0;
}
"#,
            "array element type mismatch",
        ),
        (
            "record field elements",
            r#"
struct E { x: i64 }
struct R { xs: [i64; 2] }
fn bad() -> i64 {
    let e: E = E { x: 1 };
    let r: R = R { xs: [e, e] };
    return 0;
}
"#,
            "array element type mismatch",
        ),
        (
            "float alias array",
            r#"
type Fs = [f64; 2]
struct R { xs: [i64; 2] }
fn bad() -> i64 {
    let xs: Fs = [1.0, 2.0];
    let r: R = R { xs: xs };
    return 0;
}
"#,
            "array field type mismatch",
        ),
        (
            "float-returning call",
            r#"
struct R { xs: [i64; 2] }
fn wrong() -> f64 { return 1.0; }
fn bad() -> i64 {
    let r: R = R { xs: [wrong(), wrong()] };
    return 0;
}
"#,
            "array element type mismatch",
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
fn fixed_i64_struct_field_preserves_computed_and_array_alias_values() {
    let source = r#"
struct R { xs: [i64; 3] }
fn twice(x: i64) -> i64 { return x * 2; }
fn make(x: i64) -> [i64; 3] { return [x, twice(x), x + 2]; }
fn good(x: i64) -> i64 {
    let xs: [i64; 3] = make(x);
    let r: R = R { xs: xs };
    return r.xs[0] + r.xs[1] + r.xs[2];
}

fn looped(x: i64) -> i64 {
    let mut i: i64 = 0;
    while i < 2 {
        let r: R = R { xs: [i, x, i + 1] };
        i = i + r.xs[0] + 1;
    }
    return i;
}
"#;
    let diags = diagnostics(source);
    assert!(
        diags.is_empty(),
        "valid computed struct field refused: {diags:?}"
    );
}
#[test]
fn fixed_i64_struct_field_preserves_module_capture_and_unknown_values() {
    let cases = [
        (
            "module constant",
            r#"
const K: i64 = 3
struct R { xs: [i64; 1] }
fn good() -> i64 {
    let r: R = R { xs: [K] };
    return r.xs[0];
}
"#,
        ),
        (
            "nested function capture",
            r#"
struct R { xs: [i64; 1] }
fn outer(x: i64) -> i64 {
    fn inner() -> i64 {
        let r: R = R { xs: [x] };
        return r.xs[0];
    }
    return inner();
}
"#,
        ),
        (
            "foreach integer fact",
            r#"
struct R { xs: [i64; 1] }
fn good() -> i64 {
    let values: [i64; 1] = [7];
    for value in values {
        let r: R = R { xs: [value] };
    }
    return 0;
}
"#,
        ),
        (
            "value-if expression deferred",
            r#"
struct R { xs: [i64; 1] }
fn good(flag: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { 1 } else { 2 }] };
    return r.xs[0];
}
"#,
        ),
        (
            "explicit range widening",
            r#"
struct R { xs: [i64; 1] }
fn good() -> i64 {
    for i in 0..1 {
        let r: R = R { xs: [i as i64] };
    }
    return 0;
}
"#,
        ),
    ];

    for (label, source) in cases {
        let errors = array_field_errors(source);
        assert!(errors.is_empty(), "{label} was falsely refused: {errors:?}");
    }
}

#[test]
fn fixed_i64_struct_field_uses_lexically_nearest_function_return() {
    let nested_float = r#"
struct R { xs: [i64; 1] }
fn value() -> i64 { return 1; }
fn bad() -> i64 {
    fn value() -> f64 { return 1.5; }
    let r: R = R { xs: [value()] };
    return 0;
}
"#;
    let errors = array_field_errors(nested_float);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "nested f64 shadow must not inherit the outer i64 signature: {errors:?}"
    );

    let nested_integer = r#"
struct R { xs: [i64; 1] }
fn value() -> f64 { return 1.5; }
fn good() -> i64 {
    fn value() -> i64 { return 1; }
    let r: R = R { xs: [value()] };
    return r.xs[0];
}
"#;
    let errors = array_field_errors(nested_integer);
    assert!(
        errors.is_empty(),
        "nested i64 shadow was replaced by the outer f64 signature: {errors:?}"
    );

    let sibling_restoration = r#"
struct R { xs: [i64; 1] }
fn value() -> f64 { return 1.5; }
fn left() -> i64 {
    fn value() -> i64 { return 1; }
    let r: R = R { xs: [value()] };
    return r.xs[0];
}
fn right() -> i64 {
    let r: R = R { xs: [value()] };
    return 0;
}
"#;
    let errors = array_field_errors(sibling_restoration);
    assert_eq!(
        errors
            .iter()
            .filter(|diag| diag.message.contains("array element type mismatch"))
            .count(),
        1,
        "nested signature leaked into its sibling: {errors:?}"
    );
}

#[test]
fn fixed_i64_struct_field_respects_range_variable_width() {
    let source = r#"
struct R { xs: [i64; 1] }
fn bad() -> i64 {
    for i in 0..1 {
        let r: R = R { xs: [i] };
    }
    return 0;
}
"#;
    let errors = array_field_errors(source);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "the established i32 range variable requires an explicit i64 cast: {errors:?}"
    );
}

#[test]
fn fixed_i64_struct_field_checks_known_control_flow_tails() {
    let both_branches_bad = r#"
struct R { xs: [i64; 1] }
fn bad(flag: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { 1.5 } else { 2.5 }] };
    return r.xs[0];
}
"#;
    let errors = array_field_errors(both_branches_bad);
    assert_eq!(
        errors
            .iter()
            .filter(|diag| diag.message.contains("array element type mismatch"))
            .count(),
        2,
        "each known-bad conditional branch must be checked: {errors:?}"
    );

    let loop_bad = r#"
struct R { xs: [i64; 1] }
fn bad(flag: i64) -> i64 {
    let mut i: i64 = 0;
    while i < 1 {
        let r: R = R { xs: [if flag > 0 { 1.5 } else { 1 }] };
        i = i + r.xs[0] + 1;
    }
    return 0;
}
"#;
    let errors = array_field_errors(loop_bad);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "a known-bad branch inside a loop must be checked: {errors:?}"
    );

    let bool_branch = r#"
struct R { xs: [i64; 1] }
fn bad(flag: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { true } else { 1 }] };
    return r.xs[0];
}
"#;
    let errors = array_field_errors(bool_branch);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "a known-bad boolean branch must be checked: {errors:?}"
    );

    let mixed_unknown_and_bad = r#"
struct R { xs: [i64; 1] }
fn bad(flag: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { unknown_value } else { 2.5 }] };
    return r.xs[0];
}
"#;
    let errors = array_field_errors(mixed_unknown_and_bad);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "a known bad branch must not be hidden by an unknown sibling: {errors:?}"
    );

    let local_shadow = r#"
struct R { xs: [i64; 1] }
fn bad(flag: i64, x: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { let x: f64 = 1.5; x } else { x }] };
    return r.xs[0];
}
"#;
    let errors = array_field_errors(local_shadow);
    assert!(
        errors
            .iter()
            .any(|diag| diag.message.contains("array element type mismatch")),
        "branch-local f64 shadow must be checked: {errors:?}"
    );

    let valid = r#"
struct R { xs: [i64; 1] }
fn good(flag: i64, x: i64) -> i64 {
    let r: R = R { xs: [if flag > 0 { 1 } else { x + 1 }] };
    return r.xs[0];
}
"#;
    assert!(
        array_field_errors(valid).is_empty(),
        "known i64 conditional branches must remain valid"
    );
}

#[test]
fn fixed_i64_array_return_checks_known_control_flow_tails() {
    let source = r#"
fn bad(flag: i64) -> [i64; 1] {
    return [if flag > 0 { 1 } else { 2.5 }];
}
"#;
    let errors = diagnostics(source);
    assert!(
        errors.iter().any(|diag| {
            diag.code == "E2001" && diag.message.contains("array element type mismatch")
        }),
        "conditional float in an integer array return must be rejected: {errors:?}"
    );
}
