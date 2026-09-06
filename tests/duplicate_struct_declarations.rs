// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

use libmind::{parser, type_checker};

fn codes(source: &str) -> Vec<String> {
    let module = parser::parse(source).expect("duplicate-struct fixture must parse");
    type_checker::check_module_types(&module, source, &Default::default())
        .into_iter()
        .map(|diag| diag.code.to_string())
        .collect()
}

fn assert_one_duplicate(source: &str) {
    let module = parser::parse(source).expect("duplicate-struct fixture must parse");
    let diagnostics = type_checker::check_module_types(&module, source, &Default::default());
    let duplicates: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "E2035")
        .collect();
    assert_eq!(
        duplicates.len(),
        1,
        "expected one duplicate diagnostic: {diagnostics:?}"
    );
    assert!(duplicates[0].message.contains("Holder"));
    assert!(
        duplicates[0].span.is_some(),
        "duplicate must carry a source span"
    );
}

#[test]
fn duplicate_structs_are_rejected_in_both_source_orders() {
    assert_one_duplicate("struct Holder { xs: array<i64> }\nstruct Holder { ys: array<i64> }\n");
    assert_one_duplicate("struct Holder { ys: array<i64> }\nstruct Holder { xs: array<i64> }\n");
}

#[test]
fn transparent_inline_module_shares_the_struct_namespace() {
    assert_one_duplicate(
        "struct Holder { xs: array<i64> }\nmodule inner { struct Holder { ys: array<i64> } }\n",
    );
    assert_one_duplicate(
        "module inner { struct Holder { ys: array<i64> } }\nstruct Holder { xs: array<i64> }\n",
    );
}

#[test]
fn distinct_structs_and_independent_project_files_remain_valid() {
    let distinct = "struct A { value: i64 }\nstruct B { value: i64 }\n";
    assert!(!codes(distinct).iter().any(|code| code == "E2035"));

    // Project files are checked through separate module entry points. Reusing
    // a bare struct name in two files is valid; only one file's transparent
    // declaration namespace is checked by this pass.
    let file_a = "pub struct Holder { xs: array<i64> }\n";
    let file_b = "pub struct Holder { ys: array<i64> }\n";
    assert!(!codes(file_a).iter().any(|code| code == "E2035"));
    assert!(!codes(file_b).iter().any(|code| code == "E2035"));
}
