// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

#![cfg(feature = "std-surface")]

use libmind::{parser, type_checker};

#[test]
fn mismatched_literal_lengths_are_reported_at_every_binding_depth() {
    let cases = [
        "fn f() { let a: [u8; 4] = [0, 0, 0]; }",
        "fn f() { let a: [u8; 4] = [0, 0, 0, 0, 0]; }",
        "const a: [i64; 4] = [0, 0, 0];",
        "fn f() { let a: [i64; 4] = ([0, 0, 0]); }",
        "fn f() { if true { let a: [u8; 4] = [0, 0, 0]; } }",
        "fn f() { while false { let a: [u8; 4] = [0, 0, 0]; } }",
        "fn f() { for i in 0..2 { let a: [u8; 4] = [0, 0, 0]; } }",
        "fn f() { let a: [[i64; 2]; 1] = [[0]]; }",
    ];
    for source in cases {
        let module = parser::parse(source).expect(source);
        let diagnostics = type_checker::check_module_types(&module, source, &Default::default());
        let lengths: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("array length mismatch"))
            .collect();
        assert_eq!(
            lengths.len(),
            1,
            "one precise length error for {source}: {diagnostics:?}"
        );
        assert_eq!(lengths[0].code, "E2001");
    }
}

#[test]
fn exact_empty_and_runtime_initialized_arrays_remain_valid() {
    for source in [
        "fn f() { let a: [u8; 4] = [0, 0, 0, 0]; }",
        "const a: [i64; 0] = [];",
        "fn f() { let a: [[i64; 2]; 1] = [[0, 1]]; }",
        "fn f(a: [u8; 4]) { let b: [u8; 4] = a; }",
    ] {
        let module = parser::parse(source).expect(source);
        let diagnostics = type_checker::check_module_types(&module, source, &Default::default());
        assert!(
            diagnostics.is_empty(),
            "valid binding refused: {source}: {diagnostics:?}"
        );
    }
}
